//! Desktop audio: the microphone and the speakers through cpal, wired to the engine's traits.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel_shim::Channel;
use parking_lot::{Condvar, Mutex, RwLock};
use un_core::numbers::tokenize;
use un_core::{Language, NumberParser};
use un_engine::{CancelToken, NumberHeard, SpeechError, SpeechInput, SpeechOutput};

use crate::models::SpeechModels;
use crate::recognizer::{Recognizer, CHUNK};
use crate::voice::Voice;
use crate::{chime, resample, SAMPLE_RATE};

mod crossbeam_channel_shim {
    //! A tiny bounded queue so the audio callback never blocks.
    use parking_lot::{Condvar, Mutex};
    use std::collections::VecDeque;
    use std::time::Duration;

    pub struct Channel<T> {
        queue: Mutex<VecDeque<T>>,
        cv: Condvar,
    }

    impl<T> Default for Channel<T> {
        fn default() -> Self {
            Self { queue: Mutex::new(VecDeque::new()), cv: Condvar::new() }
        }
    }

    impl<T> Channel<T> {
        pub fn push(&self, item: T) {
            let mut q = self.queue.lock();
            if q.len() > 256 {
                q.pop_front();
            }
            q.push_back(item);
            self.cv.notify_one();
        }

        pub fn pop(&self, timeout: Duration) -> Option<T> {
            let mut q = self.queue.lock();
            if q.is_empty() {
                self.cv.wait_for(&mut q, timeout);
            }
            q.pop_front()
        }
    }
}

/// Which microphone to use.
#[derive(Debug, Clone, Default)]
pub enum MicDevice {
    #[default]
    Default,
    /// Substring of the device name.
    Named(String),
}

/// One decoded utterance from the always-on stream.
struct Utterance {
    text: String,
    started: Instant,
    ended: Instant,
}

/// Something the tutor said, for telling its voice apart from the child's.
struct Spoken {
    numbers: Vec<String>,
    at: Instant,
}

/// The always-on capture thread: it owns the cpal stream (which cannot leave its thread).
struct Capture {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    language: Language,
    device_name: String,
    failed: Arc<AtomicBool>,
}

impl Capture {
    fn finish(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.finish();
    }
}

type Heard = (Mutex<VecDeque<Utterance>>, Condvar);

/// Microphone -> Silero VAD -> Zipformer, always on; an [`un_engine::SpeechInput`].
///
/// The stream opens at [`SpeechInput::prepare`] and stays open, decoding every utterance as it
/// ends, so a listen starts instantly. An answer often overlaps the tutor's last words and lands
/// in the same voice segment as them; such a segment is kept, with the number words the tutor
/// just said removed, so the tutor's own voice from the speakers is never taken as the answer.
pub struct MicrophoneInput {
    models: SpeechModels,
    device: MicDevice,
    recognizers: RwLock<HashMap<Language, Arc<Recognizer>>>,
    device_name: Mutex<String>,
    level: Arc<Mutex<f32>>,
    active: Mutex<Option<Arc<CancelToken>>>,
    capture: Mutex<Option<Capture>>,
    heard: Arc<Heard>,
    spoken: Mutex<VecDeque<Spoken>>,
    last_heard: Mutex<(String, Instant)>,
    /// Quiet microphones (Bluetooth headsets, laptops far away) are boosted up to this much before the VAD.
    pub max_gain: f32,
    pub target_peak: f32,
}

/// How long the tutor's words are remembered for filtering.
const SPOKEN_MEMORY: Duration = Duration::from_secs(20);

impl MicrophoneInput {
    pub fn new(models: SpeechModels, device: MicDevice) -> Self {
        Self {
            models,
            device,
            recognizers: RwLock::new(HashMap::new()),
            device_name: Mutex::new(String::new()),
            level: Arc::new(Mutex::new(0.0)),
            active: Mutex::new(None),
            capture: Mutex::new(None),
            heard: Arc::new((Mutex::new(VecDeque::new()), Condvar::new())),
            spoken: Mutex::new(VecDeque::new()),
            last_heard: Mutex::new((String::new(), Instant::now())),
            max_gain: 12.0,
            target_peak: 0.3,
        }
    }

    pub fn list_devices() -> Vec<String> {
        let host = cpal::default_host();
        host.input_devices()
            .map(|d| d.filter_map(|d| d.description().ok().map(|d| d.name().to_string())).collect())
            .unwrap_or_default()
    }

    fn recognizer(&self, language: Language) -> Result<Arc<Recognizer>, SpeechError> {
        if let Some(r) = self.recognizers.read().get(&language) {
            return Ok(Arc::clone(r));
        }
        let r = Arc::new(Recognizer::load(&self.models, language)?);
        self.recognizers.write().insert(language, Arc::clone(&r));
        Ok(r)
    }

