use anyhow::{anyhow, Context};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::Value;

use crate::config::AutopilotConfig;
use crate::types::{AutopilotEvent, Mission, MissionStatus, Proposal, ProposalStatus};

/// Thin wrapper over the Supabase PostgREST API using the service-role key.
#[derive(Debug, Clone)]
pub struct SupabaseClient {
    base_url: String,
    client: reqwest::Client,
}

impl SupabaseClient {
    pub fn new(config: &AutopilotConfig) -> anyhow::Result<Self> {
        if config.supabase_url.is_empty() {
            return Err(anyhow!("autopilot.supabase_url must not be empty"));
        }
        if config.supabase_service_role_key.is_empty() {
            return Err(anyhow!(
                "autopilot.supabase_service_role_key must not be empty"
            ));
        }

        let mut headers = HeaderMap::new();
        headers.insert(
            "apikey",
            HeaderValue::from_str(&config.supabase_service_role_key)
                .context("invalid supabase key")?,
        );
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", config.supabase_service_role_key))
                .context("invalid supabase key for auth header")?,
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        // Prefer return=representation to get back inserted rows.
        headers.insert("Prefer", HeaderValue::from_static("return=representation"));

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .context("failed to build HTTP client")?;

        Ok(Self {
            base_url: config.supabase_url.trim_end_matches('/').to_string(),
            client,
        })
    }

    fn rest_url(&self, table: &str) -> String {
        format!("{}/rest/v1/{}", self.base_url, table)
    }

    // ── Proposals ──

