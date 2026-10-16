//! The on-device language model behind [`Effect::Chat`](un_core::strategy::Effect::Chat): the
//! tutor's answers to whatever the child says outside the lesson's script. The engine runs a
//! request on its own thread, so a slow answer never holds up speech, listening or timers.

use un_core::strategy::ChatPrompt;

use crate::speech::CancelToken;

/// A local language model. `reply` blocks until the answer is complete (or `cancel` fires,
/// when a newer request replaced this one); the engine calls it from a dedicated thread.
pub trait ChatModel: Send + Sync {
    fn reply(&self, prompt: &ChatPrompt, cancel: &CancelToken) -> Result<String, String>;

    /// The model's name, for the log.
    fn name(&self) -> String {
        String::new()
    }
}
