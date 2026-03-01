use std::path::{Path, PathBuf};

/// All supported template files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TemplateFile {
    Agents,
    Boot,
    Bootstrap,
    Heartbeat,
    Identity,
    Soul,
    Tools,
    User,
}

impl TemplateFile {
    pub fn file_name(self) -> &'static str {
        match self {
            Self::Agents => "AGENTS.md",
            Self::Boot => "BOOT.md",
            Self::Bootstrap => "BOOTSTRAP.md",
            Self::Heartbeat => "HEARTBEAT.md",
            Self::Identity => "IDENTITY.md",
            Self::Soul => "SOUL.md",
            Self::Tools => "TOOLS.md",
            Self::User => "USER.md",
        }
    }

    /// Whether this template is scoped to the main session only (not shared sessions).
    pub fn is_main_session_only(self) -> bool {
        matches!(self, Self::Boot | Self::Bootstrap)
    }

    /// Whether this template is shared across all sessions.
    pub fn is_shared(self) -> bool {
        !self.is_main_session_only()
    }
}

/// Deterministic load order for templates. Identity and Soul come first
/// (define who the agent is), then Tools and Agents (define what it can do),
/// then Boot/Bootstrap (initialization), Heartbeat (lifecycle), User (context).
pub const TEMPLATE_LOAD_ORDER: &[TemplateFile] = &[
    TemplateFile::Identity,
    TemplateFile::Soul,
    TemplateFile::Tools,
    TemplateFile::Agents,
    TemplateFile::Boot,
    TemplateFile::Bootstrap,
    TemplateFile::Heartbeat,
    TemplateFile::User,
];

/// Templates that only load in the main session.
pub const MAIN_SESSION_TEMPLATES: &[TemplateFile] = &[TemplateFile::Boot, TemplateFile::Bootstrap];

/// Templates that load in all sessions (main + shared).
pub const SHARED_SESSION_TEMPLATES: &[TemplateFile] = &[
    TemplateFile::Identity,
    TemplateFile::Soul,
    TemplateFile::Tools,
    TemplateFile::Agents,
    TemplateFile::Heartbeat,
    TemplateFile::User,
];

/// Template search directories in precedence order (highest first).
///
/// 1. Workspace root — project-specific overrides (e.g., `./AGENTS.md`)
/// 2. `.agentzero/` — project config directory (e.g., `./.agentzero/AGENTS.md`)
/// 3. Global config — user-wide defaults (e.g., `~/.config/agentzero/AGENTS.md`)
pub fn template_search_dirs(
    workspace_root: &Path,
    global_config_dir: Option<&Path>,
) -> Vec<PathBuf> {
    let mut dirs = vec![
        workspace_root.to_path_buf(),
        workspace_root.join(".agentzero"),
    ];
    if let Some(global) = global_config_dir {
        dirs.push(global.to_path_buf());
    }
    dirs
}

/// Generate template paths for a workspace (workspace root only, for backward compatibility).
pub fn template_paths_for_workspace(workspace_root: &Path) -> Vec<PathBuf> {
    TEMPLATE_LOAD_ORDER
        .iter()
        .map(|template| workspace_root.join(template.file_name()))
        .collect()
}

/// A resolved template with its source location and content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTemplate {
    pub template: TemplateFile,
    pub source: PathBuf,
    pub content: String,
}

/// Result of discovering templates in a workspace.
#[derive(Debug, Clone)]
pub struct TemplateSet {
    pub templates: Vec<ResolvedTemplate>,
    pub missing: Vec<TemplateFile>,
}

impl TemplateSet {
    /// Get the content for a specific template, if loaded.
    pub fn get(&self, template: TemplateFile) -> Option<&ResolvedTemplate> {
        self.templates.iter().find(|t| t.template == template)
    }

    /// Get all templates appropriate for the main session.
    pub fn main_session_templates(&self) -> Vec<&ResolvedTemplate> {
        self.templates
            .iter()
            .filter(|_| true) // main session gets all templates
            .collect()
    }

    /// Get only templates appropriate for shared sessions.
    pub fn shared_session_templates(&self) -> Vec<&ResolvedTemplate> {
        self.templates
            .iter()
            .filter(|t| t.template.is_shared())
            .collect()
    }

    /// Format a guidance message for missing templates.
    pub fn missing_guidance(&self) -> Option<String> {
        if self.missing.is_empty() {
            return None;
        }
        let names: Vec<&str> = self.missing.iter().map(|t| t.file_name()).collect();
        Some(format!(
            "Optional templates not found: {}. Create them in your workspace root or .agentzero/ directory to customize agent behavior.",
            names.join(", ")
        ))
    }
}

