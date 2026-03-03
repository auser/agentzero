use anyhow::Context;
use std::io::Write;

const PAD: &str = "  ";

#[cfg(feature = "interactive")]
use console::style;

pub fn print_brand_header(writer: &mut dyn Write) -> anyhow::Result<()> {
    let logo = [
        "      _                    _                      ",
        "     / \\   __ _  ___ _ __ | |_ _______ _ __ ___  ",
        "    / _ \\ / _` |/ _ \\ '_ \\| __|_  / _ \\ '__/ _ \\ ",
        "   / ___ \\ (_| |  __/ | | | |_ / /  __/ | | (_) |",
        "  /_/   \\_\\__, |\\___|_| |_|\\__/___\\___|_|  \\___/ ",
        "          |___/                                   ",
    ];

    #[cfg(feature = "interactive")]
    {
        let accent_line = "~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~";
        let accent = style(accent_line).color256(51);
        writeln!(writer, "\n{PAD}{}", accent).context("failed to write output")?;
        for line in logo {
            writeln!(writer, "{PAD}{}", style(line).bold().color256(153))
                .context("failed to write output")?;
        }
        writeln!(
            writer,
            "\n\n{PAD}{}",
            style("Fast. Secure. Safe. 100% Rust. 100% Agnostic.").color256(151)
        )
        .context("failed to write output")?;
        writeln!(writer, "{PAD}{}", accent).context("failed to write output")?;
    }
    #[cfg(not(feature = "interactive"))]
    {
        for line in logo {
            writeln!(writer, "{PAD}{line}").context("failed to write output")?;
        }
        writeln!(
            writer,
            "\n{PAD}Fast. Secure. Safe. 100% Rust. 100% Agnostic."
        )
        .context("failed to write output")?;
    }
    writeln!(writer).context("failed to write output")?;
    Ok(())
}

pub fn print_intro(writer: &mut dyn Write, message: &str) -> anyhow::Result<()> {
    if message.trim().is_empty() {
        anyhow::bail!("intro message cannot be empty");
    }

    #[cfg(feature = "interactive")]
    writeln!(writer, "{PAD}{}", style(message).bold()).context("failed to write output")?;
    #[cfg(not(feature = "interactive"))]
    writeln!(writer, "{PAD}{message}").context("failed to write output")?;
    Ok(())
}

pub fn print_section(writer: &mut dyn Write, title: &str) -> anyhow::Result<()> {
    if title.trim().is_empty() {
        anyhow::bail!("section title cannot be empty");
    }

    #[cfg(feature = "interactive")]
    writeln!(writer, "\n{PAD}{}", style(title).bold().underlined())
        .context("failed to write output")?;
    #[cfg(not(feature = "interactive"))]
    writeln!(writer, "\n{PAD}{title}").context("failed to write output")?;
    Ok(())
}

pub fn print_success_line(writer: &mut dyn Write, message: &str) -> anyhow::Result<()> {
    if message.trim().is_empty() {
        anyhow::bail!("summary message cannot be empty");
    }

    #[cfg(feature = "interactive")]
    writeln!(
        writer,
        "{PAD}{} {}",
        style("✓").green().bold(),
        style(message).white()
    )
    .context("failed to write output")?;
    #[cfg(not(feature = "interactive"))]
    writeln!(writer, "{PAD}* {message}").context("failed to write output")?;
    Ok(())
}

pub fn cyan_value(value: impl std::fmt::Display) -> String {
    #[cfg(feature = "interactive")]
    {
        style(value).cyan().to_string()
    }
    #[cfg(not(feature = "interactive"))]
    {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{print_intro, print_section, print_success_line};

    #[test]
    fn prints_section_and_success_line() {
        let mut output = Vec::new();
        print_section(&mut output, "Provider Setup").expect("section should render");
        print_success_line(&mut output, "Config generated successfully")
            .expect("success line should render");

        let rendered = String::from_utf8(output).expect("output should be utf8");
        assert!(rendered.contains("Provider Setup"));
        assert!(rendered.contains("Config generated successfully"));
    }

    #[test]
    fn rejects_empty_intro_message() {
        let mut output = Vec::new();
        let err = print_intro(&mut output, "   ").expect_err("empty intro should fail");
        assert!(err.to_string().contains("intro message cannot be empty"));
    }
}
