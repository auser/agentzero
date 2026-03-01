//! Quote-aware shell command tokenizer.
//!
//! Splits a command string into tokens respecting single quotes, double quotes,
//! and backslash escapes. This prevents false-positive security rejections when
//! metacharacters appear inside quoted arguments (e.g., `echo 'hello;world'`).

/// Quoting context for a character in a shell token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteContext {
    Unquoted,
    SingleQuoted,
    DoubleQuoted,
}

/// A character annotated with its quoting context.
#[derive(Debug, Clone, Copy)]
pub struct AnnotatedChar {
    pub ch: char,
    pub context: QuoteContext,
}

/// A parsed shell token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellToken {
    pub text: String,
    pub was_quoted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Unquoted,
    SingleQuoted,
    DoubleQuoted,
}

/// Tokenize a shell command string into tokens, respecting quoting rules.
///
/// Returns `Err` if quotes are unbalanced.
pub fn tokenize(input: &str) -> anyhow::Result<Vec<ShellToken>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut was_quoted = false;
    let mut state = State::Unquoted;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match state {
            State::Unquoted => match ch {
                '\'' => {
                    state = State::SingleQuoted;
                    was_quoted = true;
                }
                '"' => {
                    state = State::DoubleQuoted;
                    was_quoted = true;
                }
                '\\' => {
                    if let Some(next) = chars.next() {
                        current.push(next);
                    }
                }
                c if c.is_ascii_whitespace() => {
                    if !current.is_empty() || was_quoted {
                        tokens.push(ShellToken {
                            text: std::mem::take(&mut current),
                            was_quoted,
                        });
                        was_quoted = false;
                    }
                }
                c => current.push(c),
            },
            State::SingleQuoted => match ch {
                '\'' => state = State::Unquoted,
                c => current.push(c),
            },
            State::DoubleQuoted => match ch {
                '"' => state = State::Unquoted,
                '\\' => {
                    if let Some(&next) = chars.peek() {
                        if matches!(next, '$' | '`' | '"' | '\\' | '\n') {
                            chars.next();
                            current.push(next);
                        } else {
                            current.push('\\');
                        }
                    } else {
                        current.push('\\');
                    }
                }
                c => current.push(c),
            },
        }
    }

    if state != State::Unquoted {
        anyhow::bail!("unbalanced quotes in shell command");
    }

    if !current.is_empty() || was_quoted {
        tokens.push(ShellToken {
            text: current,
            was_quoted,
        });
    }

    Ok(tokens)
}

