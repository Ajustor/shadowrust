use std::process::Command;

/// Inhibit sleep via D-Bus ScreenSaver interface.
/// Returns a cookie that can be used to release the inhibition.
pub fn dbus_inhibit() -> Option<u32> {
    let out = Command::new("dbus-send")
        .args([
            "--session",
            "--print-reply",
            "--dest=org.freedesktop.ScreenSaver",
            "/org/freedesktop/ScreenSaver",
            "org.freedesktop.ScreenSaver.Inhibit",
            "string:ShadowRust",
            "string:Capture in progress",
        ])
        .output()
        .ok()?;

    if out.status.success() {
        // Parse the uint32 cookie from the reply
        let reply = String::from_utf8_lossy(&out.stdout);
        let cookie = reply
            .split_whitespace()
            .last()
            .and_then(|s| s.parse::<u32>().ok());
        if let Some(c) = cookie {
            log::info!("Sleep inhibited via D-Bus ScreenSaver (cookie {c})");
            return Some(c);
        }
    }
    log::warn!("D-Bus ScreenSaver inhibit unavailable; sleep not prevented");
    None
}

/// Release a previously acquired sleep inhibition using the given cookie.
pub fn dbus_uninhibit(cookie: u32) {
    let _ = Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.ScreenSaver",
            "/org/freedesktop/ScreenSaver",
            "org.freedesktop.ScreenSaver.UnInhibit",
            &format!("uint32:{cookie}"),
        ])
        .status();
}
