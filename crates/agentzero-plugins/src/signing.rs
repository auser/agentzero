//! Ed25519 plugin manifest signing and verification.
//!
//! Signs the canonical JSON representation of a `PluginManifest` (excluding
//! the `signature` and `signing_key_id` fields) with an Ed25519 private key.
//! Verification checks the signature against the public key.

use crate::package::PluginManifest;
use anyhow::{anyhow, Context};
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use serde_json::json;

/// Generate a new Ed25519 keypair.
///
/// Returns `(private_key_hex, public_key_hex)`.
pub fn generate_keypair() -> (String, String) {
    let mut rng = rand::thread_rng();
    let signing_key = SigningKey::generate(&mut rng);
    let verifying_key = signing_key.verifying_key();
    (
        hex::encode(signing_key.to_bytes()),
        hex::encode(verifying_key.to_bytes()),
    )
}

/// Sign a plugin manifest with an Ed25519 private key.
///
/// Returns the hex-encoded signature.
pub fn sign_manifest(manifest: &PluginManifest, private_key_hex: &str) -> anyhow::Result<String> {
    let key_bytes = hex::decode(private_key_hex).context("invalid hex in private key")?;
    let key_array: [u8; 32] = key_bytes
        .try_into()
        .map_err(|_| anyhow!("private key must be exactly 32 bytes"))?;
    let signing_key = SigningKey::from_bytes(&key_array);

    let canonical = canonical_manifest_json(manifest);
    let signature = signing_key.sign(canonical.as_bytes());
    Ok(hex::encode(signature.to_bytes()))
}

/// Verify a plugin manifest signature against an Ed25519 public key.
pub fn verify_manifest(
    manifest: &PluginManifest,
    signature_hex: &str,
    public_key_hex: &str,
) -> anyhow::Result<bool> {
    let key_bytes = hex::decode(public_key_hex).context("invalid hex in public key")?;
    let key_array: [u8; 32] = key_bytes
        .try_into()
        .map_err(|_| anyhow!("public key must be exactly 32 bytes"))?;
    let verifying_key = VerifyingKey::from_bytes(&key_array).context("invalid public key")?;

    let sig_bytes = hex::decode(signature_hex).context("invalid hex in signature")?;
    let sig_array: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| anyhow!("signature must be exactly 64 bytes"))?;
    let signature = ed25519_dalek::Signature::from_bytes(&sig_array);

    let canonical = canonical_manifest_json(manifest);
    Ok(verifying_key
        .verify(canonical.as_bytes(), &signature)
        .is_ok())
}

/// Produce a deterministic JSON representation of the manifest for signing.
///
/// Excludes `signature` and `signing_key_id` fields so the same manifest
/// produces the same canonical form before and after signing.
fn canonical_manifest_json(manifest: &PluginManifest) -> String {
    // Build a sorted JSON object with only the signable fields.
    let canonical = json!({
        "allowed_host_calls": manifest.allowed_host_calls,
        "capabilities": manifest.capabilities,
        "dependencies": manifest.dependencies,
        "description": manifest.description,
        "entrypoint": manifest.entrypoint,
        "hooks": manifest.hooks,
        "id": manifest.id,
        "max_runtime_api": manifest.max_runtime_api,
        "min_runtime_api": manifest.min_runtime_api,
        "version": manifest.version,
        "wasm_file": manifest.wasm_file,
        "wasm_sha256": manifest.wasm_sha256,
    });
    // serde_json sorts keys in Value::Object when using json! macro.
    serde_json::to_string(&canonical).expect("canonical JSON should serialize")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::PluginManifest;

    fn test_manifest() -> PluginManifest {
        PluginManifest {
            id: "test-plugin".to_string(),
            version: "1.0.0".to_string(),
            description: Some("A test plugin".to_string()),
            entrypoint: "run".to_string(),
            wasm_file: "plugin.wasm".to_string(),
            wasm_sha256: "abc123".to_string(),
            capabilities: vec!["net".to_string()],
            hooks: vec![],
            min_runtime_api: 2,
            max_runtime_api: 2,
            allowed_host_calls: vec!["log".to_string()],
            dependencies: vec![],
            signature: None,
            signing_key_id: None,
        }
    }

    #[test]
    fn generate_keypair_returns_valid_hex() {
        let (private_key, public_key) = generate_keypair();
        assert_eq!(private_key.len(), 64, "private key should be 64 hex chars");
        assert_eq!(public_key.len(), 64, "public key should be 64 hex chars");
        assert!(hex::decode(&private_key).is_ok());
        assert!(hex::decode(&public_key).is_ok());
    }

    #[test]
    fn sign_and_verify_roundtrip() {
        let (private_key, public_key) = generate_keypair();
        let manifest = test_manifest();

        let signature = sign_manifest(&manifest, &private_key).expect("signing should succeed");
        assert_eq!(signature.len(), 128, "signature should be 128 hex chars");

        let valid =
            verify_manifest(&manifest, &signature, &public_key).expect("verify should succeed");
        assert!(valid, "signature should be valid");
    }

    #[test]
    fn tampered_manifest_fails_verification() {
        let (private_key, public_key) = generate_keypair();
        let manifest = test_manifest();

        let signature = sign_manifest(&manifest, &private_key).expect("signing should succeed");

        // Tamper with the manifest.
        let mut tampered = manifest;
        tampered.version = "2.0.0".to_string();

        let valid =
            verify_manifest(&tampered, &signature, &public_key).expect("verify should succeed");
        assert!(!valid, "tampered manifest should fail verification");
    }

    #[test]
    fn wrong_key_fails_verification() {
        let (private_key, _public_key) = generate_keypair();
        let (_other_private, other_public) = generate_keypair();
        let manifest = test_manifest();

        let signature = sign_manifest(&manifest, &private_key).expect("signing should succeed");

        let valid =
            verify_manifest(&manifest, &signature, &other_public).expect("verify should succeed");
        assert!(!valid, "wrong public key should fail verification");
    }

    #[test]
    fn invalid_private_key_returns_error() {
        let manifest = test_manifest();
        let err = sign_manifest(&manifest, "not-hex").expect_err("should fail");
        assert!(err.to_string().contains("hex"));
    }

    #[test]
    fn invalid_public_key_returns_error() {
        let err = verify_manifest(&test_manifest(), "aa".repeat(64).as_str(), "not-hex")
            .expect_err("should fail");
        assert!(err.to_string().contains("hex"));
    }

    #[test]
    fn short_private_key_returns_error() {
        let manifest = test_manifest();
        let short_key = "aabb"; // Only 2 bytes, needs 32
        let err = sign_manifest(&manifest, short_key).expect_err("should fail");
        assert!(err.to_string().contains("32 bytes"));
    }

    #[test]
    fn canonical_json_is_deterministic() {
        let manifest = test_manifest();
        let json1 = canonical_manifest_json(&manifest);
        let json2 = canonical_manifest_json(&manifest);
        assert_eq!(json1, json2);
    }
}
