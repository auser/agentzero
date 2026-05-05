//! Skill packaging: tarball creation, checksum computation, and extraction.
//!
//! Used by both `agentzero publish` (create tarball) and `agentzero install`
//! (verify and extract tarball). No network code here — purely local I/O.

use std::io::Read;
use std::path::Path;

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use sha2::{Digest, Sha256};

use crate::registry::RegistryError;

/// A packaged skill ready for publishing or verification.
pub struct SkillPackage {
    pub name: String,
    pub version: String,
    pub tarball: Vec<u8>,
    pub checksum: String,
    pub manifest: crate::SkillManifest,
}

/// Compute the SHA-256 checksum of arbitrary bytes.
///
/// Returns a string like `sha256:a1b2c3...`.
pub fn compute_checksum(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash = hasher.finalize();
    format!("sha256:{}", hex_encode(&hash))
}

/// Verify a tarball's checksum against an expected value.
pub fn verify_checksum(data: &[u8], expected: &str) -> Result<(), RegistryError> {
    let actual = compute_checksum(data);
    if actual != expected {
        return Err(RegistryError::ChecksumMismatch {
            expected: expected.to_string(),
            actual,
        });
    }
    Ok(())
}

/// Package a skill directory into a `.tar.gz` tarball.
///
/// The tarball contains all files in the skill directory, rooted at the
/// skill name (e.g., `my-skill/SKILL.md`, `my-skill/run.sh`).
pub fn package_skill(skill_dir: &Path) -> Result<SkillPackage, RegistryError> {
    let manifest = crate::registry::load_manifest(skill_dir)?;

    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    {
        let mut archive = tar::Builder::new(&mut encoder);
        archive
            .append_dir_all(&manifest.name, skill_dir)
            .map_err(|e| RegistryError::IoError(format!("failed to create tarball: {e}")))?;
        archive
            .finish()
            .map_err(|e| RegistryError::IoError(format!("failed to finish tarball: {e}")))?;
    }
    let tarball = encoder
        .finish()
        .map_err(|e| RegistryError::IoError(format!("failed to compress tarball: {e}")))?;

    let checksum = compute_checksum(&tarball);

    Ok(SkillPackage {
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        tarball,
        checksum,
        manifest,
    })
}

/// Extract a `.tar.gz` tarball into a destination directory.
///
/// The tarball is expected to contain a single top-level directory (the skill name).
/// After extraction, the skill files will be at `dest/<skill-name>/`.
pub fn extract_tarball(tarball: &[u8], dest: &Path) -> Result<String, RegistryError> {
    let decoder = GzDecoder::new(tarball);
    let mut archive = tar::Archive::new(decoder);

    let mut skill_name = String::new();

    for entry in archive
        .entries()
        .map_err(|e| RegistryError::IoError(format!("failed to read tarball entries: {e}")))?
    {
        let mut entry =
            entry.map_err(|e| RegistryError::IoError(format!("bad tarball entry: {e}")))?;

        let path = entry
            .path()
            .map_err(|e| RegistryError::IoError(format!("bad entry path: {e}")))?
            .to_path_buf();

        // Extract the top-level directory name from the first entry
        if skill_name.is_empty() {
            if let Some(first) = path.components().next() {
                skill_name = first.as_os_str().to_string_lossy().to_string();
            }
        }

        let dest_path = dest.join(&path);

        // Create parent directories
        if let Some(parent) = dest_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| RegistryError::IoError(format!("failed to create dir: {e}")))?;
        }

        // Extract files (skip directories — they're created above)
        if entry.header().entry_type().is_file() {
            let mut contents = Vec::new();
            entry
                .read_to_end(&mut contents)
                .map_err(|e| RegistryError::IoError(format!("failed to read entry: {e}")))?;
            std::fs::write(&dest_path, &contents).map_err(|e| {
                RegistryError::IoError(format!("failed to write {}: {e}", dest_path.display()))
            })?;
        }
    }

    if skill_name.is_empty() {
        return Err(RegistryError::ParseError(
            "tarball appears to be empty".into(),
        ));
    }

    Ok(skill_name)
}

/// Encode bytes as lowercase hex.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "agentzero-package-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should be after epoch")
                .as_nanos()
        ))
    }

    fn create_test_skill(dir: &Path) {
        fs::create_dir_all(dir).expect("should create");
        fs::write(
            dir.join("SKILL.md"),
            "---\nname: test-pkg\nversion: 1.0.0\nruntime: none\n---\n# Test\n",
        )
        .expect("should write");
        fs::write(dir.join("patterns.toml"), "# patterns\n").expect("should write");
    }

    #[test]
    fn compute_checksum_is_deterministic() {
        let data = b"hello world";
        let c1 = compute_checksum(data);
        let c2 = compute_checksum(data);
        assert_eq!(c1, c2);
        assert!(c1.starts_with("sha256:"));
        assert_eq!(c1.len(), 7 + 64); // "sha256:" + 64 hex chars
    }

    #[test]
    fn verify_checksum_passes() {
        let data = b"test data";
        let checksum = compute_checksum(data);
        assert!(verify_checksum(data, &checksum).is_ok());
    }

    #[test]
    fn verify_checksum_fails_on_mismatch() {
        let data = b"test data";
        let result = verify_checksum(
            data,
            "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        );
        assert!(result.is_err());
    }

    #[test]
    fn package_skill_creates_tarball() {
        let dir = temp_dir("pkg-create");
        let skill_dir = dir.join("test-pkg");
        create_test_skill(&skill_dir);

        let pkg = package_skill(&skill_dir).expect("should package");
        assert_eq!(pkg.name, "test-pkg");
        assert_eq!(pkg.version, "1.0.0");
        assert!(!pkg.tarball.is_empty());
        assert!(pkg.checksum.starts_with("sha256:"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn package_and_extract_roundtrip() {
        let dir = temp_dir("pkg-roundtrip");
        let skill_dir = dir.join("test-pkg");
        create_test_skill(&skill_dir);

        let pkg = package_skill(&skill_dir).expect("should package");

        // Verify checksum
        assert!(verify_checksum(&pkg.tarball, &pkg.checksum).is_ok());

        // Extract to a different location
        let extract_dir = dir.join("extracted");
        fs::create_dir_all(&extract_dir).expect("should create");
        let name = extract_tarball(&pkg.tarball, &extract_dir).expect("should extract");
        assert_eq!(name, "test-pkg");

        // Verify files exist
        assert!(extract_dir.join("test-pkg/SKILL.md").exists());
        assert!(extract_dir.join("test-pkg/patterns.toml").exists());

        // Verify content matches
        let original = fs::read_to_string(skill_dir.join("SKILL.md")).expect("read");
        let extracted = fs::read_to_string(extract_dir.join("test-pkg/SKILL.md")).expect("read");
        assert_eq!(original, extracted);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn package_missing_skill_fails() {
        let result = package_skill(Path::new("/nonexistent/skill"));
        assert!(result.is_err());
    }
}