/// Discover a single template by searching directories in precedence order.
///
/// Returns the first match found. Higher-precedence directories shadow lower ones.
fn discover_template(template: TemplateFile, search_dirs: &[PathBuf]) -> Option<PathBuf> {
    let name = template.file_name();
    for dir in search_dirs {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Discover and load all templates from the workspace.
///
/// Templates are discovered using the precedence rules:
/// 1. Workspace root (highest priority)
/// 2. `.agentzero/` directory
/// 3. Global config directory (lowest priority)
///
/// Missing templates are tracked but do not cause errors — they are optional.
/// Templates are loaded in the deterministic `TEMPLATE_LOAD_ORDER`.
pub fn discover_templates(workspace_root: &Path, global_config_dir: Option<&Path>) -> TemplateSet {
    let search_dirs = template_search_dirs(workspace_root, global_config_dir);
    let mut templates = Vec::new();
    let mut missing = Vec::new();

    for &template in TEMPLATE_LOAD_ORDER {
        match discover_template(template, &search_dirs) {
            Some(path) => match std::fs::read_to_string(&path) {
                Ok(content) => {
                    templates.push(ResolvedTemplate {
                        template,
                        source: path,
                        content,
                    });
                }
                Err(_) => {
                    missing.push(template);
                }
            },
            None => {
                missing.push(template);
            }
        }
    }

    TemplateSet { templates, missing }
}

/// Discover templates for a shared session (excludes main-session-only templates).
pub fn discover_shared_templates(
    workspace_root: &Path,
    global_config_dir: Option<&Path>,
) -> TemplateSet {
    let full = discover_templates(workspace_root, global_config_dir);
    let templates: Vec<ResolvedTemplate> = full
        .templates
        .into_iter()
        .filter(|t| t.template.is_shared())
        .collect();
    let missing: Vec<TemplateFile> = SHARED_SESSION_TEMPLATES
        .iter()
        .copied()
        .filter(|t| !templates.iter().any(|rt| rt.template == *t))
        .collect();
    TemplateSet { templates, missing }
}

/// List all template files that exist in the given search directories,
/// showing which directory each was resolved from.
pub fn list_template_sources(
    workspace_root: &Path,
    global_config_dir: Option<&Path>,
) -> Vec<(TemplateFile, Option<PathBuf>)> {
    let search_dirs = template_search_dirs(workspace_root, global_config_dir);
    TEMPLATE_LOAD_ORDER
        .iter()
        .map(|&template| {
            let path = discover_template(template, &search_dirs);
            (template, path)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-templates-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    // --- Existing tests (preserved) ---

    #[test]
    fn template_paths_follow_declared_load_order_success_path() {
        let root = Path::new("/tmp/workspace");
        let paths = template_paths_for_workspace(root);
        assert_eq!(paths.len(), TEMPLATE_LOAD_ORDER.len());
        assert_eq!(paths[0].to_string_lossy(), "/tmp/workspace/IDENTITY.md");
        assert_eq!(
            paths
                .last()
                .expect("last path should exist")
                .to_string_lossy(),
            "/tmp/workspace/USER.md"
        );
    }

    #[test]
    fn template_file_names_are_uppercase_markdown_negative_path() {
        for template in TEMPLATE_LOAD_ORDER {
            let name = template.file_name();
            assert!(name.ends_with(".md"));
            let stem = name.trim_end_matches(".md");
            assert!(
                stem.chars().all(|ch| !ch.is_ascii_lowercase()),
                "template name should remain uppercase: {name}"
            );
        }
        assert_eq!(TemplateFile::Agents.file_name(), "AGENTS.md");
    }

    // --- Discovery tests ---

    #[test]
    fn discover_templates_finds_workspace_root_files() {
        let dir = temp_dir();
        fs::write(dir.join("AGENTS.md"), "# Agents rules").unwrap();
        fs::write(dir.join("IDENTITY.md"), "# Identity").unwrap();

        let result = discover_templates(&dir, None);
        assert_eq!(result.templates.len(), 2);
        assert!(result.get(TemplateFile::Agents).is_some());
        assert!(result.get(TemplateFile::Identity).is_some());
        assert_eq!(
            result.get(TemplateFile::Agents).unwrap().content,
            "# Agents rules"
        );

        // Other templates are missing
        assert!(result.missing.contains(&TemplateFile::Boot));
        assert!(result.missing.contains(&TemplateFile::Soul));

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn discover_templates_finds_agentzero_dir_files() {
        let dir = temp_dir();
        let az_dir = dir.join(".agentzero");
        fs::create_dir_all(&az_dir).unwrap();
        fs::write(az_dir.join("SOUL.md"), "# Soul from .agentzero").unwrap();

        let result = discover_templates(&dir, None);
        assert_eq!(result.templates.len(), 1);
        let soul = result.get(TemplateFile::Soul).unwrap();
        assert_eq!(soul.content, "# Soul from .agentzero");
        assert!(soul.source.to_string_lossy().contains(".agentzero"));

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn discover_templates_finds_global_dir_files() {
        let workspace = temp_dir();
        let global = temp_dir();
        fs::write(global.join("TOOLS.md"), "# Global tools").unwrap();

        let result = discover_templates(&workspace, Some(&global));
        assert_eq!(result.templates.len(), 1);
        let tools = result.get(TemplateFile::Tools).unwrap();
        assert_eq!(tools.content, "# Global tools");

        fs::remove_dir_all(workspace).ok();
        fs::remove_dir_all(global).ok();
    }

    // --- Precedence tests ---

    #[test]
    fn workspace_root_overrides_agentzero_dir() {
        let dir = temp_dir();
        let az_dir = dir.join(".agentzero");
        fs::create_dir_all(&az_dir).unwrap();

        fs::write(dir.join("AGENTS.md"), "workspace version").unwrap();
        fs::write(az_dir.join("AGENTS.md"), "agentzero dir version").unwrap();

        let result = discover_templates(&dir, None);
        let agents = result.get(TemplateFile::Agents).unwrap();
        assert_eq!(agents.content, "workspace version");
        assert!(!agents.source.to_string_lossy().contains(".agentzero"));

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn agentzero_dir_overrides_global() {
        let workspace = temp_dir();
        let global = temp_dir();
        let az_dir = workspace.join(".agentzero");
        fs::create_dir_all(&az_dir).unwrap();

        fs::write(az_dir.join("SOUL.md"), "project soul").unwrap();
        fs::write(global.join("SOUL.md"), "global soul").unwrap();

        let result = discover_templates(&workspace, Some(&global));
        let soul = result.get(TemplateFile::Soul).unwrap();
        assert_eq!(soul.content, "project soul");

        fs::remove_dir_all(workspace).ok();
        fs::remove_dir_all(global).ok();
    }

    #[test]
    fn workspace_root_overrides_global() {
        let workspace = temp_dir();
        let global = temp_dir();

        fs::write(workspace.join("IDENTITY.md"), "workspace identity").unwrap();
        fs::write(global.join("IDENTITY.md"), "global identity").unwrap();

        let result = discover_templates(&workspace, Some(&global));
        let identity = result.get(TemplateFile::Identity).unwrap();
        assert_eq!(identity.content, "workspace identity");

        fs::remove_dir_all(workspace).ok();
        fs::remove_dir_all(global).ok();
    }

    // --- Missing-file behavior ---

    #[test]
    fn empty_workspace_returns_all_missing() {
        let dir = temp_dir();
        let result = discover_templates(&dir, None);
        assert!(result.templates.is_empty());
        assert_eq!(result.missing.len(), TEMPLATE_LOAD_ORDER.len());

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn missing_guidance_lists_files() {
        let dir = temp_dir();
        let result = discover_templates(&dir, None);
        let guidance = result.missing_guidance().expect("should have guidance");
        assert!(guidance.contains("AGENTS.md"));
        assert!(guidance.contains("IDENTITY.md"));
        assert!(guidance.contains(".agentzero/"));

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn no_missing_guidance_when_all_present() {
        let dir = temp_dir();
        for template in TEMPLATE_LOAD_ORDER {
            fs::write(dir.join(template.file_name()), "content").unwrap();
        }

        let result = discover_templates(&dir, None);
        assert!(result.missing.is_empty());
        assert!(result.missing_guidance().is_none());

        fs::remove_dir_all(dir).ok();
    }

    // --- Session scoping tests ---

    #[test]
    fn boot_and_bootstrap_are_main_session_only() {
        assert!(TemplateFile::Boot.is_main_session_only());
        assert!(TemplateFile::Bootstrap.is_main_session_only());
        assert!(!TemplateFile::Agents.is_main_session_only());
        assert!(!TemplateFile::Identity.is_main_session_only());
        assert!(!TemplateFile::Soul.is_main_session_only());
    }

    #[test]
    fn main_session_gets_all_templates() {
        let dir = temp_dir();
        fs::write(dir.join("BOOT.md"), "boot").unwrap();
        fs::write(dir.join("AGENTS.md"), "agents").unwrap();
        fs::write(dir.join("IDENTITY.md"), "identity").unwrap();

        let result = discover_templates(&dir, None);
        let main = result.main_session_templates();
        assert_eq!(main.len(), 3);

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn shared_session_excludes_boot_and_bootstrap() {
        let dir = temp_dir();
        fs::write(dir.join("BOOT.md"), "boot").unwrap();
        fs::write(dir.join("BOOTSTRAP.md"), "bootstrap").unwrap();
        fs::write(dir.join("AGENTS.md"), "agents").unwrap();
        fs::write(dir.join("IDENTITY.md"), "identity").unwrap();

        let result = discover_templates(&dir, None);
        let shared = result.shared_session_templates();
        // Should only have AGENTS and IDENTITY, not BOOT or BOOTSTRAP
        assert_eq!(shared.len(), 2);
        assert!(shared.iter().all(|t| t.template.is_shared()));
        assert!(!shared.iter().any(|t| t.template == TemplateFile::Boot));
        assert!(!shared.iter().any(|t| t.template == TemplateFile::Bootstrap));

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn discover_shared_templates_excludes_main_only() {
        let dir = temp_dir();
        fs::write(dir.join("BOOT.md"), "boot").unwrap();
        fs::write(dir.join("AGENTS.md"), "agents").unwrap();

        let result = discover_shared_templates(&dir, None);
        assert_eq!(result.templates.len(), 1);
        assert_eq!(result.templates[0].template, TemplateFile::Agents);
        // BOOT is not in the missing list since it's not a shared template
        assert!(!result.missing.contains(&TemplateFile::Boot));

        fs::remove_dir_all(dir).ok();
    }

    // --- Load order tests ---

    #[test]
    fn templates_loaded_in_deterministic_order() {
        let dir = temp_dir();
        // Write in reverse order to verify load order is by TEMPLATE_LOAD_ORDER, not filesystem
        fs::write(dir.join("USER.md"), "user").unwrap();
        fs::write(dir.join("HEARTBEAT.md"), "heartbeat").unwrap();
        fs::write(dir.join("BOOTSTRAP.md"), "bootstrap").unwrap();
        fs::write(dir.join("BOOT.md"), "boot").unwrap();
        fs::write(dir.join("AGENTS.md"), "agents").unwrap();
        fs::write(dir.join("TOOLS.md"), "tools").unwrap();
        fs::write(dir.join("SOUL.md"), "soul").unwrap();
        fs::write(dir.join("IDENTITY.md"), "identity").unwrap();

        let result = discover_templates(&dir, None);
        assert_eq!(result.templates.len(), 8);

        // Verify order matches TEMPLATE_LOAD_ORDER
        for (i, resolved) in result.templates.iter().enumerate() {
            assert_eq!(resolved.template, TEMPLATE_LOAD_ORDER[i]);
        }

        fs::remove_dir_all(dir).ok();
    }

    // --- list_template_sources ---

    #[test]
    fn list_sources_shows_found_and_missing() {
        let dir = temp_dir();
        fs::write(dir.join("AGENTS.md"), "agents").unwrap();

        let sources = list_template_sources(&dir, None);
        assert_eq!(sources.len(), TEMPLATE_LOAD_ORDER.len());

        let agents_entry = sources
            .iter()
            .find(|(t, _)| *t == TemplateFile::Agents)
            .unwrap();
        assert!(agents_entry.1.is_some());

        let boot_entry = sources
            .iter()
            .find(|(t, _)| *t == TemplateFile::Boot)
            .unwrap();
        assert!(boot_entry.1.is_none());

        fs::remove_dir_all(dir).ok();
    }

    // --- Search dirs ---

    #[test]
    fn search_dirs_include_all_locations() {
        let workspace = Path::new("/workspace");
        let global = Path::new("/global");
        let dirs = template_search_dirs(workspace, Some(global));
        assert_eq!(dirs.len(), 3);
        assert_eq!(dirs[0], PathBuf::from("/workspace"));
        assert_eq!(dirs[1], PathBuf::from("/workspace/.agentzero"));
        assert_eq!(dirs[2], PathBuf::from("/global"));
    }

    #[test]
    fn search_dirs_without_global() {
        let workspace = Path::new("/workspace");
        let dirs = template_search_dirs(workspace, None);
        assert_eq!(dirs.len(), 2);
        assert_eq!(dirs[0], PathBuf::from("/workspace"));
        assert_eq!(dirs[1], PathBuf::from("/workspace/.agentzero"));
    }
}
