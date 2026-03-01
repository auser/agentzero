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
}
