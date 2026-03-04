use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Goal {
    pub id: String,
    pub title: String,
    pub completed: bool,
}

impl Goal {
    pub fn complete(&mut self) {
        self.completed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::Goal;

    #[test]
    fn complete_sets_goal_success_path() {
        let mut goal = Goal {
            id: "g1".to_string(),
            title: "Ship feature".to_string(),
            completed: false,
        };
        goal.complete();
        assert!(goal.completed);
    }

    #[test]
    fn goal_starts_incomplete_negative_path() {
        let goal = Goal {
            id: "g2".to_string(),
            title: "Test".to_string(),
            completed: false,
        };
        assert!(!goal.completed);
    }
}
