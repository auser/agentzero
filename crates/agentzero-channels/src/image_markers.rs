/// Parse `[IMAGE:<source>]` markers from user messages.
///
/// Supported source formats:
/// - `[IMAGE:/path/to/file.png]` — local file path
/// - `[IMAGE:data:image/png;base64,...]` — data URI
/// - `[IMAGE:https://example.com/image.png]` — remote URL (requires allow_remote_fetch)
///
/// A parsed image reference extracted from a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRef {
    /// The source string (file path, data URI, or URL).
    pub source: String,
    /// What kind of source this is.
    pub kind: ImageSourceKind,
    /// Byte offset in the original message where the marker starts.
    pub start: usize,
    /// Byte offset in the original message where the marker ends.
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageSourceKind {
    /// Local file path.
    LocalFile,
    /// Data URI (e.g., data:image/png;base64,...).
    DataUri,
    /// Remote URL (http/https).
    RemoteUrl,
}

/// Parse all `[IMAGE:<source>]` markers from a message.
pub fn parse_image_markers(text: &str) -> Vec<ImageRef> {
    let mut refs = Vec::new();
    let marker_prefix = "[IMAGE:";
    let mut search_from = 0;

    while search_from < text.len() {
        let start = match text[search_from..].find(marker_prefix) {
            Some(pos) => search_from + pos,
            None => break,
        };

        let content_start = start + marker_prefix.len();
        let end = match text[content_start..].find(']') {
            Some(pos) => content_start + pos + 1,
            None => break, // Unclosed marker, skip
        };

        let source = text[content_start..end - 1].trim().to_string();
        if !source.is_empty() {
            let kind = classify_source(&source);
            refs.push(ImageRef {
                source,
                kind,
                start,
                end,
            });
        }

        search_from = end;
    }

    refs
}

/// Remove image markers from a message, returning just the text content.
pub fn strip_image_markers(text: &str) -> String {
    let refs = parse_image_markers(text);
    if refs.is_empty() {
        return text.to_string();
    }

    let mut result = String::with_capacity(text.len());
    let mut last_end = 0;

    for r in &refs {
        result.push_str(&text[last_end..r.start]);
        last_end = r.end;
    }
    result.push_str(&text[last_end..]);

    // Clean up extra whitespace left by removed markers
    let cleaned: Vec<&str> = result.split_whitespace().collect();
    cleaned.join(" ")
}

fn classify_source(source: &str) -> ImageSourceKind {
    if source.starts_with("data:") {
        ImageSourceKind::DataUri
    } else if source.starts_with("http://") || source.starts_with("https://") {
        ImageSourceKind::RemoteUrl
    } else {
        ImageSourceKind::LocalFile
    }
}

/// Validate image references against policy constraints.
pub fn validate_image_refs(
    refs: &[ImageRef],
    max_images: usize,
    allow_remote_fetch: bool,
) -> Result<(), String> {
    if refs.len() > max_images {
        return Err(format!(
            "Too many images: {} (max {})",
            refs.len(),
            max_images
        ));
    }

    for r in refs {
        if r.kind == ImageSourceKind::RemoteUrl && !allow_remote_fetch {
            return Err(format!("Remote image fetch is disabled: {}", r.source));
        }
    }

    Ok(())
}

/// Check whether the provider supports vision before sending images.
///
/// - `vision_support = Some(true)` → allowed
/// - `vision_support = Some(false)` → error: provider explicitly does not support vision
/// - `vision_support = None` → allowed (assume provider handles it; no explicit config)
pub fn check_vision_support(
    image_refs: &[ImageRef],
    vision_support: Option<bool>,
) -> Result<(), String> {
    if image_refs.is_empty() {
        return Ok(());
    }

    match vision_support {
        Some(false) => Err(format!(
            "Provider does not support vision, but message contains {} image(s). \
             Set model_support_vision = true in config or remove images.",
            image_refs.len()
        )),
        _ => Ok(()),
    }
}

/// Load image data from parsed image references, producing ContentParts.
///
/// - LocalFile: read file, base64-encode, infer media_type from extension.
/// - DataUri: parse inline data URI.
/// - RemoteUrl: skipped (caller should use validate_image_refs to reject if needed).
pub async fn load_image_refs(
    refs: &[ImageRef],
) -> anyhow::Result<Vec<agentzero_core::ContentPart>> {
    use agentzero_core::ContentPart;
    use base64::Engine;

    let mut parts = Vec::new();
    for r in refs {
        match r.kind {
            ImageSourceKind::LocalFile => {
                let data = tokio::fs::read(&r.source).await.map_err(|e| {
                    anyhow::anyhow!("failed to read image file '{}': {e}", r.source)
                })?;
                let media_type = mime_from_extension(&r.source);
                let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
                parts.push(ContentPart::Image {
                    media_type,
                    data: encoded,
                });
            }
            ImageSourceKind::DataUri => {
                // data:image/png;base64,...
                if let Some((header, b64)) = r.source.split_once(',') {
                    let media_type = header
                        .strip_prefix("data:")
                        .and_then(|s| s.strip_suffix(";base64"))
                        .unwrap_or("image/png")
                        .to_string();
                    parts.push(ContentPart::Image {
                        media_type,
                        data: b64.to_string(),
                    });
                }
            }
            ImageSourceKind::RemoteUrl => {
                // Remote URLs are not loaded automatically.
                // Caller should validate/reject or handle externally.
            }
        }
    }
    Ok(parts)
}