/// Tokenize and return annotated characters per token for policy evaluation.
///
/// Each token is a `Vec<AnnotatedChar>` preserving the quoting context of every
/// character. This allows the context-aware policy to distinguish between
/// `echo "hello;world"` (semicolon in double quotes) and `echo hello;world`
/// (bare semicolon).
pub fn tokenize_annotated(input: &str) -> anyhow::Result<Vec<Vec<AnnotatedChar>>> {
    let mut tokens: Vec<Vec<AnnotatedChar>> = Vec::new();
    let mut current: Vec<AnnotatedChar> = Vec::new();
    let mut in_token = false;
    let mut state = State::Unquoted;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match state {
            State::Unquoted => match ch {
                '\'' => {
                    state = State::SingleQuoted;
                    in_token = true;
                }
                '"' => {
                    state = State::DoubleQuoted;
                    in_token = true;
                }
                '\\' => {
                    if let Some(next) = chars.next() {
                        current.push(AnnotatedChar {
                            ch: next,
                            context: QuoteContext::Unquoted,
                        });
                        in_token = true;
                    }
                }
                c if c.is_ascii_whitespace() => {
                    if !current.is_empty() || in_token {
                        tokens.push(std::mem::take(&mut current));
                        in_token = false;
                    }
                }
                c => {
                    current.push(AnnotatedChar {
                        ch: c,
                        context: QuoteContext::Unquoted,
                    });
                    in_token = true;
                }
            },
            State::SingleQuoted => match ch {
                '\'' => state = State::Unquoted,
                c => {
                    current.push(AnnotatedChar {
                        ch: c,
                        context: QuoteContext::SingleQuoted,
                    });
                }
            },
            State::DoubleQuoted => match ch {
                '"' => state = State::Unquoted,
                '\\' => {
                    if let Some(&next) = chars.peek() {
                        if matches!(next, '$' | '`' | '"' | '\\' | '\n') {
                            chars.next();
                            current.push(AnnotatedChar {
                                ch: next,
                                context: QuoteContext::DoubleQuoted,
                            });
                        } else {
                            current.push(AnnotatedChar {
                                ch: '\\',
                                context: QuoteContext::DoubleQuoted,
                            });
                        }
                    } else {
                        current.push(AnnotatedChar {
                            ch: '\\',
                            context: QuoteContext::DoubleQuoted,
                        });
                    }
                }
                c => {
                    current.push(AnnotatedChar {
                        ch: c,
                        context: QuoteContext::DoubleQuoted,
                    });
                }
            },
        }
    }

    if state != State::Unquoted {
        anyhow::bail!("unbalanced quotes in shell command");
    }

    if !current.is_empty() || in_token {
        tokens.push(current);
    }

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(tokens: &[ShellToken]) -> Vec<&str> {
        tokens.iter().map(|t| t.text.as_str()).collect()
    }

    #[test]
    fn tokenize_simple_command() {
        let tokens = tokenize("echo hello").unwrap();
        assert_eq!(texts(&tokens), vec!["echo", "hello"]);
        assert!(!tokens[0].was_quoted);
        assert!(!tokens[1].was_quoted);
    }

    #[test]
    fn tokenize_single_quoted() {
        let tokens = tokenize("echo 'hello world'").unwrap();
        assert_eq!(texts(&tokens), vec!["echo", "hello world"]);
        assert!(tokens[1].was_quoted);
    }

    #[test]
    fn tokenize_double_quoted() {
        let tokens = tokenize(r#"echo "hello world""#).unwrap();
        assert_eq!(texts(&tokens), vec!["echo", "hello world"]);
        assert!(tokens[1].was_quoted);
    }

    #[test]
    fn tokenize_semicolon_in_single_quotes() {
        let tokens = tokenize("echo 'hello;world'").unwrap();
        assert_eq!(texts(&tokens), vec!["echo", "hello;world"]);
    }

    #[test]
    fn tokenize_pipe_in_double_quotes() {
        let tokens = tokenize(r#"echo "a|b""#).unwrap();
        assert_eq!(texts(&tokens), vec!["echo", "a|b"]);
    }

    #[test]
    fn tokenize_backslash_escape() {
        let tokens = tokenize(r"echo hello\ world").unwrap();
        assert_eq!(texts(&tokens), vec!["echo", "hello world"]);
    }

    #[test]
    fn tokenize_unbalanced_single_quote_errors() {
        assert!(tokenize("echo 'hello").is_err());
    }

    #[test]
    fn tokenize_unbalanced_double_quote_errors() {
        assert!(tokenize(r#"echo "hello"#).is_err());
    }

    #[test]
    fn tokenize_empty_input() {
        let tokens = tokenize("").unwrap();
        assert!(tokens.is_empty());
    }

    #[test]
    fn tokenize_backslash_in_double_quotes() {
        let tokens = tokenize(r#"echo "a\"b""#).unwrap();
        assert_eq!(texts(&tokens), vec!["echo", r#"a"b"#]);
    }

    #[test]
    fn tokenize_adjacent_quotes() {
        let tokens = tokenize(r#"echo 'a'"b""#).unwrap();
        assert_eq!(texts(&tokens), vec!["echo", "ab"]);
    }

    #[test]
    fn annotated_preserves_context() {
        let tokens = tokenize_annotated("echo 'a;b'").unwrap();
        assert_eq!(tokens.len(), 2);
        // "echo" is all Unquoted
        assert!(tokens[0]
            .iter()
            .all(|c| c.context == QuoteContext::Unquoted));
        // "a;b" is all SingleQuoted
        assert!(tokens[1]
            .iter()
            .all(|c| c.context == QuoteContext::SingleQuoted));
        assert_eq!(tokens[1][1].ch, ';');
    }

    #[test]
    fn annotated_mixed_context() {
        let tokens = tokenize_annotated(r#"echo hello";"world"#).unwrap();
        assert_eq!(tokens.len(), 2);
        // The second token is: hello (Unquoted) + ; (DoubleQuoted) + world (Unquoted)
        let second = &tokens[1];
        assert_eq!(second[0].ch, 'h');
        assert_eq!(second[0].context, QuoteContext::Unquoted);
        assert_eq!(second[5].ch, ';');
        assert_eq!(second[5].context, QuoteContext::DoubleQuoted);
        assert_eq!(second[6].ch, 'w');
        assert_eq!(second[6].context, QuoteContext::Unquoted);
    }
}