    pub async fn insert_proposal(&self, proposal: &Proposal) -> anyhow::Result<()> {
        let resp = self
            .client
            .post(self.rest_url("proposals"))
            .json(proposal)
            .send()
            .await
            .context("failed to insert proposal")?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("insert proposal failed: {body}"));
        }
        Ok(())
    }

    pub async fn update_proposal_status(
        &self,
        proposal_id: &str,
        status: ProposalStatus,
    ) -> anyhow::Result<()> {
        let resp = self
            .client
            .patch(self.rest_url("proposals"))
            .query(&[("id", format!("eq.{proposal_id}"))])
            .json(&serde_json::json!({
                "status": status,
                "updated_at": chrono::Utc::now(),
            }))
            .send()
            .await
            .context("failed to update proposal status")?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("update proposal status failed: {body}"));
        }
        Ok(())
    }

    // ── Missions ──

    pub async fn insert_mission(&self, mission: &Mission) -> anyhow::Result<()> {
        let resp = self
            .client
            .post(self.rest_url("missions"))
            .json(mission)
            .send()
            .await
            .context("failed to insert mission")?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("insert mission failed: {body}"));
        }
        Ok(())
    }

    pub async fn update_mission_status(
        &self,
        mission_id: &str,
        status: MissionStatus,
    ) -> anyhow::Result<()> {
        let resp = self
            .client
            .patch(self.rest_url("missions"))
            .query(&[("id", format!("eq.{mission_id}"))])
            .json(&serde_json::json!({
                "status": status,
                "updated_at": chrono::Utc::now(),
            }))
            .send()
            .await
            .context("failed to update mission status")?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("update mission status failed: {body}"));
        }
        Ok(())
    }

    pub async fn heartbeat_mission(&self, mission_id: &str) -> anyhow::Result<()> {
        let resp = self
            .client
            .patch(self.rest_url("missions"))
            .query(&[("id", format!("eq.{mission_id}"))])
            .json(&serde_json::json!({
                "heartbeat_at": chrono::Utc::now(),
            }))
            .send()
            .await
            .context("failed to heartbeat mission")?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("heartbeat mission failed: {body}"));
        }
        Ok(())
    }

    pub async fn query_stale_missions(
        &self,
        threshold_minutes: u32,
    ) -> anyhow::Result<Vec<Mission>> {
        let threshold = chrono::Utc::now() - chrono::Duration::minutes(threshold_minutes as i64);
        let resp = self
            .client
            .get(self.rest_url("missions"))
            .query(&[
                ("status", "eq.in_progress".to_string()),
                ("heartbeat_at", format!("lt.{}", threshold.to_rfc3339())),
            ])
            .send()
            .await
            .context("failed to query stale missions")?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("query stale missions failed: {body}"));
        }
        let missions: Vec<Mission> = resp.json().await.context("failed to parse missions")?;
        Ok(missions)
    }

    // ── Aggregations ──

    pub async fn get_daily_spend(&self) -> anyhow::Result<u64> {
        let today_start = chrono::Utc::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .expect("valid midnight timestamp");
        let today_start_utc =
            chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(today_start, chrono::Utc);
        let resp = self
            .client
            .get(self.rest_url("cap_gate_ledger"))
            .query(&[(
                "recorded_at",
                format!("gte.{}", today_start_utc.to_rfc3339()),
            )])
            .header("Prefer", "return=representation")
            .send()
            .await
            .context("failed to query daily spend")?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("query daily spend failed: {body}"));
        }
        let rows: Vec<Value> = resp.json().await.context("failed to parse ledger")?;
        let total: u64 = rows
            .iter()
            .filter_map(|r| r.get("cost_microdollars")?.as_u64())
            .sum();
        Ok(total)
    }

    pub async fn get_concurrent_mission_count(&self) -> anyhow::Result<usize> {
        let resp = self
            .client
            .get(self.rest_url("missions"))
            .query(&[("status", "eq.in_progress")])
            .header("Prefer", "count=exact")
            .header("Range-Unit", "items")
            .header("Range", "0-0")
            .send()
            .await
            .context("failed to count concurrent missions")?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("count concurrent missions failed: {body}"));
        }
        // Supabase returns content-range header: "0-0/N" where N is total count.
        let count = resp
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split('/').next_back())
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(0);
        Ok(count)
    }

    // ── Events ──

    pub async fn insert_event(&self, event: &AutopilotEvent) -> anyhow::Result<()> {
        let resp = self
            .client
            .post(self.rest_url("events"))
            .json(event)
            .send()
            .await
            .context("failed to insert event")?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("insert event failed: {body}"));
        }
        Ok(())
    }

    // ── Content ──

    pub async fn upsert_content(&self, content: &Value) -> anyhow::Result<()> {
        let resp = self
            .client
            .post(self.rest_url("content"))
            .header(
                "Prefer",
                "resolution=merge-duplicates,return=representation",
            )
            .json(content)
            .send()
            .await
            .context("failed to upsert content")?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("upsert content failed: {body}"));
        }
        Ok(())
    }

    // ── Generic queries ──

    pub async fn list_proposals(
        &self,
        status_filter: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> anyhow::Result<Vec<Proposal>> {
        let mut query = vec![
            ("order".to_string(), "created_at.desc".to_string()),
            ("limit".to_string(), limit.to_string()),
            ("offset".to_string(), offset.to_string()),
        ];
        if let Some(status) = status_filter {
            query.push(("status".to_string(), format!("eq.{status}")));
        }
        let resp = self
            .client
            .get(self.rest_url("proposals"))
            .query(&query)
            .send()
            .await
            .context("failed to list proposals")?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("list proposals failed: {body}"));
        }
        let proposals: Vec<Proposal> = resp.json().await.context("failed to parse proposals")?;
        Ok(proposals)
    }

    pub async fn list_missions(
        &self,
        status_filter: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> anyhow::Result<Vec<Mission>> {
        let mut query = vec![
            ("order".to_string(), "created_at.desc".to_string()),
            ("limit".to_string(), limit.to_string()),
            ("offset".to_string(), offset.to_string()),
        ];
        if let Some(status) = status_filter {
            query.push(("status".to_string(), format!("eq.{status}")));
        }
        let resp = self
            .client
            .get(self.rest_url("missions"))
            .query(&query)
            .send()
            .await
            .context("failed to list missions")?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("list missions failed: {body}"));
        }
        let missions: Vec<Mission> = resp.json().await.context("failed to parse missions")?;
        Ok(missions)
    }

    pub async fn get_mission(&self, mission_id: &str) -> anyhow::Result<Option<Mission>> {
        let resp = self
            .client
            .get(self.rest_url("missions"))
            .query(&[("id", format!("eq.{mission_id}"))])
            .send()
            .await
            .context("failed to get mission")?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("get mission failed: {body}"));
        }
        let missions: Vec<Mission> = resp.json().await.context("failed to parse mission")?;
        Ok(missions.into_iter().next())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_requires_url_and_key() {
        let config = AutopilotConfig::default();
        assert!(SupabaseClient::new(&config).is_err());

        let config = AutopilotConfig {
            supabase_url: "https://test.supabase.co".to_string(),
            supabase_service_role_key: "key123".to_string(),
            ..Default::default()
        };
        assert!(SupabaseClient::new(&config).is_ok());
    }

    #[test]
    fn rest_url_construction() {
        let config = AutopilotConfig {
            supabase_url: "https://test.supabase.co/".to_string(),
            supabase_service_role_key: "key123".to_string(),
            ..Default::default()
        };
        let client = SupabaseClient::new(&config).expect("client");
        assert_eq!(
            client.rest_url("proposals"),
            "https://test.supabase.co/rest/v1/proposals"
        );
    }
}
