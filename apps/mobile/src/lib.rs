//! The library the iOS app links: re-exports the Un FFI so its symbols end up in this
//! `staticlib`, and makes the unmath lessons the app's lesson list, in unmath's order.
//!
//! ```text
//! cargo build --manifest-path apps/mobile/Cargo.toml --release --target aarch64-apple-ios
//! ```
//!
//! (`tools/ios.sh` does this, generates the Swift bindings and the Xcode project.) The app calls
//! [`un_mobile_init`] once at startup.

use std::sync::Arc;

use un_core::strategy::{SessionConfig, Strategy, StrategyFactory, StrategyInfo, StrategyRegistry};

pub use un_ffi;

/// Makes the unmath lessons the app's lesson list. Call once before any other function of the
/// library; calling again is harmless.
#[no_mangle]
pub extern "C" fn un_mobile_init() {
    un_ffi::set_registry(registry);
}

/// The unmath lessons and nothing else (no engine demo), with the session settings they need.
pub fn registry() -> StrategyRegistry {
    let mut lessons = StrategyRegistry::new();
    unmath::register(&mut lessons);
    let mut registry = StrategyRegistry::new();
    for info in lessons.infos() {
        if let Some(inner) = lessons.get(&info.id) {
            registry.register(Arc::new(Tuned { inner }));
        }
    }
    registry
}

/// The settings unmath lessons need over the app's defaults: practice loops take many attempts
/// (a lesson pauses at `max_tasks`, unmath README), and the family has five toys.
fn tune(config: &mut SessionConfig) {
    config.max_tasks = config.max_tasks.max(40);
    config.total_objects = 5;
}

/// A lesson with its session settings applied when a session is created.
struct Tuned {
    inner: Arc<dyn StrategyFactory>,
}

impl StrategyFactory for Tuned {
    fn info(&self) -> StrategyInfo {
        self.inner.info()
    }

    fn create(&self, config: &SessionConfig) -> Box<dyn Strategy> {
        let mut config = config.clone();
        tune(&mut config);
        self.inner.create(&config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_lists_the_unmath_lessons_and_nothing_else() {
        un_mobile_init();
        un_mobile_init();
        let ids: Vec<String> = un_ffi::available_strategies().into_iter().map(|s| s.id).collect();
        let mut unmath_only = StrategyRegistry::new();
        unmath::register(&mut unmath_only);
        let expected: Vec<String> = unmath_only.infos().into_iter().map(|i| i.id).collect();
        assert_eq!(ids, expected);
        assert!(!ids.is_empty() && ids.iter().all(|id| id.starts_with("unmath-")), "{ids:?}");
    }

    #[test]
    fn lessons_get_the_settings_they_need() {
        let mut config = SessionConfig { max_tasks: 10, total_objects: 10, ..Default::default() };
        tune(&mut config);
        assert_eq!((config.max_tasks, config.total_objects), (40, 5));
    }
}