    fn input_device(&self) -> Result<cpal::Device, SpeechError> {
        let host = cpal::default_host();
        let device = match &self.device {
            MicDevice::Default => host.default_input_device(),
            MicDevice::Named(name) => {
                let wanted = name.to_lowercase();
                host.input_devices()
                    .map_err(|e| SpeechError::AudioEngineFailed(e.to_string()))?
                    .find(|d| d.description().map(|d| d.name().to_lowercase().contains(&wanted)).unwrap_or(false))
            }
        };
        device.ok_or_else(|| SpeechError::AudioEngineFailed("no microphone".into()))
    }

    /// Makes sure the stream runs on the wanted device for `language`, restarting it when the
    /// device changed (a headset plugged in, a new system default) or it failed.
    fn ensure_capture(&self, language: Language) -> Result<(), SpeechError> {
        let recognizer = self.recognizer(language)?;
        let device = self.input_device()?;
        let name = device.description().map(|d| d.name().to_string()).unwrap_or_default();
        let mut capture = self.capture.lock();
        if let Some(c) = capture.as_mut() {
            if c.language == language && c.device_name == name && !c.failed.load(Ordering::SeqCst) {
                return Ok(());
            }
            c.finish();
        }
        *capture = None;
        *self.device_name.lock() = name.clone();
        let config = device.default_input_config().map_err(|e| SpeechError::AudioEngineFailed(format!("no microphone: {e}")))?;
        let mut vad = recognizer.vad()?;
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), SpeechError>>();
        let worker = Worker {
            stop: Arc::clone(&stop),
            failed: Arc::clone(&failed),
            heard: Arc::clone(&self.heard),
            level: Arc::clone(&self.level),
            max_gain: self.max_gain,
            target_peak: self.target_peak,
        };
        let thread = thread::Builder::new()
            .name("un-microphone".into())
            .spawn(move || worker.run(device, config, recognizer, &mut vad, ready_tx))
            .map_err(|e| SpeechError::AudioEngineFailed(format!("microphone thread: {e}")))?;
        ready_rx.recv().unwrap_or_else(|_| Err(SpeechError::AudioEngineFailed("microphone thread died".into())))?;
        *capture = Some(Capture { stop, thread: Some(thread), language, device_name: name, failed });
        Ok(())
    }

    /// Number words the tutor said recently, removed from utterances that overlap its speech.
    fn tutor_numbers(&self, now: Instant) -> Vec<String> {
        let mut spoken = self.spoken.lock();
        while spoken.front().is_some_and(|s| now.duration_since(s.at) > SPOKEN_MEMORY) {
            spoken.pop_front();
        }
        spoken.iter().flat_map(|s| s.numbers.iter().cloned()).collect()
    }

    /// Waits for the first utterance still going on, or starting, after the listen began that
    /// `accept` turns into a value; the timeout and cancellation end it.
    fn listen<T>(
        &self,
        language: Language,
        timeout: Duration,
        cancel: &CancelToken,
        accept: &dyn Fn(&str) -> Option<T>,
    ) -> Result<(T, String), SpeechError> {
        self.ensure_capture(language)?;
        let since = Instant::now();
        let deadline = since + timeout;
        let mut last_transcript: Option<String> = None;
        let (queue, cv) = &*self.heard;
        let mut queue = queue.lock();
        loop {
            while let Some(u) = queue.pop_front() {
                if u.ended < since {
                    continue;
                }
                let text = if u.started < since {
                    // Overlaps the tutor: drop the numbers the tutor just said, keep the child's.
                    let tutor = self.tutor_numbers(since);
                    without_words(&u.text, &tutor)
                } else {
                    u.text.clone()
                };
                last_transcript = Some(u.text.clone());
                *self.last_heard.lock() = (u.text.clone(), Instant::now());
                if let Some(value) = accept(&text) {
                    return Ok((value, u.text));
                }
            }
            if cancel.is_cancelled() {
                return Err(SpeechError::Cancelled);
            }
            if self.capture.lock().as_ref().is_none_or(|c| c.failed.load(Ordering::SeqCst)) {
                return Err(SpeechError::AudioEngineFailed("microphone stream stopped".into()));
            }
            if Instant::now() >= deadline {
                return Err(SpeechError::Timeout { last_transcript });
            }
            cv.wait_for(&mut queue, Duration::from_millis(50));
        }
    }

    /// Stops the stream; `prepare` or the next listen reopens it.
    pub fn close(&self) {
        *self.capture.lock() = None;
        *self.level.lock() = 0.0;
    }
}

impl Drop for MicrophoneInput {
    fn drop(&mut self) {
        self.close();
    }
}

/// What the capture thread needs; it reads the stream, runs the VAD and decodes utterances.
struct Worker {
    stop: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    heard: Arc<Heard>,
    level: Arc<Mutex<f32>>,
    max_gain: f32,
    target_peak: f32,
}

