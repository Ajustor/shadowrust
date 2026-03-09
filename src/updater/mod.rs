/// Background update checker.
///
/// Spawns a thread that hits the GitHub Releases API once at startup and
/// compares the latest tag with the compiled-in version. The result is stored
/// in a shared mutex so the UI can poll it without blocking.
use std::sync::{Arc, Mutex};

const GITHUB_RELEASES_URL: &str = "https://api.github.com/repos/Ajustor/shadowrust/releases/latest";

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateStatus {
    /// Check hasn't finished yet.
    Checking,
    /// We are on the latest version (or the check returned nothing useful).
    UpToDate,
    /// A newer version is available.
    Available { version: String, url: String },
    /// Network error or JSON parse error — silently ignored.
    Failed,
}

#[derive(Clone)]
pub struct UpdateChecker {
    pub status: Arc<Mutex<UpdateStatus>>,
}

impl UpdateChecker {
    /// Spawn the background check thread and return a handle.
    pub fn start() -> Self {
        let status = Arc::new(Mutex::new(UpdateStatus::Checking));
        let status_clone = Arc::clone(&status);

        std::thread::Builder::new()
            .name("update-check".into())
            .spawn(move || {
                let result = check_for_update();
                if let Ok(mut s) = status_clone.lock() {
                    *s = result;
                }
            })
            .ok();

        Self { status }
    }

    pub fn get(&self) -> UpdateStatus {
        self.status
            .lock()
            .map(|g| g.clone())
            .unwrap_or(UpdateStatus::Failed)
    }
}

fn check_for_update() -> UpdateStatus {
    let current = env!("CARGO_PKG_VERSION");

    let response = ureq::get(GITHUB_RELEASES_URL)
        .set("User-Agent", &format!("shadowrust/{current}"))
        .set("Accept", "application/vnd.github+json")
        .call();

    let body = match response {
        Ok(r) => match r.into_string() {
            Ok(s) => s,
            Err(_) => return UpdateStatus::Failed,
        },
        Err(_) => return UpdateStatus::Failed,
    };

    let json: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return UpdateStatus::Failed,
    };

    let tag = match json["tag_name"].as_str() {
        Some(t) => t.trim_start_matches('v').to_string(),
        None => return UpdateStatus::UpToDate,
    };

    let html_url = json["html_url"]
        .as_str()
        .unwrap_or("https://github.com/Ajustor/shadowrust/releases/latest")
        .to_string();

    if is_newer(&tag, current) {
        UpdateStatus::Available {
            version: tag,
            url: html_url,
        }
    } else {
        UpdateStatus::UpToDate
    }
}

/// Returns true if `remote` is strictly newer than `current`.
/// Compares as semver triplets; any parse failure → false.
fn is_newer(remote: &str, current: &str) -> bool {
    let parse = |s: &str| -> Option<(u64, u64, u64)> {
        let mut parts = s.splitn(3, '.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts
            .next()
            .and_then(|p| p.split('-').next())?
            .parse()
            .ok()?;
        Some((major, minor, patch))
    };
    match (parse(remote), parse(current)) {
        (Some(r), Some(c)) => r > c,
        _ => false,
    }
}
