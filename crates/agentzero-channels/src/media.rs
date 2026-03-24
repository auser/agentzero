//! Automatic media understanding pipeline.
//!
//! Processes media attachments and URLs in inbound channel messages,
//! adding transcripts and descriptions for agent consumption.

use serde::{Deserialize, Serialize};

/// A media attachment on a channel message.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MediaAttachment {
    /// MIME type (e.g., "audio/wav", "image/png").
    pub mime_type: String,
    /// URL to the media resource (if available).
    #[serde(default)]
    pub url: Option<String>,
    /// Transcript of audio content (filled by pipeline).
    #[serde(default)]
    pub transcript: Option<String>,
    /// Description of visual content (filled by pipeline).
    #[serde(default)]
    pub description: Option<String>,
}

/// Configuration for the media pipeline.
#[derive(Debug, Clone, Default)]
pub struct MediaPipelineConfig {
    /// Whether the media pipeline is enabled.
    pub enabled: bool,
}

/// Process media in a channel message.
///
/// This function:
/// 1. Processes any `attachments` already present (from media-aware channels)
/// 2. Scans `content` for common media URLs (image/audio links)
/// 3. Enriches the message with transcripts and descriptions
///
/// All processing is fallible — errors are logged and skipped, never blocking the message.
pub async fn process_media(
    attachments: &mut Vec<MediaAttachment>,
    content: &str,
    _config: &MediaPipelineConfig,
) {
    // Detect media URLs in content and add as attachments
    detect_media_urls(content, attachments);

    // Future: transcribe audio attachments, describe images
    // For now, the pipeline infrastructure is in place but actual
    // transcription/vision API calls will be added when those
    // provider integrations are wired.
}

/// Known media file extensions mapped to their MIME types.
const IMAGE_EXTENSIONS: &[(&str, &str)] = &[
    (".png", "image/png"),
    (".jpg", "image/jpeg"),
    (".jpeg", "image/jpeg"),
    (".gif", "image/gif"),
    (".webp", "image/webp"),
];

const AUDIO_EXTENSIONS: &[(&str, &str)] = &[
    (".mp3", "audio/mpeg"),
    (".wav", "audio/wav"),
    (".ogg", "audio/ogg"),
    (".m4a", "audio/m4a"),
];

const VIDEO_EXTENSIONS: &[(&str, &str)] = &[
    (".mp4", "video/mp4"),
    (".webm", "video/webm"),
    (".mov", "video/quicktime"),
];

/// Detect common media URLs in message content and add them as attachments.
fn detect_media_urls(content: &str, attachments: &mut Vec<MediaAttachment>) {
    for word in content.split_whitespace() {
        // Only process HTTP(S) URLs
        if !word.starts_with("http://") && !word.starts_with("https://") {
            continue;
        }

        let lower = word.to_lowercase();

        // Check all known extensions for a matching MIME type
        let mime = IMAGE_EXTENSIONS
            .iter()
            .chain(AUDIO_EXTENSIONS.iter())
            .chain(VIDEO_EXTENSIONS.iter())
            .find(|(ext, _)| lower.ends_with(ext))
            .map(|(_, mime)| *mime);

        if let Some(full_mime) = mime {
            attachments.push(MediaAttachment {
                mime_type: full_mime.to_string(),
                url: Some(word.to_string()),
                transcript: None,
                description: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_media_urls_finds_image_urls() {
        let mut attachments = Vec::new();
        detect_media_urls(
            "Check this out https://example.com/photo.png and https://example.com/pic.jpg",
            &mut attachments,
        );
        assert_eq!(attachments.len(), 2);
        assert_eq!(attachments[0].mime_type, "image/png");
        assert_eq!(
            attachments[0].url.as_deref(),
            Some("https://example.com/photo.png")
        );
        assert_eq!(attachments[1].mime_type, "image/jpeg");
        assert_eq!(
            attachments[1].url.as_deref(),
            Some("https://example.com/pic.jpg")
        );
    }

    #[test]
    fn detect_media_urls_finds_audio_urls() {
        let mut attachments = Vec::new();
        detect_media_urls(
            "Listen to https://example.com/song.mp3 and https://example.com/clip.wav",
            &mut attachments,
        );
        assert_eq!(attachments.len(), 2);
        assert_eq!(attachments[0].mime_type, "audio/mpeg");
        assert_eq!(attachments[1].mime_type, "audio/wav");
    }

    #[test]
    fn detect_media_urls_ignores_non_media() {
        let mut attachments = Vec::new();
        detect_media_urls(
            "Visit https://example.com/page.html and https://example.com/doc.pdf",
            &mut attachments,
        );
        assert!(attachments.is_empty());
    }

    #[test]
    fn detect_media_urls_ignores_non_http() {
        let mut attachments = Vec::new();
        detect_media_urls(
            "Look at file:///tmp/photo.png and /path/to/image.jpg",
            &mut attachments,
        );
        assert!(attachments.is_empty());
    }

    #[tokio::test]
    async fn process_media_with_empty_content() {
        let mut attachments = Vec::new();
        let config = MediaPipelineConfig { enabled: true };
        process_media(&mut attachments, "", &config).await;
        assert!(attachments.is_empty());
    }

    #[test]
    fn media_attachment_default() {
        let attachment = MediaAttachment::default();
        assert!(attachment.mime_type.is_empty());
        assert!(attachment.url.is_none());
        assert!(attachment.transcript.is_none());
        assert!(attachment.description.is_none());
    }
}
