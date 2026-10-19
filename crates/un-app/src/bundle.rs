//! The Linux bundle's own libraries.
//!
//! The window loads its display libraries at run time, by name (Wayland, xkbcommon, X11 and its
//! extensions): on a Linux that keeps them outside the standard directories they are not found.
//! The release bundle carries them in `lib/` next to the executables, with the keyboard layouts
//! xkbcommon reads in `xkb/`, and [`use_bundled_libraries`] restarts the program once with that
//! folder on the library path. OpenGL stays the system's (it is the graphics driver): the
//! driver directories of NixOS and Guix are added too.

/// Restarts the program with the bundle's `lib/` on `LD_LIBRARY_PATH`, once, when there is one
/// (the release bundle); otherwise, and on other systems, does nothing. Call it first in `main`.
pub fn use_bundled_libraries() {
    #[cfg(target_os = "linux")]
    linux::restart();
}

#[cfg(target_os = "linux")]
mod linux {
    use std::os::unix::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// Set in the restarted program, so it does not restart again.
    const RESTARTED: &str = "UN_BUNDLED_LIBRARIES";

    /// Where NixOS and Guix keep the graphics driver's OpenGL.
    const DRIVER_DIRS: [&str; 2] = ["/run/opengl-driver/lib", "/run/current-system/profile/lib"];

    pub fn restart() {
        if std::env::var_os(RESTARTED).is_some() {
            return;
        }
        let Ok(exe) = std::env::current_exe() else { return };
        let Some(dir) = exe.parent().map(Path::to_path_buf) else { return };
        let lib = dir.join("lib");
        if !lib.is_dir() {
            return;
        }
        let mut paths: Vec<PathBuf> = vec![lib];
        paths.extend(DRIVER_DIRS.iter().map(PathBuf::from).filter(|d| d.is_dir()));
        if let Some(home) = std::env::var_os("HOME") {
            let guix = Path::new(&home).join(".guix-profile/lib");
            if guix.is_dir() {
                paths.push(guix);
            }
        }
        if let Some(existing) = std::env::var_os("LD_LIBRARY_PATH") {
            paths.extend(std::env::split_paths(&existing));
        }
        let Ok(joined) = std::env::join_paths(paths) else { return };

        let mut args = std::env::args_os();
        let mut command = Command::new(&exe);
        if let Some(name) = args.next() {
            command.arg0(name);
        }
        command.args(args).env("LD_LIBRARY_PATH", joined).env(RESTARTED, "1");
        // The keyboard layouts, where the system has none at xkbcommon's built-in place.
        if std::env::var_os("XKB_CONFIG_ROOT").is_none() && !Path::new("/usr/share/X11/xkb").is_dir() && dir.join("xkb").is_dir() {
            command.env("XKB_CONFIG_ROOT", dir.join("xkb"));
        }
        // Only returns if the restart failed; then go on as we are.
        let error = command.exec();
        eprintln!("could not restart with the bundled libraries ({}): {error}", dir.join("lib").display());
    }
}
