use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SopStep {
    pub title: String,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SopPlan {
    pub id: String,
    pub steps: Vec<SopStep>,
}

pub fn create_plan(id: &str, steps: &[&str]) -> anyhow::Result<SopPlan> {
    if id.trim().is_empty() {
        bail!("sop id cannot be empty");
    }
    if steps.is_empty() {
        bail!("sop requires at least one step");
    }

    let mapped = steps
        .iter()
        .map(|title| SopStep {
            title: (*title).to_string(),
            completed: false,
        })
        .collect::<Vec<_>>();

    Ok(SopPlan {
        id: id.to_string(),
        steps: mapped,
    })
}

pub fn advance_step(plan: &mut SopPlan, step_index: usize) -> anyhow::Result<()> {
    let step = plan
        .steps
        .get_mut(step_index)
        .with_context(|| format!("step index {step_index} is out of range"))?;

    if step.completed {
        bail!("step `{}` is already completed", step.title);
    }

    step.completed = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{advance_step, create_plan};

    #[test]
    fn advance_step_success_path() {
        let mut plan = create_plan("deploy", &["build", "ship"]).expect("plan should create");
        advance_step(&mut plan, 0).expect("advance should succeed");
        assert!(plan.steps[0].completed);
        assert!(!plan.steps[1].completed);
    }

    #[test]
    fn advance_step_rejects_out_of_range_negative_path() {
        let mut plan = create_plan("deploy", &["build"]).expect("plan should create");
        let err = advance_step(&mut plan, 3).expect_err("out of range should fail");
        assert!(err.to_string().contains("out of range"));
    }
}
