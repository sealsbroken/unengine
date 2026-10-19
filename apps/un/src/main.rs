//! Un on the desktop: the window with the unmath lessons (the iOS app's list).
// A window, no console behind it in release builds (Windows).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> eframe::Result<()> {
    un_app::bundle::use_bundled_libraries();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    un_app::set_registry(un_mobile::registry);
    let first = un_app::registry().infos().first().map(|i| i.id.clone());
    let defaults = un_desktop::Defaults { detector: Some("toys".to_string()), strategy: first };
    let result = un_desktop::run_with(un_app::registry(), defaults);
    // The system's OpenGL gave the window nothing to draw with: try the bundle's own (Linux).
    if let Err(error @ (eframe::Error::NoGlutinConfigs(..) | eframe::Error::Glutin(_) | eframe::Error::OpenGL(_))) = &result {
        eprintln!("OpenGL failed ({error}); trying the bundled one");
        un_app::bundle::restart_with_bundled_opengl();
    }
    result
}
