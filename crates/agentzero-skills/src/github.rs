//! GitHub API client for skill registry operations.
//!
//! Handles fetching releases (for install) and creating releases (for publish).
//! All operations are async and use the GitHub REST API v3.

use serde::{Deserialize, Serialize};

use crate::registry::RegistryError;
use crate::remote::ResolvedRelease;

const GITHUB_API: &str = "https://api.github.com";
const USER_AGENT: &str = "agentzero/0.1";

/// GitHub API client with optional authentication.
pub struct GitHubClient {
    client: reqwest::Client,
    token: Option<String>,
}

/// A GitHub release as returned by the API.
#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    body: Option<String>,
    assets: Vec<GitHubAsset>,
}

/// A release asset (file attachment).
#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

/// Request body for creating a GitHub release.
#[derive(Serialize)]
struct CreateReleaseRequest<'a> {
    tag_name: &'a str,
    name: &'a str,
    body: &'a str,
    draft: bool,
    prerelease: bool,
}

impl GitHubClient {
    /// Create a new client. Token is optional for public repos (read)
    /// but required for creating releases (publish).
    pub fn new(token: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .expect("reqwest client should build");
        Self { client, token }
    }

    /// Resolve a token from the environment or the provided option.
    pub fn from_env() -> Self {
        let token = std::env::var("GITHUB_TOKEN").ok();
        Self::new(token)
    }

    /// Fetch the latest release for a repo and resolve it to a download URL.
    pub async fn get_latest_release(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<ResolvedRelease, RegistryError> {
        let url = format!("{GITHUB_API}/repos/{owner}/{repo}/releases/latest");
        let release = self.fetch_release(&url).await?;
        self.resolve_release(&release)
    }

    /// Fetch a specific release by tag.
    pub async fn get_release_by_tag(
        &self,
        owner: &str,
        repo: &str,
        tag: &str,
    ) -> Result<ResolvedRelease, RegistryError> {
        let url = format!("{GITHUB_API}/repos/{owner}/{repo}/releases/tags/{tag}");
        let release = self.fetch_release(&url).await?;
        self.resolve_release(&release)
    }

    /// Create a new release on a GitHub repo.
    pub async fn create_release(
        &self,
        owner: &str,
        repo: &str,
        tag: &str,
        name: &str,
        body: &str,
    ) -> Result<String, RegistryError> {
        let token = self.token.as_ref().ok_or_else(|| {
            RegistryError::IoError(
                "GitHub token required for publish (set GITHUB_TOKEN)".into(),
            )
        })?;

        let url = format!("{GITHUB_API}/repos/{owner}/{repo}/releases");
        let request = CreateReleaseRequest {
            tag_name: tag,
            name,
            body,
            draft: false,
            prerelease: false,
        };

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/vnd.github+json")
            .json(&request)
            .send()
            .await
            .map_err(|e| RegistryError::IoError(format!("failed to create release: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(RegistryError::IoError(format!(
                "GitHub API error {status}: {body}"
            )));
        }

        // Extract the upload URL for assets
        let release: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| RegistryError::ParseError(format!("failed to parse response: {e}")))?;

        let upload_url = release
            .get("upload_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RegistryError::ParseError("no upload_url in response".into()))?;

        // Strip the {?name,label} template suffix
        let upload_url = upload_url
            .split('{')
            .next()
            .unwrap_or(upload_url)
            .to_string();

        Ok(upload_url)
    }

