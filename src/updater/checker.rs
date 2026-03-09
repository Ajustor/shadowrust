use std::sync::{Arc, Mutex};

use super::status::UpdateStatus;
use super::version::check_for_update;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checker_initial_state_is_checking() {
        let checker = UpdateChecker {
            status: Arc::new(Mutex::new(UpdateStatus::Checking)),
        };
        assert_eq!(checker.get(), UpdateStatus::Checking);
    }

    #[test]
    fn test_checker_get_returns_up_to_date() {
        let checker = UpdateChecker {
            status: Arc::new(Mutex::new(UpdateStatus::UpToDate)),
        };
        assert_eq!(checker.get(), UpdateStatus::UpToDate);
    }

    #[test]
    fn test_checker_get_returns_available() {
        let checker = UpdateChecker {
            status: Arc::new(Mutex::new(UpdateStatus::Available {
                version: "1.0.0".to_string(),
                url: "https://example.com".to_string(),
            })),
        };
        match checker.get() {
            UpdateStatus::Available { version, url } => {
                assert_eq!(version, "1.0.0");
                assert_eq!(url, "https://example.com");
            }
            _ => panic!("expected Available"),
        }
    }

    #[test]
    fn test_checker_get_returns_failed() {
        let checker = UpdateChecker {
            status: Arc::new(Mutex::new(UpdateStatus::Failed)),
        };
        assert_eq!(checker.get(), UpdateStatus::Failed);
    }
}
