//! File-based skill bundle loader.
//!
//! Reads skill bundles from `.agentzero/skills/<name>/` directories. Each
//! skill directory must contain at minimum a `skill.toml` manifest. An
//! optional `prompt.md` file provides the system prompt fragment injected
//! when the skill is activated.
//!
//! ## Directory layout
//!
//! ```text
//! .agentzero/skills/code-review/
//! ├── skill.toml       # SkillBundle metadata (required)
//! ├── prompt.md        # System prompt fragment (optional)
//! └── tools/           # Optional tool definitions
//!     └── lint.json    # DynamicToolDef as JSON
//! ```

use agentzero_core::{
    SkillActivation, SkillBundle, SkillBundleMeta, SkillLoader, SkillToolDef, ToolContext,
};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Manages active skill state for a session.
struct ActiveSkill {
    name: String,
    prompt_fragment: String,
}

/// Loads skill bundles from a directory on disk.
pub struct FileSkillLoader {
    /// Root directory containing skill subdirectories (e.g. `.agentzero/skills/`).
    skills_dir: PathBuf,
    /// Currently active skills in this session.
    active: RwLock<Vec<ActiveSkill>>,
}

impl FileSkillLoader {
    /// Create a new loader rooted at the given skills directory.
    pub fn new(skills_dir: impl Into<PathBuf>) -> Self {
        Self {
            skills_dir: skills_dir.into(),
            active: RwLock::new(Vec::new()),
        }
    }

    /// Read and parse the `skill.toml` manifest from a skill directory.
    fn read_manifest(skill_dir: &Path) -> anyhow::Result<SkillBundle> {
        let manifest_path = skill_dir.join("skill.toml");
        let content = std::fs::read_to_string(&manifest_path)
            .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", manifest_path.display()))?;
        let mut bundle: SkillBundle = toml::from_str(&content)
            .map_err(|e| anyhow::anyhow!("failed to parse {}: {e}", manifest_path.display()))?;

        // Load prompt.md if present.
        let prompt_path = skill_dir.join("prompt.md");
        if prompt_path.exists() {
            match std::fs::read_to_string(&prompt_path) {
                Ok(prompt) => bundle.prompt_template = prompt,
                Err(e) => {
                    warn!(
                        skill = %bundle.name,
                        path = %prompt_path.display(),
                        "failed to read prompt.md: {e}"
                    );
                }
            }
        }

        // Load tool definitions from tools/ directory if present.
        let tools_dir = skill_dir.join("tools");
        if tools_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&tools_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|ext| ext == "json") {
                        match std::fs::read_to_string(&path) {
                            Ok(json_str) => {
                                match serde_json::from_str::<serde_json::Value>(&json_str) {
                                    Ok(definition) => {
                                        bundle
                                            .tool_defs
                                            .push(SkillToolDef::DynamicTool { definition });
                                    }
                                    Err(e) => {
                                        warn!(
                                            skill = %bundle.name,
                                            file = %path.display(),
                                            "failed to parse tool definition: {e}"
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                warn!(
                                    skill = %bundle.name,
                                    file = %path.display(),
                                    "failed to read tool file: {e}"
                                );
                            }
                        }
                    }
                }
            }
        }

        Ok(bundle)
    }

    /// List skill subdirectories that contain a `skill.toml`.
    fn discover_skill_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        let entries = match std::fs::read_dir(&self.skills_dir) {
            Ok(e) => e,
            Err(_) => return dirs,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join("skill.toml").exists() {
                dirs.push(path);
            }
        }
        dirs.sort();
        dirs
    }

    /// Get the collected prompt fragments from all active skills, ordered by
    /// activation order.
    pub async fn active_prompt_fragments(&self) -> Vec<String> {
        let active = self.active.read().await;
        active
            .iter()
            .filter(|s| !s.prompt_fragment.is_empty())
            .map(|s| s.prompt_fragment.clone())
            .collect()
    }

    /// Names of all currently active skills.
    pub async fn active_skill_names(&self) -> Vec<String> {
        let active = self.active.read().await;
        active.iter().map(|s| s.name.clone()).collect()
    }
}