impl Worker {
    fn run(
        self,
        device: cpal::Device,
        config: cpal::SupportedStreamConfig,
        recognizer: Arc<Recognizer>,
        vad: &mut sherpa_rs::silero_vad::SileroVad,
        ready: std::sync::mpsc::Sender<Result<(), SpeechError>>,
    ) {
        let channels = config.channels() as usize;
        let rate = config.sample_rate();
        let queue: Arc<Channel<Vec<f32>>> = Arc::new(Channel::default());
        let producer = Arc::clone(&queue);
        let failed_cb = Arc::clone(&self.failed);
        let stream = device
            .build_input_stream(
                config.config(),
                move |data: &[f32], _| {
                    let mono: Vec<f32> = data.chunks(channels).map(|frame| frame.iter().sum::<f32>() / channels as f32).collect();
                    producer.push(mono);
                },
                move |error| {
                    log::warn!("microphone stream error: {error}");
                    failed_cb.store(true, Ordering::SeqCst);
                },
                None,
            )
            .map_err(|e| SpeechError::AudioEngineFailed(format!("microphone: {e}")))
            .and_then(|s| s.play().map(|_| s).map_err(|e| SpeechError::AudioEngineFailed(format!("microphone: {e}"))));
        let stream = match stream {
            Ok(stream) => {
                let _ = ready.send(Ok(()));
                stream
            }
            Err(error) => {
                let _ = ready.send(Err(error));
                return;
            }
        };

        let mut pending: Vec<f32> = Vec::new();
        let (mut gain, mut peak) = (1.0f32, 0.0f32);
        let mut fed: usize = 0; // samples given to the VAD; its segment starts count the same way
        while !self.stop.load(Ordering::SeqCst) && !self.failed.load(Ordering::SeqCst) {
            let Some(block) = queue.pop(Duration::from_millis(50)) else { continue };
            let block = if rate == SAMPLE_RATE { block } else { resample(&block, rate, SAMPLE_RATE) };
            // Automatic gain: track the recent peak and lift quiet audio towards the target.
            let block_peak = block.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            peak = block_peak.max(peak * 0.995);
            if peak > 1e-4 {
                let wanted = (self.target_peak / peak).clamp(1.0, self.max_gain);
                gain += 0.2 * (wanted - gain);
            }
            *self.level.lock() = (block_peak * gain).min(1.0);
            pending.extend(block.iter().map(|v| (v * gain).clamp(-1.0, 1.0)));
            while pending.len() >= CHUNK {
                let chunk: Vec<f32> = pending.drain(..CHUNK).collect();
                vad.accept_waveform(chunk);
                fed += CHUNK;
            }
            let now = Instant::now();
            while !vad.is_empty() {
                let segment = vad.front();
                vad.pop();
                let behind = fed.saturating_sub(segment.start.max(0) as usize) as f64 / SAMPLE_RATE as f64;
                let started = now.checked_sub(Duration::from_secs_f64(behind)).unwrap_or(now);
                let length = Duration::from_secs_f64(segment.samples.len() as f64 / SAMPLE_RATE as f64);
                let ended = started + length;
                match recognizer.decode(&segment.samples) {
                    Ok(text) if !text.is_empty() => {
                        let (queue, cv) = &*self.heard;
                        let mut queue = queue.lock();
                        if queue.len() > 32 {
                            queue.pop_front();
                        }
                        queue.push_back(Utterance { text, started, ended });
                        cv.notify_all();
                    }
                    Ok(_) => {}
                    Err(error) => log::warn!("recogniser: {error}"),
                }
            }
        }
        drop(stream);
        *self.level.lock() = 0.0;
    }
}

impl SpeechInput for MicrophoneInput {
    fn prepare(&self, language: Language) -> Result<(), SpeechError> {
        self.ensure_capture(language)
    }

    fn listen_for_number(&self, language: Language, timeout: Duration, cancel: &CancelToken) -> Result<NumberHeard, SpeechError> {
        self.stop();
        *self.active.lock() = None;
        let (value, transcript) = self.listen(language, timeout, cancel, &|t| NumberParser::parse(t, language))?;
        Ok(NumberHeard { value, transcript })
    }

    fn listen_for_text(
        &self,
        language: Language,
        timeout: Duration,
        cancel: &CancelToken,
        accept: &dyn Fn(&str) -> bool,
    ) -> Result<String, SpeechError> {
        let (text, _) = self.listen(language, timeout, cancel, &|t| accept(t).then(|| t.to_string()))?;
        Ok(text)
    }

    fn stop(&self) {
        if let Some(token) = self.active.lock().take() {
            token.cancel();
        }
    }

