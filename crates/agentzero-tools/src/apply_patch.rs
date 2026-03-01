use anyhow::Context;

const BEGIN_PATCH: &str = "*** Begin Patch";
const END_PATCH: &str = "*** End Patch";

#[derive(Debug, Default, Clone, Copy)]
pub struct ApplyPatchTool;

impl ApplyPatchTool {
    pub fn validate_patch(&self, patch: &str) -> anyhow::Result<()> {
        let trimmed = patch.trim();
        if trimmed.is_empty() {
            anyhow::bail!("patch must not be empty");
        }
        let first = trimmed
            .lines()
            .next()
            .context("patch must include a begin marker")?;
        if first != BEGIN_PATCH {
            anyhow::bail!("patch must start with `{BEGIN_PATCH}`");
        }
        if !trimmed.lines().any(|line| line == END_PATCH) {
            anyhow::bail!("patch must end with `{END_PATCH}`");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ApplyPatchTool;

    #[test]
    fn validate_patch_accepts_basic_envelope_success_path() {
        let tool = ApplyPatchTool;
        let patch = "*** Begin Patch\n*** Update File: test.txt\n@@\n-old\n+new\n*** End Patch\n";
        tool.validate_patch(patch)
            .expect("well-formed patch should validate");
    }

    #[test]
    fn validate_patch_rejects_missing_begin_marker_negative_path() {
        let tool = ApplyPatchTool;
        let err = tool
            .validate_patch("*** Update File: test.txt\n*** End Patch\n")
            .expect_err("missing begin marker should fail");
        assert!(err.to_string().contains("patch must start with"));
    }
}
