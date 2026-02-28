use regex::Regex;
use std::error::Error;

fn apply_regex(input: String, pattern: &str, replacement: &str) -> String {
    let regex = Regex::new(pattern).expect("redaction regex must compile");
    regex.replace_all(&input, replacement).to_string()
}

pub fn redact_text(input: &str) -> String {
    let mut out = input.to_string();

    out = apply_regex(
        out,
        r#"(?i)\b(OPENAI_API_KEY|TURSO_AUTH_TOKEN)\s*=\s*([^\s,;]+)"#,
        "$1=[REDACTED]",
    );
    out = apply_regex(
        out,
        r#"(?i)"(api[_-]?key|auth[_-]?token)"\s*:\s*"[^"]*""#,
        "\"$1\":\"[REDACTED]\"",
    );
    out = apply_regex(
        out,
        r#"(?i)\b(authorization:\s*bearer)\s+[^\s,;]+"#,
        "$1 [REDACTED]",
    );
    out = apply_regex(out, r#"(?i)\b(bearer)\s+[^\s,;]+"#, "$1 [REDACTED]");
    out = apply_regex(out, r#"\bsk-[A-Za-z0-9_-]{10,}\b"#, "sk-[REDACTED]");

    out
}

pub fn redact_error_chain(err: &(dyn Error + 'static)) -> String {
    let mut parts = vec![redact_text(&err.to_string())];
    let mut source = err.source();

    while let Some(cause) = source {
        parts.push(redact_text(&cause.to_string()));
        source = cause.source();
    }

    parts.join(": ")
}

pub fn install_redacting_panic_hook() {
    std::panic::set_hook(Box::new(|panic_info| {
        let message = if let Some(text) = panic_info.payload().downcast_ref::<&str>() {
            (*text).to_string()
        } else if let Some(text) = panic_info.payload().downcast_ref::<String>() {
            text.clone()
        } else {
            "panic occurred".to_string()
        };

        eprintln!("panic: {}", redact_text(&message));
        if let Some(location) = panic_info.location() {
            eprintln!(
                "at {}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            );
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::{redact_error_chain, redact_text};
    use std::error::Error;
    use std::fmt::{Display, Formatter};

    #[test]
    fn redacts_common_secret_formats() {
        let input = "OPENAI_API_KEY=sk-supersecret12345 Authorization: Bearer token123 {\"auth_token\":\"abc\"}";
        let out = redact_text(input);

        assert!(!out.contains("sk-supersecret12345"));
        assert!(!out.contains("token123"));
        assert!(!out.contains("\"abc\""));
        assert!(out.contains("OPENAI_API_KEY=[REDACTED]"));
    }

    #[test]
    fn leaves_non_secret_text_unchanged() {
        let input = "status ok; model=gpt-4o-mini";
        assert_eq!(redact_text(input), input);
    }

    #[test]
    fn redacts_error_chain_output() {
        #[derive(Debug)]
        struct Root;
        impl Display for Root {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                write!(f, "root OPENAI_API_KEY=sk-exposed012345")
            }
        }
        impl Error for Root {}

        #[derive(Debug)]
        struct Wrapped(Root);
        impl Display for Wrapped {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                write!(f, "wrapper failed")
            }
        }
        impl Error for Wrapped {
            fn source(&self) -> Option<&(dyn Error + 'static)> {
                Some(&self.0)
            }
        }

        let wrapped = Wrapped(Root);
        let text = redact_error_chain(&wrapped);
        assert!(text.contains("OPENAI_API_KEY=[REDACTED]"));
        assert!(!text.contains("sk-exposed012345"));
    }
}
