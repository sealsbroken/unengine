//! Desktop audio: the microphone and the speakers through cpal, wired to the engine's traits.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel_shim::Channel;
use parking_lot::{Mutex, RwLock};
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

/// Microphone -> Silero VAD -> Zipformer, one utterance at a time; an [`un_engine::SpeechInput`].
pub struct MicrophoneInput {
    models: SpeechModels,
    device: MicDevice,
    recognizers: RwLock<HashMap<Language, Arc<Recognizer>>>,
    device_name: Mutex<String>,
    level: Mutex<f32>,
    active: Mutex<Option<Arc<CancelToken>>>,
    /// Quiet microphones (Bluetooth headsets, laptops far away) are boosted up to this much before the VAD.
    pub max_gain: f32,
    pub target_peak: f32,
}

impl MicrophoneInput {
    pub fn new(models: SpeechModels, device: MicDevice) -> Self {
        Self {
            models,
            device,
            recognizers: RwLock::new(HashMap::new()),
            device_name: Mutex::new(String::new()),
            level: Mutex::new(0.0),
            active: Mutex::new(None),
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

    /// Records until `accept` returns a value for a transcript, the timeout, or cancellation.
    fn listen<T>(
        &self,
        language: Language,
        timeout: Duration,
        cancel: &CancelToken,
        accept: &dyn Fn(&str) -> Option<T>,
    ) -> Result<(T, String), SpeechError> {
        let recognizer = self.recognizer(language)?;
        let device = self.input_device()?;
        *self.device_name.lock() = device.description().map(|d| d.name().to_string()).unwrap_or_default();
        let config = device.default_input_config().map_err(|e| SpeechError::AudioEngineFailed(e.to_string()))?;
        let channels = config.channels() as usize;
        let rate = config.sample_rate();
        let queue: Arc<Channel<Vec<f32>>> = Arc::new(Channel::default());
        let producer = Arc::clone(&queue);
        let failed = Arc::new(AtomicBool::new(false));
        let failed_cb = Arc::clone(&failed);
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
            .map_err(|e| SpeechError::AudioEngineFailed(format!("microphone: {e}")))?;
        stream.play().map_err(|e| SpeechError::AudioEngineFailed(format!("microphone: {e}")))?;

        let mut vad = recognizer.vad()?;
        let deadline = Instant::now() + timeout;
        let mut pending: Vec<f32> = Vec::new();
        let mut last_transcript: Option<String> = None;
        let (mut gain, mut peak) = (1.0f32, 0.0f32);
        let result = loop {
            if cancel.is_cancelled() {
                break Err(SpeechError::Cancelled);
            }
            if failed.load(Ordering::SeqCst) {
                break Err(SpeechError::AudioEngineFailed("microphone stream stopped".into()));
            }
            if Instant::now() >= deadline {
                break Err(SpeechError::Timeout { last_transcript: last_transcript.clone() });
            }
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
            }
            let mut heard = None;
            while !vad.is_empty() {
                let segment = vad.front();
                vad.pop();
                let text = recognizer.decode(&segment.samples)?;
                if text.is_empty() {
                    continue;
                }
                last_transcript = Some(text.clone());
                if let Some(value) = accept(&text) {
                    heard = Some((value, text));
                    break;
                }
            }
            if let Some(h) = heard {
                break Ok(h);
            }
        };
        drop(stream);
        *self.level.lock() = 0.0;
        result
    }
}

impl SpeechInput for MicrophoneInput {
    fn prepare(&self, language: Language) -> Result<(), SpeechError> {
        self.recognizer(language)?;
        let device = self.input_device()?;
        *self.device_name.lock() = device.description().map(|d| d.name().to_string()).unwrap_or_default();
        device.default_input_config().map_err(|e| SpeechError::AudioEngineFailed(format!("no microphone: {e}")))?;
        Ok(())
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

    fn device_name(&self) -> String {
        self.device_name.lock().clone()
    }

    fn level(&self) -> f32 {
        *self.level.lock()
    }
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