    /// Upload a release asset (e.g., the tarball).
    pub async fn upload_asset(
        &self,
        upload_url: &str,
        filename: &str,
        data: &[u8],
    ) -> Result<(), RegistryError> {
        let token = self.token.as_ref().ok_or_else(|| {
            RegistryError::IoError("GitHub token required for upload".into())
        })?;

        let url = format!("{upload_url}?name={filename}");
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/gzip")
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| RegistryError::IoError(format!("failed to upload asset: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(RegistryError::IoError(format!(
                "asset upload failed {status}: {body}"
            )));
        }

        Ok(())
    }

    /// Download bytes from a URL.
    pub async fn download(&self, url: &str) -> Result<Vec<u8>, RegistryError> {
        let mut req = self.client.get(url);
        if let Some(ref token) = self.token {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
        req = req.header("Accept", "application/octet-stream");

        let resp = req
            .send()
            .await
            .map_err(|e| RegistryError::IoError(format!("download failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(RegistryError::IoError(format!(
                "download failed with status {status}"
            )));
        }

        resp.bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| RegistryError::IoError(format!("failed to read response: {e}")))
    }

    async fn fetch_release(&self, url: &str) -> Result<GitHubRelease, RegistryError> {
        let mut req = self.client.get(url);
        if let Some(ref token) = self.token {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
        req = req.header("Accept", "application/vnd.github+json");

        let resp = req
            .send()
            .await
            .map_err(|e| RegistryError::IoError(format!("GitHub API request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            if status == reqwest::StatusCode::NOT_FOUND {
                return Err(RegistryError::NotFound(format!(
                    "no release found at {url}"
                )));
            }
            let body = resp.text().await.unwrap_or_default();
            return Err(RegistryError::IoError(format!(
                "GitHub API error {status}: {body}"
            )));
        }

        resp.json::<GitHubRelease>()
            .await
            .map_err(|e| RegistryError::ParseError(format!("failed to parse release: {e}")))
    }

    fn resolve_release(&self, release: &GitHubRelease) -> Result<ResolvedRelease, RegistryError> {
        // Find a .tar.gz asset
        let tarball_asset = release
            .assets
            .iter()
            .find(|a| a.name.ends_with(".tar.gz"))
            .ok_or_else(|| {
                RegistryError::NotFound(format!(
                    "no .tar.gz asset in release {}",
                    release.tag_name
                ))
            })?;

        // Extract version from tag (strip leading 'v' if present)
        let version = release
            .tag_name
            .strip_prefix('v')
            .unwrap_or(&release.tag_name)
            .to_string();

        // Try to find checksum in release body
        let checksum = release
            .body
            .as_deref()
            .and_then(crate::remote::extract_checksum_from_body);

        Ok(ResolvedRelease {
            tag: release.tag_name.clone(),
            version,
            tarball_url: tarball_asset.browser_download_url.clone(),
            checksum,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_creates_without_token() {
        let client = GitHubClient::new(None);
        assert!(client.token.is_none());
    }

    #[test]
    fn client_creates_with_token() {
        let client = GitHubClient::new(Some("ghp_test123".into()));
        assert!(client.token.is_some());
    }

    #[test]
    fn resolve_release_finds_tarball() {
        let client = GitHubClient::new(None);
        let release = GitHubRelease {
            tag_name: "v1.0.0".into(),
            body: Some("sha256:a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2".into()),
            assets: vec![GitHubAsset {
                name: "my-skill-1.0.0.tar.gz".into(),
                browser_download_url: "https://github.com/downloads/my-skill-1.0.0.tar.gz".into(),
            }],
        };

        let resolved = client.resolve_release(&release).expect("should resolve");
        assert_eq!(resolved.version, "1.0.0");
        assert_eq!(resolved.tag, "v1.0.0");
        assert!(resolved.tarball_url.contains("tar.gz"));
        assert!(resolved.checksum.is_some());
    }

    #[test]
    fn resolve_release_no_tarball_fails() {
        let client = GitHubClient::new(None);
        let release = GitHubRelease {
            tag_name: "v1.0.0".into(),
            body: None,
            assets: vec![GitHubAsset {
                name: "readme.md".into(),
                browser_download_url: "https://example.com/readme.md".into(),
            }],
        };

        let result = client.resolve_release(&release);
        assert!(result.is_err());
    }
}
