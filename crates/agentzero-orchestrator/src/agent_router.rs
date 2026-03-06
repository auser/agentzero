//! AI-based agent routing.
//!
//! Uses a fast LLM call (e.g. Haiku) to classify an inbound message and
//! pick the best agent to handle it.  Falls back to keyword matching when
//! the AI router is unavailable or fails.

use agentzero_core::Provider;

/// What an agent declares about itself — used for routing decisions.
#[derive(Debug, Clone)]
pub struct AgentDescriptor {
    pub id: String,
    pub name: String,
    pub description: String,
    pub keywords: Vec<String>,
    pub subscribes_to: Vec<String>,
    pub produces: Vec<String>,
    pub privacy_boundary: String,
}

pub struct AgentRouter {
    /// Fast provider for classification (e.g. Haiku).
    provider: Option<Box<dyn Provider>>,
    /// Fall back to keyword matching if AI fails.
    fallback_to_keywords: bool,
}

impl AgentRouter {
    pub fn new(provider: Option<Box<dyn Provider>>, fallback_to_keywords: bool) -> Self {
        Self {
            provider,
            fallback_to_keywords,
        }
    }

    /// Keyword-only router (no AI).
    pub fn keywords_only() -> Self {
        Self {
            provider: None,
            fallback_to_keywords: true,
        }
    }

    /// Use AI to pick the best agent for this message.
    /// Returns the agent id, or None if no agent matches.
    pub async fn route(
        &self,
        message: &str,
        agents: &[AgentDescriptor],
    ) -> anyhow::Result<Option<String>> {
        if agents.is_empty() {
            return Ok(None);
        }

        // Try AI routing first
        if let Some(ref provider) = self.provider {
            match self.route_with_ai(provider.as_ref(), message, agents).await {
                Ok(Some(id)) => return Ok(Some(id)),
                Ok(None) => {
                    tracing::debug!("AI router returned no match");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "AI router failed, falling back");
                }
            }
        }

        // Keyword fallback
        if self.fallback_to_keywords {
            return Ok(self.route_by_keywords(message, agents));
        }

        Ok(None)
    }

    async fn route_with_ai(
        &self,
        provider: &dyn Provider,
        message: &str,
        agents: &[AgentDescriptor],
    ) -> anyhow::Result<Option<String>> {
        let agent_list = agents
            .iter()
            .map(|a| format!("- {} (id: {}): {}", a.name, a.id, a.description))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "You are a message router. Given the user message and available agents, \
             respond with ONLY the agent id that should handle this message. \
             If no agent is a good fit, respond with \"none\".\n\n\
             Available agents:\n{agent_list}\n\n\
             User message: {message}\n\n\
             Agent id:"
        );

        let result = provider.complete(&prompt).await?;
        let agent_id = result.output_text.trim().to_string();

        if agent_id == "none" {
            return Ok(None);
        }

        // Validate the response is a known agent
        if agents.iter().any(|a| a.id == agent_id) {
            Ok(Some(agent_id))
        } else {
            tracing::debug!(
                returned = %agent_id,
                "AI router returned unknown agent id"
            );
            Ok(None)
        }
    }

    fn route_by_keywords(&self, message: &str, agents: &[AgentDescriptor]) -> Option<String> {
        let lower = message.to_lowercase();
        let mut best: Option<(&AgentDescriptor, usize)> = None;

        for agent in agents {
            let hits = agent
                .keywords
                .iter()
                .filter(|kw| lower.contains(&kw.to_lowercase()))
                .count();
            if hits > 0 && best.as_ref().map_or(true, |(_, prev)| hits > *prev) {
                best = Some((agent, hits));
            }
        }

        best.map(|(a, _)| a.id.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_agents() -> Vec<AgentDescriptor> {
        vec![
            AgentDescriptor {
                id: "image-gen".into(),
                name: "Image Generator".into(),
                description: "Creates images from text descriptions".into(),
                keywords: vec![
                    "draw".into(),
                    "image".into(),
                    "picture".into(),
                    "illustration".into(),
                ],
                subscribes_to: vec!["channel.*.message".into()],
                produces: vec!["task.image.complete".into()],
                privacy_boundary: "any".into(),
            },
            AgentDescriptor {
                id: "code-review".into(),
                name: "Code Reviewer".into(),
                description: "Reviews code for bugs and style issues".into(),
                keywords: vec!["review".into(), "code".into(), "PR".into()],
                subscribes_to: vec!["channel.*.message".into()],
                produces: vec!["task.review.complete".into()],
                privacy_boundary: "any".into(),
            },
        ]
    }

    #[test]
    fn keyword_routing_picks_best_match() {
        let router = AgentRouter::keywords_only();
        let agents = test_agents();
        let result = router.route_by_keywords("please draw me a picture of a cat", &agents);
        assert_eq!(result.as_deref(), Some("image-gen"));
    }

    #[test]
    fn keyword_routing_picks_code_review() {
        let router = AgentRouter::keywords_only();
        let agents = test_agents();
        let result = router.route_by_keywords("review this PR for code quality", &agents);
        assert_eq!(result.as_deref(), Some("code-review"));
    }

    #[test]
    fn keyword_routing_no_match() {
        let router = AgentRouter::keywords_only();
        let agents = test_agents();
        let result = router.route_by_keywords("what is the weather today", &agents);
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn route_with_no_agents_returns_none() {
        let router = AgentRouter::keywords_only();
        let result = router.route("hello", &[]).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn route_falls_back_to_keywords() {
        let router = AgentRouter::new(None, true);
        let agents = test_agents();
        let result = router.route("draw a horse", &agents).await.unwrap();
        assert_eq!(result.as_deref(), Some("image-gen"));
    }
}