#[async_trait]
impl SkillLoader for FileSkillLoader {
    async fn load_bundle(&self, name: &str) -> anyhow::Result<SkillBundle> {
        let skill_dir = self.skills_dir.join(name);
        if !skill_dir.is_dir() {
            anyhow::bail!("skill `{name}` not found at {}", skill_dir.display());
        }
        Self::read_manifest(&skill_dir)
    }

    async fn list_available(&self) -> anyhow::Result<Vec<SkillBundleMeta>> {
        let dirs = self.discover_skill_dirs();
        let mut metas = Vec::with_capacity(dirs.len());
        for dir in dirs {
            match Self::read_manifest(&dir) {
                Ok(bundle) => {
                    metas.push(SkillBundleMeta {
                        name: bundle.name.clone(),
                        description: bundle.description.clone(),
                        trigger: bundle.trigger.clone(),
                        has_tools: !bundle.tool_defs.is_empty(),
                        dependencies: bundle.dependencies.clone(),
                    });
                }
                Err(e) => {
                    warn!(
                        dir = %dir.display(),
                        "failed to read skill manifest: {e}"
                    );
                }
            }
        }
        metas.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(metas)
    }

    async fn activate(&self, name: &str, _ctx: &ToolContext) -> anyhow::Result<SkillActivation> {
        // Check if already active.
        {
            let active = self.active.read().await;
            if active.iter().any(|s| s.name == name) {
                anyhow::bail!("skill `{name}` is already active");
            }
        }

        let bundle = self.load_bundle(name).await?;

        // Check dependencies.
        {
            let active = self.active.read().await;
            let active_names: Vec<&str> = active.iter().map(|s| s.name.as_str()).collect();
            for dep in &bundle.dependencies {
                if !active_names.contains(&dep.as_str()) {
                    anyhow::bail!("skill `{name}` requires skill `{dep}` to be active first");
                }
            }
        }

        let prompt_fragment = bundle.prompt_template.clone();

        // Track active state.
        {
            let mut active = self.active.write().await;
            active.push(ActiveSkill {
                name: name.to_string(),
                prompt_fragment: prompt_fragment.clone(),
            });
        }

        debug!(skill = %name, "skill activated");

        // Tool instantiation from SkillToolDefs is handled by the infra crate
        // (which has access to DynamicToolDef and MCP). The activation returns
        // the raw tool_defs in the bundle for the caller to process.
        Ok(SkillActivation {
            prompt_fragment,
            tools: Vec::new(), // tools are instantiated by infra, not here
        })
    }

