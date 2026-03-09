pub fn spawn_caffeinate() -> Option<std::process::Child> {
    std::process::Command::new("caffeinate")
        .args(["-i", "-d"])
        .spawn()
        .map_err(|e| log::warn!("caffeinate spawn failed: {e}"))
        .ok()
}
