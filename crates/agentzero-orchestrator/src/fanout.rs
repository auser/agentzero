//! Fan-out executor for parallel sub-agent orchestration.
//!
//! Spawns multiple agents concurrently and collects their results according
//! to a configured [`MergeStrategy`]: wait for all, any, or a quorum.

use agentzero_core::MergeStrategy;
use std::time::Duration;
use tokio::task::JoinSet;

/// A single agent's result from a fan-out step.
#[derive(Debug, Clone)]
pub struct FanOutResult {
    pub agent_id: String,
    pub output: Result<String, String>,
}

/// Configuration for a single fan-out step in a pipeline.
#[derive(Debug, Clone)]
pub struct FanOutStep {
    pub agents: Vec<String>,
    pub merge: MergeStrategy,
    pub timeout: Duration,
}

/// Execute a fan-out step: run `task_fn` for each agent in parallel,
/// collecting results according to the merge strategy.
///
/// The `task_fn` receives an agent ID and returns a result string.
/// It is called once per agent in `step.agents`.
pub async fn execute_fanout<F, Fut>(step: &FanOutStep, task_fn: F) -> Vec<FanOutResult>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<String, String>> + Send + 'static,
{
    let total = step.agents.len();
    if total == 0 {
        return vec![];
    }

    let mut join_set = JoinSet::new();
    for agent_id in &step.agents {
        let id = agent_id.clone();
        let fut = task_fn(id.clone());
        join_set.spawn(async move {
            FanOutResult {
                agent_id: id,
                output: fut.await,
            }
        });
    }

    let mut results = Vec::with_capacity(total);
    let deadline = tokio::time::Instant::now() + step.timeout;

    match &step.merge {
        MergeStrategy::WaitAll => {
            while let Some(res) = tokio::time::timeout_at(deadline, join_set.join_next())
                .await
                .ok()
                .flatten()
            {
                if let Ok(fan_result) = res {
                    results.push(fan_result);
                }
            }
        }
        MergeStrategy::WaitAny => {
            if let Ok(Some(Ok(fan_result))) =
                tokio::time::timeout_at(deadline, join_set.join_next()).await
            {
                results.push(fan_result);
            }
            // Abort remaining tasks.
            join_set.abort_all();
        }
        MergeStrategy::WaitQuorum { min } => {
            let needed = (*min).min(total);
            while results.len() < needed {
                match tokio::time::timeout_at(deadline, join_set.join_next()).await {
                    Ok(Some(Ok(fan_result))) => results.push(fan_result),
                    _ => break,
                }
            }
            // Abort remaining tasks once quorum is met.
            join_set.abort_all();
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fanout_wait_all_collects_all_results() {
        let step = FanOutStep {
            agents: vec!["a".to_string(), "b".to_string(), "c".to_string()],
            merge: MergeStrategy::WaitAll,
            timeout: Duration::from_secs(5),
        };

        let results = execute_fanout(&step, |id| async move { Ok(format!("result-{id}")) }).await;

        assert_eq!(results.len(), 3);
        let mut ids: Vec<_> = results.iter().map(|r| r.agent_id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }

    #[tokio::test]
    async fn fanout_wait_any_returns_first() {
        let step = FanOutStep {
            agents: vec!["fast".to_string(), "slow".to_string()],
            merge: MergeStrategy::WaitAny,
            timeout: Duration::from_secs(5),
        };

        let results = execute_fanout(&step, |id| async move {
            if id == "slow" {
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
            Ok(format!("result-{id}"))
        })
        .await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].agent_id, "fast");
    }

    #[tokio::test]
    async fn fanout_wait_quorum_returns_min_results() {
        let step = FanOutStep {
            agents: vec!["a".to_string(), "b".to_string(), "slow".to_string()],
            merge: MergeStrategy::WaitQuorum { min: 2 },
            timeout: Duration::from_secs(5),
        };

        let results = execute_fanout(&step, |id| async move {
            if id == "slow" {
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
            Ok(format!("result-{id}"))
        })
        .await;

        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn fanout_empty_agents_returns_empty() {
        let step = FanOutStep {
            agents: vec![],
            merge: MergeStrategy::WaitAll,
            timeout: Duration::from_secs(1),
        };

        let results = execute_fanout(&step, |id| async move { Ok(format!("result-{id}")) }).await;

        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn fanout_timeout_returns_partial_results() {
        let step = FanOutStep {
            agents: vec!["fast".to_string(), "slow".to_string()],
            merge: MergeStrategy::WaitAll,
            timeout: Duration::from_millis(100),
        };

        let results = execute_fanout(&step, |id| async move {
            if id == "slow" {
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
            Ok(format!("result-{id}"))
        })
        .await;

        // Should get at least the fast one before timeout.
        assert!(!results.is_empty());
        assert!(results.iter().any(|r| r.agent_id == "fast"));
    }

    #[tokio::test]
    async fn fanout_handles_errors() {
        let step = FanOutStep {
            agents: vec!["good".to_string(), "bad".to_string()],
            merge: MergeStrategy::WaitAll,
            timeout: Duration::from_secs(5),
        };

        let results = execute_fanout(&step, |id| async move {
            if id == "bad" {
                Err("something went wrong".to_string())
            } else {
                Ok("success".to_string())
            }
        })
        .await;

        assert_eq!(results.len(), 2);
        let good = results.iter().find(|r| r.agent_id == "good").unwrap();
        assert!(good.output.is_ok());
        let bad = results.iter().find(|r| r.agent_id == "bad").unwrap();
        assert!(bad.output.is_err());
    }
}
