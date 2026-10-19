//! Which display server the window uses on Linux.
//!
//! winit takes Wayland whenever `WAYLAND_DISPLAY` is set and never falls back: on a desktop
//! without the Wayland client library (`libwayland-client.so.0`, package libwayland-client0)
//! the window then fails with `NoWaylandLib`, though X11 (XWayland) would work. So when Wayland
//! is announced but its library does not load, and an X11 display is there, the window opens on
//! X11. `UN_DISPLAY=x11` or `UN_DISPLAY=wayland` chooses by hand.

/// The event loop's builder hook: X11 when Wayland cannot work (or is not wanted); otherwise
/// winit's own choice.
#[cfg(target_os = "linux")]
pub fn event_loop_hook() -> Option<eframe::EventLoopBuilderHook> {
    use winit::platform::wayland::EventLoopBuilderExtWayland;
    use winit::platform::x11::EventLoopBuilderExtX11;

    let set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
    let wayland = set("WAYLAND_DISPLAY") || set("WAYLAND_SOCKET");
    let x11 = set("DISPLAY");
    match std::env::var("UN_DISPLAY").as_deref() {
        Ok("x11") => return Some(Box::new(|builder| { builder.with_x11(); })),
        Ok("wayland") => return Some(Box::new(|builder| { builder.with_wayland(); })),
        _ => {}
    }
    if !wayland || loads("libwayland-client.so.0") {
        return None;
    }
    if x11 {
        log::warn!("the Wayland client library (libwayland-client.so.0) is missing: the window opens on X11");
        return Some(Box::new(|builder| { builder.with_x11(); }));
    }
    eprintln!(
        "Un needs the Wayland client library for its window: install libwayland-client0 \
         (Debian, Ubuntu) or wayland (Fedora, Arch), or run it in an X11 session."
    );
    None
}

#[cfg(not(target_os = "linux"))]
pub fn event_loop_hook() -> Option<eframe::EventLoopBuilderHook> {
    None
}

/// Whether the shared library `name` can be loaded (as winit will try to).
#[cfg(target_os = "linux")]
fn loads(name: &str) -> bool {
    let Ok(name) = std::ffi::CString::new(name) else { return false };
    // SAFETY: dlopen with a valid C string; the handle is closed at once.
    unsafe {
        let handle = libc::dlopen(name.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL);
        if handle.is_null() {
            return false;
        }
        libc::dlclose(handle);
    }
    true
}