fn mime_from_extension(path: &str) -> String {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// Build a ConversationMessage::User from text that may contain image markers.
///
/// If vision is enabled and markers are present, produces a multi-modal message.
/// If vision is disabled, strips markers and produces a text-only message.
pub async fn build_user_message(
    text: &str,
    vision_enabled: bool,
) -> anyhow::Result<agentzero_core::ConversationMessage> {
    let refs = parse_image_markers(text);
    if refs.is_empty() || !vision_enabled {
        let clean_text = if refs.is_empty() {
            text.to_string()
        } else {
            strip_image_markers(text)
        };
        return Ok(agentzero_core::ConversationMessage::user(clean_text));
    }

    let stripped = strip_image_markers(text);
    let parts = load_image_refs(&refs).await?;
    Ok(agentzero_core::ConversationMessage::user_with_parts(
        stripped, parts,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_local_file() {
        let refs = parse_image_markers("Look at this [IMAGE:/tmp/screenshot.png] image");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].source, "/tmp/screenshot.png");
        assert_eq!(refs[0].kind, ImageSourceKind::LocalFile);
    }

    #[test]
    fn parse_data_uri() {
        let refs = parse_image_markers("[IMAGE:data:image/png;base64,iVBOR...]");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].kind, ImageSourceKind::DataUri);
    }

    #[test]
    fn parse_remote_url() {
        let refs = parse_image_markers("[IMAGE:https://example.com/photo.jpg]");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].source, "https://example.com/photo.jpg");
        assert_eq!(refs[0].kind, ImageSourceKind::RemoteUrl);
    }

    #[test]
    fn parse_multiple_markers() {
        let text = "Compare [IMAGE:/a.png] with [IMAGE:/b.png] and [IMAGE:https://c.jpg]";
        let refs = parse_image_markers(text);
        assert_eq!(refs.len(), 3);
        assert_eq!(refs[0].kind, ImageSourceKind::LocalFile);
        assert_eq!(refs[1].kind, ImageSourceKind::LocalFile);
        assert_eq!(refs[2].kind, ImageSourceKind::RemoteUrl);
    }

    #[test]
    fn parse_no_markers() {
        let refs = parse_image_markers("Just a normal message");
        assert!(refs.is_empty());
    }

    #[test]
    fn parse_unclosed_marker_skipped() {
        let refs = parse_image_markers("[IMAGE:/broken");
        assert!(refs.is_empty());
    }

    #[test]
    fn parse_empty_marker_skipped() {
        let refs = parse_image_markers("[IMAGE:]");
        assert!(refs.is_empty());
    }

    #[test]
    fn strip_markers_removes_images() {
        let text = "Look at [IMAGE:/tmp/a.png] this image";
        let stripped = strip_image_markers(text);
        assert_eq!(stripped, "Look at this image");
    }

    #[test]
    fn strip_no_markers_unchanged() {
        let text = "hello world";
        assert_eq!(strip_image_markers(text), "hello world");
    }

    #[test]
    fn validate_too_many_images() {
        let refs = vec![
            ImageRef {
                source: "/a.png".into(),
                kind: ImageSourceKind::LocalFile,
                start: 0,
                end: 10,
            },
            ImageRef {
                source: "/b.png".into(),
                kind: ImageSourceKind::LocalFile,
                start: 20,
                end: 30,
            },
        ];
        assert!(validate_image_refs(&refs, 1, true).is_err());
        assert!(validate_image_refs(&refs, 2, true).is_ok());
    }

    #[test]
    fn validate_remote_fetch_disabled() {
        let refs = vec![ImageRef {
            source: "https://example.com/a.png".into(),
            kind: ImageSourceKind::RemoteUrl,
            start: 0,
            end: 30,
        }];
        assert!(validate_image_refs(&refs, 4, false).is_err());
        assert!(validate_image_refs(&refs, 4, true).is_ok());
    }

    #[test]
    fn validate_local_files_always_ok() {
        let refs = vec![ImageRef {
            source: "/tmp/a.png".into(),
            kind: ImageSourceKind::LocalFile,
            start: 0,
            end: 10,
        }];
        assert!(validate_image_refs(&refs, 4, false).is_ok());
    }

    fn sample_image_ref() -> ImageRef {
        ImageRef {
            source: "/tmp/a.png".into(),
            kind: ImageSourceKind::LocalFile,
            start: 0,
            end: 10,
        }
    }

    #[test]
    fn vision_check_no_images_always_ok() {
        assert!(check_vision_support(&[], Some(false)).is_ok());
        assert!(check_vision_support(&[], Some(true)).is_ok());
        assert!(check_vision_support(&[], None).is_ok());
    }

    #[test]
    fn vision_check_explicit_false_rejects_images() {
        let refs = vec![sample_image_ref()];
        let err = check_vision_support(&refs, Some(false)).unwrap_err();
        assert!(err.contains("does not support vision"));
        assert!(err.contains("1 image(s)"));
    }

    #[test]
    fn vision_check_explicit_true_allows_images() {
        let refs = vec![sample_image_ref()];
        assert!(check_vision_support(&refs, Some(true)).is_ok());
    }

    #[test]
    fn vision_check_none_allows_images() {
        let refs = vec![sample_image_ref()];
        assert!(check_vision_support(&refs, None).is_ok());
    }
}
