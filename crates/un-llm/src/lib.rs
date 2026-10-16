//! The tutor's answers to free speech from a local language model, on llama.cpp.
//!
//! [`LlamaChat`] implements the engine's [`ChatModel`]: it applies the model's own chat template
//! to the strategy's master prompt and the conversation so far, and samples a short answer.
//! A model is loaded on the first request ([`OnDemand`]), then shared for the process ([`LlamaChat::shared`]); one answer is
//! generated at a time, on the engine's chat thread, and the context is kept between answers so
//! the master prompt is read once, not every time. Any instruction-tuned GGUF works; the
//! default is Qwen3 4B Instruct 2507 (Apache-2.0, 2.5 GB at Q4_K_M), which speaks Russian and
//! English well enough for a small child and answers in a second or two on an Apple GPU.

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{LlamaChatMessage, LlamaChatTemplate, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use parking_lot::Mutex;
use un_core::strategy::ChatPrompt;
use un_engine::{CancelToken, ChatModel};

fn backend() -> Result<&'static LlamaBackend, String> {
    static BACKEND: OnceLock<Result<LlamaBackend, String>> = OnceLock::new();
    BACKEND
        .get_or_init(|| {
            // The model stays loaded for the life of the process (sessions share it), so its Metal
            // buffers are still registered when the process exits, and ggml's Metal backend
            // aborts on an assertion then. Without residency sets there is nothing to unregister.
            if std::env::var_os("GGML_METAL_NO_RESIDENCY").is_none() {
                std::env::set_var("GGML_METAL_NO_RESIDENCY", "1");
            }
            let mut backend = LlamaBackend::init().map_err(|e| e.to_string())?;
            backend.void_logs();
            Ok(backend)
        })
        .as_ref()
        .map_err(Clone::clone)
}

pub struct LlamaChat {
    /// Loaded for the life of the process (see [`LlamaChat::shared`]), so a context can keep it.
    model: &'static LlamaModel,
    template: LlamaChatTemplate,
    name: String,
    /// The context of the last answer and the tokens in its cache: the master prompt and the
    /// conversation so far are the same from one request to the next, so only the new tail is
    /// read again. One answer at a time.
    session: Mutex<Option<Session>>,
}

struct Session {
    context: LlamaContext<'static>,
    tokens: Vec<LlamaToken>,
}

const CANCELLED: &str = "cancelled";

/// Room for the master prompt, a few turns and the answer.
const N_CTX: usize = 2048;
const N_BATCH: usize = 512;

impl LlamaChat {
    /// Loads a GGUF model, all layers on the GPU where there is one. The weights stay in memory
    /// until the process ends.
    pub fn load(path: &Path) -> Result<Self, String> {
        let backend = backend()?;
        let params = LlamaModelParams::default().with_n_gpu_layers(999);
        let model = LlamaModel::load_from_file(backend, path, &params).map_err(|e| format!("{}: {e}", path.display()))?;
        let template = model.chat_template(None).map_err(|e| format!("{}: no chat template ({e})", path.display()))?;
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(Self { model: Box::leak(Box::new(model)), template, name, session: Mutex::new(None) })
    }

    /// The model at `path`, loaded on first use and shared afterwards: sessions come and go,
    /// gigabytes of weights should not.
    pub fn shared(path: &Path) -> Result<Arc<Self>, String> {
        static MODELS: OnceLock<Mutex<HashMap<PathBuf, Arc<LlamaChat>>>> = OnceLock::new();
        let models = MODELS.get_or_init(|| Mutex::new(HashMap::new()));
        let mut models = models.lock();
        if let Some(model) = models.get(path) {
            return Ok(Arc::clone(model));
        }
        let model = Arc::new(Self::load(path)?);
        models.insert(path.to_path_buf(), Arc::clone(&model));
        Ok(model)
    }

    fn prompt_tokens(&self, prompt: &ChatPrompt) -> Result<Vec<LlamaToken>, String> {
        let mut messages = vec![LlamaChatMessage::new("system".into(), prompt.system.clone()).map_err(|e| e.to_string())?];
        for turn in &prompt.turns {
            let role = if turn.child { "user" } else { "assistant" };
            messages.push(LlamaChatMessage::new(role.into(), turn.text.clone()).map_err(|e| e.to_string())?);
        }
        let text = self.model.apply_chat_template(&self.template, &messages, true).map_err(|e| e.to_string())?;
        // The template already carries the special tokens; parse them, add none.
        Ok(self.model.vocab().tokenize(text.as_bytes(), false, true))
    }

