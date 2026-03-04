use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CoordinationStatus {
    pub active_workers: u32,
    pub queued_tasks: u32,
}

impl CoordinationStatus {
    pub fn is_idle(&self) -> bool {
        self.active_workers == 0 && self.queued_tasks == 0
    }
}

#[cfg(test)]
mod tests {
    use super::CoordinationStatus;

    #[test]
    fn is_idle_true_when_empty_success_path() {
        let status = CoordinationStatus {
            active_workers: 0,
            queued_tasks: 0,
        };
        assert!(status.is_idle());
    }

    #[test]
    fn is_idle_false_when_busy_negative_path() {
        let status = CoordinationStatus {
            active_workers: 1,
            queued_tasks: 0,
        };
        assert!(!status.is_idle());
    }
}
