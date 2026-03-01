use anyhow::anyhow;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MediaKind {
    Text,
    Image,
    Audio,
    Unknown,
}

pub fn infer_media_kind(path: &str) -> anyhow::Result<MediaKind> {
    if path.trim().is_empty() {
        return Err(anyhow!("media path cannot be empty"));
    }

    let lowered = path.to_ascii_lowercase();
    let kind = if lowered.ends_with(".txt") || lowered.ends_with(".md") {
        MediaKind::Text
    } else if lowered.ends_with(".png") || lowered.ends_with(".jpg") || lowered.ends_with(".jpeg") {
        MediaKind::Image
    } else if lowered.ends_with(".mp3") || lowered.ends_with(".wav") {
        MediaKind::Audio
    } else {
        MediaKind::Unknown
    };

    Ok(kind)
}

#[cfg(test)]
mod tests {
    use super::{infer_media_kind, MediaKind};

    #[test]
    fn infer_media_kind_for_image_success_path() {
        let kind = infer_media_kind("artifact.png").expect("media kind should parse");
        assert_eq!(kind, MediaKind::Image);
    }

    #[test]
    fn infer_media_kind_rejects_empty_path_negative_path() {
        let err = infer_media_kind("  ").expect_err("empty path should fail");
        assert!(err.to_string().contains("cannot be empty"));
    }
}
