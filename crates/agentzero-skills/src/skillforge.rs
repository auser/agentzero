use anyhow::bail;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillTemplate {
    pub name: String,
    pub description: String,
}

pub fn validate_skill_name(name: &str) -> anyhow::Result<()> {
    if name.trim().is_empty() {
        bail!("skill name cannot be empty");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("skill name contains invalid characters");
    }
    Ok(())
}

pub fn render_skill_markdown(template: &SkillTemplate) -> anyhow::Result<String> {
    validate_skill_name(&template.name)?;
    if template.description.trim().is_empty() {
        bail!("skill description cannot be empty");
    }

    Ok(format!(
        "# {}\n\n## Purpose\n{}\n\n## Usage\n- Add clear instructions and guardrails for this skill.\n",
        template.name, template.description
    ))
}

#[cfg(test)]
mod tests {
    use super::{render_skill_markdown, SkillTemplate};

    #[test]
    fn render_skill_markdown_success_path() {
        let markdown = render_skill_markdown(&SkillTemplate {
            name: "my_skill".to_string(),
            description: "Test skill".to_string(),
        })
        .expect("render should succeed");

        assert!(markdown.contains("# my_skill"));
        assert!(markdown.contains("## Purpose"));
    }

    #[test]
    fn render_skill_markdown_rejects_invalid_name_negative_path() {
        let err = render_skill_markdown(&SkillTemplate {
            name: "bad skill".to_string(),
            description: "Test skill".to_string(),
        })
        .expect_err("invalid name should fail");

        assert!(err.to_string().contains("invalid characters"));
    }
}
