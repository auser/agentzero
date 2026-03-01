use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

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

pub fn template_paths_for_workspace(workspace_root: &Path) -> Vec<PathBuf> {
    TEMPLATE_LOAD_ORDER
        .iter()
        .map(|template| workspace_root.join(template.file_name()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{template_paths_for_workspace, TemplateFile, TEMPLATE_LOAD_ORDER};
    use std::path::Path;

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
}