    async fn deactivate(&self, name: &str) -> anyhow::Result<()> {
        let mut active = self.active.write().await;
        let prev_len = active.len();
        active.retain(|s| s.name != name);
        if active.len() == prev_len {
            anyhow::bail!("skill `{name}` is not active");
        }
        debug!(skill = %name, "skill deactivated");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-skills-bundle-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    fn write_skill(skills_dir: &Path, name: &str, toml_content: &str, prompt: Option<&str>) {
        let skill_dir = skills_dir.join(name);
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(skill_dir.join("skill.toml"), toml_content).expect("write skill.toml");
        if let Some(p) = prompt {
            fs::write(skill_dir.join("prompt.md"), p).expect("write prompt.md");
        }
    }

    #[tokio::test]
    async fn load_bundle_reads_manifest_and_prompt() {
        let dir = temp_dir();
        write_skill(
            &dir,
            "test-skill",
            r#"
name = "test-skill"
description = "A test skill"

[trigger]
type = "keyword"
keywords = ["test", "lint"]
"#,
            Some("You are a test expert.\n\nAlways verify assertions."),
        );

        let loader = FileSkillLoader::new(&dir);
        let bundle = loader
            .load_bundle("test-skill")
            .await
            .expect("load should succeed");

        assert_eq!(bundle.name, "test-skill");
        assert_eq!(bundle.description, "A test skill");
        assert_eq!(
            bundle.prompt_template,
            "You are a test expert.\n\nAlways verify assertions."
        );
        assert!(matches!(
            bundle.trigger,
            agentzero_core::SkillTrigger::Keyword { ref keywords } if keywords.len() == 2
        ));
        assert_eq!(bundle.priority, 100); // default

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn list_available_discovers_skill_dirs() {
        let dir = temp_dir();
        write_skill(
            &dir,
            "alpha",
            "name = \"alpha\"\ndescription = \"Alpha skill\"",
            None,
        );
        write_skill(
            &dir,
            "beta",
            "name = \"beta\"\ndescription = \"Beta skill\"",
            Some("Beta prompt"),
        );
        // Create a non-skill directory (no skill.toml).
        fs::create_dir_all(dir.join("not-a-skill")).expect("create dir");

        let loader = FileSkillLoader::new(&dir);
        let metas = loader.list_available().await.expect("list should succeed");

        assert_eq!(metas.len(), 2);
        assert_eq!(metas[0].name, "alpha");
        assert_eq!(metas[1].name, "beta");

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn activate_and_deactivate_lifecycle() {
        let dir = temp_dir();
        write_skill(
            &dir,
            "my-skill",
            "name = \"my-skill\"\ndescription = \"My skill\"",
            Some("Skill prompt fragment"),
        );

        let loader = FileSkillLoader::new(&dir);
        let ctx = agentzero_core::ToolContext::new("/tmp/test".to_string());

        // Activate
        let activation = loader.activate("my-skill", &ctx).await.expect("activate");
        assert_eq!(activation.prompt_fragment, "Skill prompt fragment");

        // Should be in active list
        let active = loader.active_skill_names().await;
        assert_eq!(active, vec!["my-skill"]);

        // Double-activate should fail
        let err = loader
            .activate("my-skill", &ctx)
            .await
            .expect_err("should fail");
        assert!(err.to_string().contains("already active"));

        // Deactivate
        loader.deactivate("my-skill").await.expect("deactivate");
        assert!(loader.active_skill_names().await.is_empty());

        // Double-deactivate should fail
        let err = loader
            .deactivate("my-skill")
            .await
            .expect_err("should fail");
        assert!(err.to_string().contains("not active"));

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn activate_checks_dependencies() {
        let dir = temp_dir();
        write_skill(
            &dir,
            "base",
            "name = \"base\"\ndescription = \"Base\"",
            None,
        );
        write_skill(
            &dir,
            "dependent",
            "name = \"dependent\"\ndescription = \"Depends on base\"\ndependencies = [\"base\"]",
            None,
        );

        let loader = FileSkillLoader::new(&dir);
        let ctx = agentzero_core::ToolContext::new("/tmp/test".to_string());

        // Activate dependent without base should fail.
        let err = loader
            .activate("dependent", &ctx)
            .await
            .expect_err("should fail");
        assert!(err.to_string().contains("requires skill `base`"));

        // Activate base first, then dependent should succeed.
        loader.activate("base", &ctx).await.expect("activate base");
        loader
            .activate("dependent", &ctx)
            .await
            .expect("activate dependent");

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn load_bundle_with_tool_defs() {
        let dir = temp_dir();
        let skill_dir = dir.join("tool-skill");
        fs::create_dir_all(skill_dir.join("tools")).expect("create tools dir");
        fs::write(
            skill_dir.join("skill.toml"),
            "name = \"tool-skill\"\ndescription = \"Has tools\"",
        )
        .expect("write toml");
        fs::write(
            skill_dir.join("tools").join("lint.json"),
            r#"{"name": "lint", "description": "Run linter", "strategy": {"type": "shell", "command_template": "echo lint {{input}}"}}"#,
        )
        .expect("write tool json");

        let loader = FileSkillLoader::new(&dir);
        let bundle = loader.load_bundle("tool-skill").await.expect("load");

        assert_eq!(bundle.tool_defs.len(), 1);
        assert!(matches!(
            bundle.tool_defs[0],
            SkillToolDef::DynamicTool { .. }
        ));

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn active_prompt_fragments_returns_ordered() {
        let dir = temp_dir();
        write_skill(
            &dir,
            "a",
            "name = \"a\"\ndescription = \"A\"",
            Some("Prompt A"),
        );
        write_skill(
            &dir,
            "b",
            "name = \"b\"\ndescription = \"B\"",
            Some("Prompt B"),
        );

        let loader = FileSkillLoader::new(&dir);
        let ctx = agentzero_core::ToolContext::new("/tmp/test".to_string());

        loader.activate("a", &ctx).await.expect("activate a");
        loader.activate("b", &ctx).await.expect("activate b");

        let fragments = loader.active_prompt_fragments().await;
        assert_eq!(fragments, vec!["Prompt A", "Prompt B"]);

        let _ = fs::remove_dir_all(dir);
    }
}
