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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_equality() {
        assert_eq!(UpdateStatus::Checking, UpdateStatus::Checking);
        assert_eq!(UpdateStatus::UpToDate, UpdateStatus::UpToDate);
        assert_eq!(UpdateStatus::Failed, UpdateStatus::Failed);
        assert_ne!(UpdateStatus::Checking, UpdateStatus::UpToDate);
    }

    #[test]
    fn test_status_available_equality() {
        let a = UpdateStatus::Available {
            version: "1.0.0".to_string(),
            url: "https://example.com".to_string(),
        };
        let b = UpdateStatus::Available {
            version: "1.0.0".to_string(),
            url: "https://example.com".to_string(),
        };
        assert_eq!(a, b);
    }

    #[test]
    fn test_status_available_inequality_version() {
        let a = UpdateStatus::Available {
            version: "1.0.0".to_string(),
            url: "https://example.com".to_string(),
        };
        let b = UpdateStatus::Available {
            version: "2.0.0".to_string(),
            url: "https://example.com".to_string(),
        };
        assert_ne!(a, b);
    }

    #[test]
    fn test_status_clone() {
        let status = UpdateStatus::Available {
            version: "1.2.3".to_string(),
            url: "https://github.com/releases".to_string(),
        };
        let cloned = status.clone();
        assert_eq!(status, cloned);
    }

    #[test]
    fn test_status_debug() {
        let status = UpdateStatus::Checking;
        let debug_str = format!("{:?}", status);
        assert!(debug_str.contains("Checking"));
    }
}
