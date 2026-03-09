use super::status::UpdateStatus;

const GITHUB_RELEASES_URL: &str = "https://api.github.com/repos/Ajustor/shadowrust/releases/latest";

pub(crate) fn check_for_update() -> UpdateStatus {
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
pub(crate) fn is_newer(remote: &str, current: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer_same_version() {
        assert!(!is_newer("1.0.0", "1.0.0"));
    }

    #[test]
    fn test_is_newer_higher_major() {
        assert!(is_newer("2.0.0", "1.0.0"));
    }

    #[test]
    fn test_is_newer_higher_minor() {
        assert!(is_newer("1.1.0", "1.0.0"));
    }

    #[test]
    fn test_is_newer_higher_patch() {
        assert!(is_newer("1.0.1", "1.0.0"));
    }

    #[test]
    fn test_is_newer_lower_version() {
        assert!(!is_newer("1.0.0", "2.0.0"));
    }

    #[test]
    fn test_is_newer_lower_minor() {
        assert!(!is_newer("1.0.0", "1.1.0"));
    }

    #[test]
    fn test_is_newer_lower_patch() {
        assert!(!is_newer("1.0.0", "1.0.1"));
    }

    #[test]
    fn test_is_newer_with_v_prefix() {
        // is_newer doesn't handle the v prefix itself — caller strips it
        assert!(!is_newer("v1.0.0", "1.0.0")); // "v1" won't parse as u64
    }

    #[test]
    fn test_is_newer_pre_release() {
        // "1.0.1-beta" → patch parsed as "1" (split on '-')
        assert!(is_newer("1.0.1-beta", "1.0.0"));
    }

    #[test]
    fn test_is_newer_invalid_remote() {
        assert!(!is_newer("invalid", "1.0.0"));
    }

    #[test]
    fn test_is_newer_invalid_current() {
        assert!(!is_newer("1.0.0", "invalid"));
    }

    #[test]
    fn test_is_newer_both_invalid() {
        assert!(!is_newer("abc", "xyz"));
    }

    #[test]
    fn test_is_newer_empty_strings() {
        assert!(!is_newer("", ""));
    }

    #[test]
    fn test_is_newer_complex_versions() {
        assert!(is_newer("10.20.30", "10.20.29"));
        assert!(!is_newer("10.20.30", "10.20.31"));
        assert!(is_newer("0.6.0", "0.5.0")); // realistic for this project
    }
}