    fn generate(&self, prompt: &ChatPrompt, cancel: &CancelToken) -> Result<String, String> {
        let tokens = self.prompt_tokens(prompt)?;
        let max_tokens = prompt.max_tokens.max(1) as usize;
        if tokens.is_empty() {
            return Err("empty prompt".into());
        }
        if tokens.len() + max_tokens + 8 > N_CTX {
            return Err(format!("prompt of {} tokens is too long", tokens.len()));
        }
        let mut slot = self.session.lock();
        if slot.is_none() {
            let params = LlamaContextParams::default()
                .with_n_ctx(NonZeroU32::new(N_CTX as u32))
                .with_n_batch(N_BATCH as u32)
                .with_n_threads(4)
                .with_n_threads_batch(4);
            let context = self.model.new_context(backend()?, params).map_err(|e| e.to_string())?;
            *slot = Some(Session { context, tokens: Vec::new() });
        }
        let session = slot.as_mut().expect("a session");
        let result = answer(self.model, session, &tokens, max_tokens, cancel);
        // A cancelled answer leaves the cache consistent (every token in it was read); any other
        // failure may have left it half-written: start afresh next time.
        if result.as_ref().is_err_and(|e| e != CANCELLED) {
            *slot = None;
        }
        result
    }
}

/// Reads what is new in `tokens` into the session's cache, then samples the answer.
fn answer(
    model: &LlamaModel,
    session: &mut Session,
    tokens: &[LlamaToken],
    max_tokens: usize,
    cancel: &CancelToken,
) -> Result<String, String> {
    let vocab = model.vocab();
    // Keep the cached prefix the prompt shares; the last prompt token is always read again, for its logits.
    let shared = session.tokens.iter().zip(tokens).take_while(|(a, b)| a == b).count().min(tokens.len() - 1);
    let started = std::time::Instant::now();
    session.context.clear_kv_cache_seq(Some(0), Some(shared as u32), None).map_err(|e| e.to_string())?;
    session.tokens.truncate(shared);
    let mut batch = LlamaBatch::new(N_BATCH, 1);
    let last = tokens.len() - 1;
    for (chunk_index, chunk) in tokens[shared..].chunks(N_BATCH).enumerate() {
        batch.clear();
        for (i, token) in chunk.iter().enumerate() {
            let position = shared + chunk_index * N_BATCH + i;
            batch.add(*token, position as i32, &[0], position == last).map_err(|e| e.to_string())?;
        }
        session.context.decode(&mut batch).map_err(|e| e.to_string())?;
    }
    session.tokens.extend_from_slice(&tokens[shared..]);
    let read = started.elapsed().as_secs_f64();
    let mut sampler = LlamaSampler::chain_simple([
        LlamaSampler::penalties(vocab.n_tokens(), 64, 1.1, 0.0, 0.0),
        LlamaSampler::top_k(40),
        LlamaSampler::top_p(0.9, 1),
        LlamaSampler::temp(0.6),
        LlamaSampler::dist(seed()),
    ]);
    let mut out: Vec<u8> = Vec::new();
    let mut generated = 0;
    for _ in 0..max_tokens {
        if cancel.is_cancelled() {
            return Err(CANCELLED.into());
        }
        let token = sampler.sample(&session.context, batch.n_tokens() - 1);
        sampler.accept(token);
        if vocab.is_eog(token) {
            break;
        }
        out.extend(vocab.token_to_piece(token, false, None));
        if ends_a_sentence_pair(&out) {
            break;
        }
        batch.clear();
        batch.add(token, session.tokens.len() as i32, &[0], true).map_err(|e| e.to_string())?;
        session.context.decode(&mut batch).map_err(|e| e.to_string())?;
        session.tokens.push(token);
        generated += 1;
    }
    // Where the time goes, on the console (a phone has no logger).
    eprintln!(
        "un-llm: read {} new of {} prompt tokens in {read:.2} s, wrote {generated} in {:.2} s",
        tokens.len() - shared,
        tokens.len(),
        started.elapsed().as_secs_f64() - read
    );
    Ok(tidy(&String::from_utf8_lossy(&out)))
}

impl ChatModel for LlamaChat {
    fn reply(&self, prompt: &ChatPrompt, cancel: &CancelToken) -> Result<String, String> {
        self.generate(prompt, cancel)
    }

    fn name(&self) -> String {
        self.name.clone()
    }
}

