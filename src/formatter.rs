//! Output formatting for file contents.
//!
//! Supports multiple output formats: Markdown, plain text, and JSON.

use crate::config::DefaultFormat;
use crate::slicer::FileContent;
use anyhow::Result;
use serde::Serialize;

/// Output format for compiled file contents.
#[derive(Clone, Debug, PartialEq)]
pub enum Format {
    /// Markdown format with headers and separators.
    Markdown,
    /// Plain text with just the content.
    Plain,
    /// Structured JSON output.
    Json,
}

/// Formats file contents into the specified output format.
pub struct Formatter {
    format: Format,
    include_tokens: bool,
}

#[derive(Serialize)]
struct JsonOutput {
    files: Vec<JsonFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tokens: Option<usize>,
}

#[derive(Serialize)]
struct JsonFile {
    path: String,
    slice: String,
    content: String,
}

impl Formatter {
    /// Creates a new `Formatter` with the specified output format.
    pub fn new(format: Format) -> Self {
        Formatter {
            format,
            include_tokens: false,
        }
    }

    /// Enables or disables token count output.
    pub fn with_tokens(mut self, include_tokens: bool) -> Self {
        self.include_tokens = include_tokens;
        self
    }

    /// Formats file contents into a single output string.
    ///
    /// # Arguments
    ///
    /// * `files` - Slice of file contents to format
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails (for JSON format).
    pub fn format(&self, files: &[FileContent]) -> Result<String> {
        match &self.format {
            Format::Markdown => self.format_markdown(files),
            Format::Plain => self.format_plain(files),
            Format::Json => self.format_json(files),
        }
    }

    fn format_markdown(&self, files: &[FileContent]) -> Result<String> {
        let mut output = String::new();

        for (i, file) in files.iter().enumerate() {
            if i > 0 {
                output.push_str("\n---\n\n");
            }

            output.push_str(&format!(
                "# {} ({})\n\n",
                file.path.display(),
                file.slice_info.description()
            ));
            output.push_str(&file.content);
            output.push('\n');
        }

        if self.include_tokens {
            if !output.is_empty() {
                output.push_str("\n---\n\n");
            }
            output.push_str(&format!(
                "# Tokens: {}\n",
                format_number(count_tokens(files))
            ));
        }

        Ok(output)
    }

    fn format_plain(&self, files: &[FileContent]) -> Result<String> {
        let mut output = String::new();

        for file in files {
            output.push_str(&file.content);
            output.push('\n');
        }

        if self.include_tokens {
            output.push_str(&format!("Tokens: {}\n", format_number(count_tokens(files))));
        }

        Ok(output)
    }

    fn format_json(&self, files: &[FileContent]) -> Result<String> {
        let json_files: Vec<JsonFile> = files
            .iter()
            .map(|f| JsonFile {
                path: f.path.display().to_string(),
                slice: f.slice_info.description(),
                content: f.content.clone(),
            })
            .collect();

        let output = JsonOutput {
            files: json_files,
            tokens: self.include_tokens.then(|| count_tokens(files)),
        };

        Ok(serde_json::to_string_pretty(&output)?)
    }
}

impl From<DefaultFormat> for Format {
    fn from(format: DefaultFormat) -> Self {
        match format {
            DefaultFormat::Md => Format::Markdown,
            DefaultFormat::Plain => Format::Plain,
            DefaultFormat::Json => Format::Json,
        }
    }
}

fn count_tokens(files: &[FileContent]) -> usize {
    files
        .iter()
        .map(|file| count_text_tokens(&file.content))
        .sum()
}

fn count_text_tokens(text: &str) -> usize {
    let mut count = 0;
    let mut in_word = false;

    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            if !in_word {
                count += 1;
                in_word = true;
            }
        } else {
            in_word = false;
            if !ch.is_whitespace() {
                count += 1;
            }
        }
    }

    count
}

fn format_number(n: usize) -> String {
    let digits = n.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);

    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            formatted.push(',');
        }
        formatted.push(ch);
    }

    formatted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slicer::SliceInfo;
    use std::path::PathBuf;

    fn create_test_content(path: &str, content: &str, slice_info: SliceInfo) -> FileContent {
        FileContent {
            path: PathBuf::from(path),
            content: content.to_string(),
            slice_info,
        }
    }

    #[test]
    fn test_format_markdown_single_file() {
        let files = vec![create_test_content(
            "test.rs",
            "fn main() {}",
            SliceInfo::Full,
        )];

        let formatter = Formatter::new(Format::Markdown);
        let output = formatter.format(&files).unwrap();

        assert!(output.contains("# test.rs (full)"));
        assert!(output.contains("fn main() {}"));
    }

    #[test]
    fn test_format_markdown_multiple_files() {
        let files = vec![
            create_test_content("a.rs", "content a", SliceInfo::Full),
            create_test_content("b.rs", "content b", SliceInfo::Lines { start: 1, end: 10 }),
        ];

        let formatter = Formatter::new(Format::Markdown);
        let output = formatter.format(&files).unwrap();

        assert!(output.contains("# a.rs (full)"));
        assert!(output.contains("# b.rs (lines 1-10)"));
        assert!(output.contains("---")); // separator between files
    }

    #[test]
    fn test_format_plain() {
        let files = vec![
            create_test_content("a.rs", "content a", SliceInfo::Full),
            create_test_content("b.rs", "content b", SliceInfo::Full),
        ];

        let formatter = Formatter::new(Format::Plain);
        let output = formatter.format(&files).unwrap();

        assert_eq!(output, "content a\ncontent b\n");
        assert!(!output.contains("a.rs")); // no file names in plain
    }

    #[test]
    fn test_format_json() {
        let files = vec![create_test_content(
            "test.rs",
            "fn main() {}",
            SliceInfo::Lines { start: 1, end: 5 },
        )];

        let formatter = Formatter::new(Format::Json);
        let output = formatter.format(&files).unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed["files"].is_array());
        assert_eq!(parsed["files"][0]["path"], "test.rs");
        assert_eq!(parsed["files"][0]["slice"], "lines 1-5");
        assert_eq!(parsed["files"][0]["content"], "fn main() {}");
    }

    #[test]
    fn test_format_markdown_with_tokens() {
        let files = vec![create_test_content(
            "test.rs",
            "fn main() {}",
            SliceInfo::Full,
        )];

        let formatter = Formatter::new(Format::Markdown).with_tokens(true);
        let output = formatter.format(&files).unwrap();

        assert!(output.contains("# Tokens: 6"));
    }

    #[test]
    fn test_format_json_with_tokens() {
        let files = vec![create_test_content(
            "test.rs",
            "hello world",
            SliceInfo::Full,
        )];

        let formatter = Formatter::new(Format::Json).with_tokens(true);
        let output = formatter.format(&files).unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["tokens"], 2);
    }

    #[test]
    fn test_format_empty_files() {
        let files: Vec<FileContent> = vec![];

        let formatter = Formatter::new(Format::Markdown);
        let output = formatter.format(&files).unwrap();

        assert_eq!(output, "");
    }
}