    fn tutor_speaking(&self, text: &str, language: Language) {
        let numbers: Vec<String> = tokenize(text).into_iter().filter(|w| NumberParser::parse(w, language).is_some()).collect();
        if numbers.is_empty() {
            return;
        }
        let mut spoken = self.spoken.lock();
        spoken.push_back(Spoken { numbers, at: Instant::now() });
        while spoken.len() > 8 {
            spoken.pop_front();
        }
    }

    fn device_name(&self) -> String {
        self.device_name.lock().clone()
    }

    fn level(&self) -> f32 {
        *self.level.lock()
    }

    fn last_transcript(&self) -> String {
        let (text, at) = &*self.last_heard.lock();
        if at.elapsed() < Duration::from_secs(4) {
            text.clone()
        } else {
            String::new()
        }
    }
}

/// `text` with every token in `words` removed.
fn without_words(text: &str, words: &[String]) -> String {
    tokenize(text).into_iter().filter(|t| !words.contains(t)).collect::<Vec<_>>().join(" ")
}

/// Piper voices played through the default output; an [`un_engine::SpeechOutput`].
pub struct SpeakerOutput {
    models: SpeechModels,
    voices: RwLock<HashMap<Language, Arc<Voice>>>,
    /// Below 1 is slower, which suits young children.
    pub speed: f32,
    stop: Mutex<Arc<AtomicBool>>,
    playing: Mutex<()>,
}

impl SpeakerOutput {
    pub fn new(models: SpeechModels) -> Self {
        Self {
            models,
            voices: RwLock::new(HashMap::new()),
            speed: 0.9,
            stop: Mutex::new(Arc::new(AtomicBool::new(false))),
            playing: Mutex::new(()),
        }
    }

    fn voice(&self, language: Language) -> Result<Arc<Voice>, SpeechError> {
        if let Some(v) = self.voices.read().get(&language) {
            return Ok(Arc::clone(v));
        }
        let v = Arc::new(Voice::load(&self.models, language)?);
        self.voices.write().insert(language, Arc::clone(&v));
        Ok(v)
    }

    /// Plays mono samples on the default output; returns early when stopped.
    pub fn play(&self, samples: &[f32], rate: u32) -> Result<(), SpeechError> {
        let _guard = self.playing.lock();
        let stop = Arc::new(AtomicBool::new(false));
        *self.stop.lock() = Arc::clone(&stop);
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or_else(|| SpeechError::AudioEngineFailed("no speakers".into()))?;
        let config = device.default_output_config().map_err(|e| SpeechError::AudioEngineFailed(e.to_string()))?;
        let channels = config.channels() as usize;
        let out_rate = config.sample_rate();
        let samples = if out_rate == rate { samples.to_vec() } else { resample(samples, rate, out_rate) };
        let total = samples.len();
        let position = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let pos_cb = Arc::clone(&position);
        let stream = device
            .build_output_stream(
                config.config(),
                move |data: &mut [f32], _| {
                    let mut p = pos_cb.load(Ordering::Relaxed);
                    for frame in data.chunks_mut(channels) {
                        let v = if p < total { samples[p] } else { 0.0 };
                        p += 1;
                        for out in frame.iter_mut() {
                            *out = v;
                        }
                    }
                    pos_cb.store(p, Ordering::Relaxed);
                },
                |error| log::warn!("playback error: {error}"),
                None,
            )
            .map_err(|e| SpeechError::AudioEngineFailed(format!("speakers: {e}")))?;
        stream.play().map_err(|e| SpeechError::AudioEngineFailed(format!("speakers: {e}")))?;
        while position.load(Ordering::Relaxed) < total + out_rate as usize / 10 {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(stream);
        Ok(())
    }
}

impl SpeechOutput for SpeakerOutput {
    fn prepare(&self, language: Language) -> Result<(), SpeechError> {
        self.voice(language)?;
        Ok(())
    }

    fn speak(&self, text: &str, language: Language) -> Result<(), SpeechError> {
        let voice = self.voice(language)?;
        let audio = voice.synthesize(text, self.speed)?;
        self.play(&audio.samples, audio.sample_rate)
    }

    fn stop(&self) {
        self.stop.lock().store(true, Ordering::SeqCst);
    }

    fn play_chime(&self) {
        if let Err(error) = self.play(&chime::samples(), chime::RATE) {
            log::warn!("chime: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tutor_numbers_are_removed_from_an_overlapping_utterance() {
        let tutor: Vec<String> = tokenize("I see five caps. Take one away.")
            .into_iter()
            .filter(|w| NumberParser::parse(w, Language::English).is_some())
            .collect();
        assert_eq!(tutor, vec!["five", "one"]);
        let heard = without_words("i see five caps take one away ten", &tutor);
        assert_eq!(NumberParser::parse(&heard, Language::English), Some(10));
        let tutor_only = without_words("i see five caps take one away", &tutor);
        assert_eq!(NumberParser::parse(&tutor_only, Language::English), None);
    }
}