/// A model that loads only when the first answer is wanted: nothing touches the GPU or the
/// memory for it while the lesson keeps to its script. Once loaded it stays (see
/// [`LlamaChat::shared`]); a model that failed to load is not tried again.
pub struct OnDemand {
    path: PathBuf,
    model: Mutex<Option<Result<Arc<LlamaChat>, String>>>,
    name: String,
}

impl OnDemand {
    pub fn new(path: &Path) -> Arc<Self> {
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        Arc::new(Self { path: path.to_path_buf(), model: Mutex::new(None), name })
    }

    fn model(&self) -> Result<Arc<LlamaChat>, String> {
        let mut slot = self.model.lock();
        if slot.is_none() {
            let started = std::time::Instant::now();
            let model = LlamaChat::shared(&self.path);
            // On stderr as well as the log: a phone has no logger, and a model that will not
            // load (memory, a bad file) must be visible in the console.
            match &model {
                Ok(_) => eprintln!("un-llm: {} loaded in {:.1} s", self.path.display(), started.elapsed().as_secs_f64()),
                Err(error) => {
                    log::warn!("chat model unavailable: {error}");
                    eprintln!("un-llm: chat model unavailable: {error}");
                }
            }
            *slot = Some(model);
        }
        slot.as_ref().expect("loaded").clone()
    }
}

impl ChatModel for OnDemand {
    fn reply(&self, prompt: &ChatPrompt, cancel: &CancelToken) -> Result<String, String> {
        let model = self.model()?;
        if cancel.is_cancelled() {
            return Err(CANCELLED.into());
        }
        model.reply(prompt, cancel)
    }

    fn name(&self) -> String {
        self.name.clone()
    }
}

fn seed() -> u32 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(1)
}

/// Two sentences are as much as a small child listens to: generation stops there.
fn ends_a_sentence_pair(out: &[u8]) -> bool {
    let text = String::from_utf8_lossy(out);
    sentence_ends(&text).len() >= 2
}

fn sentence_ends(text: &str) -> Vec<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut ends = Vec::new();
    for (k, (i, c)) in chars.iter().enumerate() {
        if matches!(c, '.' | '!' | '?') {
            let next = chars.get(k + 1).map(|(_, c)| *c);
            if next.is_none_or(|n| n.is_whitespace()) && next.is_some() || next.is_none() && k > 0 {
                ends.push(i + c.len_utf8());
            }
        }
    }
    ends
}

/// The answer as the tutor says it: no markup, no emoji, at most two sentences.
pub fn tidy(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .filter(|c| !matches!(c, '*' | '#' | '_' | '`' | '~' | '[' | ']' | '<' | '>' | '|'))
        // Emoji, pictographs, and the invisible selectors and joiners that come with them.
        .filter(|c| {
            let code = *c as u32;
            code < 0x1F000 && !(0x2600..0x27C0).contains(&code) && !(0xFE00..=0xFE0F).contains(&code) && code != 0x200D
        })
        .collect();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let ends = sentence_ends(&cleaned);
    let cut = if ends.len() >= 2 { ends[1] } else { cleaned.len() };
    cleaned[..cut].trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_cut_to_two_sentences_without_markup() {
        assert_eq!(tidy("**Привет!** Как дела? А у меня всё хорошо."), "Привет! Как дела?");
        assert_eq!(tidy("Yes, I see 3.5 toys"), "Yes, I see 3.5 toys");
        assert_eq!(tidy("  Great job!  😊 "), "Great job!");
        assert_eq!(tidy("Как в сказке! ️"), "Как в сказке!");
        assert_eq!(tidy("Let's count: one, two."), "Let's count: one, two.");
    }

    /// Set UN_LLM_GGUF to a model to hear it answer (`cargo test -p un-llm -- --nocapture`).
    #[test]
    fn a_model_answers_a_child() {
        let Ok(path) = std::env::var("UN_LLM_GGUF") else { return };
        let chat = LlamaChat::shared(Path::new(&path)).unwrap();
        let prompt = ChatPrompt {
            system: "Ты — Амбер, добрая учительница для детей 5 лет. Отвечай по-русски одним-двумя короткими предложениями."
                .into(),
            turns: vec![un_core::strategy::ChatTurn { child: true, text: "А ты любишь собак?".into() }],
            max_tokens: 60,
        };
        let started = std::time::Instant::now();
        let answer = chat.reply(&prompt, &CancelToken::default()).unwrap();
        eprintln!("{answer:?} in {:.1} s", started.elapsed().as_secs_f64());
        assert!(!answer.is_empty());
    }
}
