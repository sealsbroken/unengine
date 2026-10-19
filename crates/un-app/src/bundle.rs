//! The Linux bundle's own libraries.
//!
//! The window loads its display libraries at run time, by name (Wayland, xkbcommon, X11 and its
//! extensions): on a Linux that keeps them outside the standard directories they are not found.
//! The release bundle carries them in `lib/` next to the executables, with the keyboard layouts
//! xkbcommon reads in `xkb/` (and the X11 locale data, `locale/`), and [`use_bundled_libraries`]
//! restarts the program once with that folder on the library path.
//!
//! OpenGL is the system's first (the graphics driver; NixOS's and Guix's driver directories are
//! added). Where it gives the window nothing to draw with, the bundle's own Mesa in `lib/gl/`
//! (hardware drivers for Intel, AMD and Nouveau, and software rendering for anything else) takes
//! over: [`restart_with_bundled_opengl`], or `UN_GL=bundled` from the start.

/// Restarts the program with the bundle's `lib/` on `LD_LIBRARY_PATH`, once, when there is one
/// (the release bundle); otherwise, and on other systems, does nothing. Call it first in `main`.
pub fn use_bundled_libraries() {
    #[cfg(target_os = "linux")]
    linux::restart(false);
}

/// The system's OpenGL failed: restarts the program with the bundle's Mesa, if there is one and
/// it is not in use already. Returns when it cannot.
pub fn restart_with_bundled_opengl() {
    #[cfg(target_os = "linux")]
    linux::restart(true);
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

    /// Set (to `bundled`) where the bundle's Mesa is the OpenGL.
    const GL: &str = "UN_GL";

    pub fn restart(bundled_gl: bool) {
        let gl_in_use = std::env::var(GL).as_deref() == Ok("bundled") && std::env::var_os(RESTARTED).is_some();
        if (std::env::var_os(RESTARTED).is_some() && !bundled_gl) || (bundled_gl && gl_in_use) {
            return;
        }
        let Ok(exe) = std::env::current_exe() else { return };
        let Some(dir) = exe.parent().map(Path::to_path_buf) else { return };
        let lib = dir.join("lib");
        let gl = lib.join("gl");
        if !lib.is_dir() || (bundled_gl && !gl.is_dir()) {
            return;
        }
        let use_gl = (bundled_gl || std::env::var(GL).as_deref() == Ok("bundled")) && gl.is_dir();
        let mut paths: Vec<PathBuf> = Vec::new();
        if use_gl {
            paths.push(gl.clone());
        }
        paths.push(lib);
        // The system's OpenGL driver, unless the bundle's is the one (no mixing the two).
        if !use_gl {
            paths.extend(DRIVER_DIRS.iter().map(PathBuf::from).filter(|d| d.is_dir()));
            if let Some(home) = std::env::var_os("HOME") {
                let guix = Path::new(&home).join(".guix-profile/lib");
                if guix.is_dir() {
                    paths.push(guix);
                }
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
        // The keyboard layouts and the X11 locale data (dead keys), where the system has none at
        // xkbcommon's built-in places.
        if std::env::var_os("XKB_CONFIG_ROOT").is_none() && !Path::new("/usr/share/X11/xkb").is_dir() && dir.join("xkb").is_dir() {
            command.env("XKB_CONFIG_ROOT", dir.join("xkb"));
        }
        if std::env::var_os("XLOCALEDIR").is_none() && !Path::new("/usr/share/X11/locale").is_dir() && dir.join("locale").is_dir() {
            command.env("XLOCALEDIR", dir.join("locale"));
        }
        if use_gl {
            // glvnd's EGL and GLX dispatch to Mesa; Mesa's drivers from the bundle.
            command
                .env(GL, "bundled")
                .env("__EGL_VENDOR_LIBRARY_FILENAMES", gl.join("50_mesa.json"))
                .env("__GLX_VENDOR_LIBRARY_NAME", "mesa")
                .env("LIBGL_DRIVERS_PATH", gl.join("dri"))
                .env("GBM_BACKENDS_PATH", gl.join("gbm"));
            eprintln!("using the bundled OpenGL (Mesa)");
        }
        // Only returns if the restart failed; then go on as we are.
        let error = command.exec();
        eprintln!("could not restart with the bundled libraries ({}): {error}", dir.join("lib").display());
    }
}
