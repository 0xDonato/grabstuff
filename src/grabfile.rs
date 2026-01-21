//! Grabfile script parsing and execution.
//!
//! Grabfiles (`.grab`) are scripts that define patterns and slicing options
//! for batch file content extraction.

use crate::config::Config;
use crate::file_matcher::FileMatcher;
use crate::formatter::{Format, Formatter};
use crate::output::{write_output, OutputDestination};
use crate::slicer::{FileContent, SliceMode, Slicer};
use anyhow::{Context, Result};
use rayon::prelude::*;
use std::path::PathBuf;

/// Parsed grabfile containing patterns and global options.
///
/// # Grabfile Format
///
/// ```text
/// # Comment
/// --format json
/// --output output.json
///
/// src/*.rs --head 100 lines
/// docs/ --full
/// README.md --tail 20 lines
/// ```
#[derive(Debug)]
pub struct GrabFile {
    entries: Vec<GrabEntry>,
    global_options: GlobalOptions,
}

#[derive(Debug)]
struct GrabEntry {
    pattern: String,
    slice_mode: SliceMode,
}

#[derive(Debug)]
struct GlobalOptions {
    format: Format,
    output: Option<PathBuf>,
    copy: bool,
    ignores: Vec<String>,
}

impl Default for GlobalOptions {
    fn default() -> Self {
        GlobalOptions {
            format: Format::Markdown,
            output: None,
            copy: false,
            ignores: Vec::new(),
        }
    }
}

impl GrabFile {
    /// Parses a grabfile from the given path.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the `.grab` file
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or contains invalid syntax.
    pub fn parse(path: &PathBuf) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read grabfile: {}", path.display()))?;

        let mut entries = Vec::new();
        let mut global_options = GlobalOptions::default();

        for (line_num, line) in content.lines().enumerate() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Skip shebang
            if line.starts_with("#!") {
                continue;
            }

            // Parse line
            if line.starts_with("--") {
                // Global option
                parse_global_option(line, &mut global_options)
                    .with_context(|| format!("Invalid syntax at line {}", line_num + 1))?;
            } else {
                // Pattern with optional options
                let entry = parse_entry(line)
                    .with_context(|| format!("Invalid syntax at line {}", line_num + 1))?;
                entries.push(entry);
            }
        }

        Ok(GrabFile {
            entries,
            global_options,
        })
    }

    /// Executes the grabfile, processing all matched files.
    ///
    /// Files are processed in parallel using Rayon for improved performance.
    ///
    /// # Arguments
    ///
    /// * `config` - Application configuration with default ignore patterns
    ///
    /// # Errors
    ///
    /// Returns an error if no files match or if file processing fails.
    pub fn execute(&self, config: &Config) -> Result<()> {
        // Combine ignores
        let mut ignores = config.defaults.ignore.clone();
        ignores.extend(self.global_options.ignores.clone());

        let matcher = FileMatcher::new(&ignores);

        // Collect all files with their slice modes
        let mut file_tasks: Vec<(PathBuf, SliceMode)> = Vec::new();

        for entry in &self.entries {
            let files = matcher.match_patterns(std::slice::from_ref(&entry.pattern))?;
            for file in files {
                file_tasks.push((file, entry.slice_mode.clone()));
            }
        }

        if file_tasks.is_empty() {
            anyhow::bail!("No files matched any patterns in the grabfile");
        }

        // Process files in parallel
        let results: Vec<Result<FileContent>> = file_tasks
            .par_iter()
            .map(|(path, slice_mode)| {
                let slicer = Slicer::new(slice_mode.clone());
                slicer.process_file(path)
            })
            .collect();

        // Collect results
        let mut all_contents = Vec::with_capacity(results.len());
        for result in results {
            all_contents.push(result?);
        }

        // Sort by path for consistent output
        all_contents.sort_by(|a, b| a.path.cmp(&b.path));

        // Format output
        let formatter = Formatter::new(self.global_options.format.clone());
        let output = formatter.format(&all_contents)?;

        // Determine destination
        let destination = if self.global_options.copy {
            OutputDestination::Clipboard
        } else if let Some(ref path) = self.global_options.output {
            OutputDestination::File(path.clone())
        } else {
            OutputDestination::Stdout
        };

        write_output(&output, destination)?;

        Ok(())
    }
}

fn parse_global_option(line: &str, options: &mut GlobalOptions) -> Result<()> {
    let parts: Vec<&str> = line.split_whitespace().collect();

    match parts.first().copied() {
        Some("--format") => {
            let format = parts
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("Invalid syntax: --format requires a value"))?;
            options.format = match *format {
                "md" | "markdown" => Format::Markdown,
                "plain" | "text" => Format::Plain,
                "json" => Format::Json,
                _ => anyhow::bail!("Invalid syntax: unknown format '{}'", format),
            };
        }
        Some("--output") => {
            let path = parts
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("Invalid syntax: --output requires a path"))?;
            options.output = Some(PathBuf::from(path));
        }
        Some("--copy") => {
            options.copy = true;
        }
        Some("--ignore") => {
            let pattern = parts
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("Invalid syntax: --ignore requires a pattern"))?;
            options.ignores.push(pattern.to_string());
        }
        _ => anyhow::bail!("Invalid syntax: unknown option '{}'", line),
    }

    Ok(())
}

fn parse_entry(line: &str) -> Result<GrabEntry> {
    let parts: Vec<&str> = line.split_whitespace().collect();

    if parts.is_empty() {
        anyhow::bail!("Invalid syntax: empty entry");
    }

    let pattern = parts[0].to_string();
    let mut slice_mode = SliceMode::Full;

    // Parse options
    let mut i = 1;
    while i < parts.len() {
        match parts[i] {
            "--head" => {
                let n: usize = parts
                    .get(i + 1)
                    .ok_or_else(|| anyhow::anyhow!("Invalid syntax: --head requires N"))?
                    .parse()
                    .context("Invalid syntax: N must be a number")?;
                let unit = parts
                    .get(i + 2)
                    .ok_or_else(|| anyhow::anyhow!("Invalid syntax: --head requires unit"))?;
                slice_mode = match *unit {
                    "lines" | "line" => SliceMode::HeadLines(n),
                    "chars" | "char" | "characters" => SliceMode::HeadChars(n),
                    _ => anyhow::bail!("Invalid syntax: unit must be 'lines' or 'chars'"),
                };
                i += 3;
            }
            "--tail" => {
                let n: usize = parts
                    .get(i + 1)
                    .ok_or_else(|| anyhow::anyhow!("Invalid syntax: --tail requires N"))?
                    .parse()
                    .context("Invalid syntax: N must be a number")?;
                let unit = parts
                    .get(i + 2)
                    .ok_or_else(|| anyhow::anyhow!("Invalid syntax: --tail requires unit"))?;
                slice_mode = match *unit {
                    "lines" | "line" => SliceMode::TailLines(n),
                    "chars" | "char" | "characters" => SliceMode::TailChars(n),
                    _ => anyhow::bail!("Invalid syntax: unit must be 'lines' or 'chars'"),
                };
                i += 3;
            }
            "--full" => {
                slice_mode = SliceMode::Full;
                i += 1;
            }
            _ => {
                anyhow::bail!("Invalid syntax: unknown option '{}'", parts[i]);
            }
        }
    }

    Ok(GrabEntry {
        pattern,
        slice_mode,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_grabfile(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::with_suffix(".grab").unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file.flush().unwrap();
        file
    }

    #[test]
    fn test_parse_simple_grabfile() {
        let content = "src/*.rs\n*.md";
        let file = create_temp_grabfile(content);
        let path = file.path().to_path_buf();

        let grabfile = GrabFile::parse(&path).unwrap();
        assert_eq!(grabfile.entries.len(), 2);
        assert_eq!(grabfile.entries[0].pattern, "src/*.rs");
        assert_eq!(grabfile.entries[1].pattern, "*.md");
    }

    #[test]
    fn test_parse_with_comments() {
        let content = "# This is a comment\nsrc/*.rs\n# Another comment\n*.md";
        let file = create_temp_grabfile(content);
        let path = file.path().to_path_buf();

        let grabfile = GrabFile::parse(&path).unwrap();
        assert_eq!(grabfile.entries.len(), 2);
    }

    #[test]
    fn test_parse_with_shebang() {
        let content = "#!/usr/bin/env grabstuff\nsrc/*.rs";
        let file = create_temp_grabfile(content);
        let path = file.path().to_path_buf();

        let grabfile = GrabFile::parse(&path).unwrap();
        assert_eq!(grabfile.entries.len(), 1);
    }

    #[test]
    fn test_parse_global_format_option() {
        let content = "--format json\nsrc/*.rs";
        let file = create_temp_grabfile(content);
        let path = file.path().to_path_buf();

        let grabfile = GrabFile::parse(&path).unwrap();
        assert!(matches!(grabfile.global_options.format, Format::Json));
    }

    #[test]
    fn test_parse_global_output_option() {
        let content = "--output result.md\nsrc/*.rs";
        let file = create_temp_grabfile(content);
        let path = file.path().to_path_buf();

        let grabfile = GrabFile::parse(&path).unwrap();
        assert_eq!(
            grabfile.global_options.output,
            Some(PathBuf::from("result.md"))
        );
    }

    #[test]
    fn test_parse_global_copy_option() {
        let content = "--copy\nsrc/*.rs";
        let file = create_temp_grabfile(content);
        let path = file.path().to_path_buf();

        let grabfile = GrabFile::parse(&path).unwrap();
        assert!(grabfile.global_options.copy);
    }

    #[test]
    fn test_parse_entry_with_head_lines() {
        let content = "src/*.rs --head 50 lines";
        let file = create_temp_grabfile(content);
        let path = file.path().to_path_buf();

        let grabfile = GrabFile::parse(&path).unwrap();
        assert!(matches!(
            grabfile.entries[0].slice_mode,
            SliceMode::HeadLines(50)
        ));
    }

    #[test]
    fn test_parse_entry_with_tail_chars() {
        let content = "src/*.rs --tail 100 chars";
        let file = create_temp_grabfile(content);
        let path = file.path().to_path_buf();

        let grabfile = GrabFile::parse(&path).unwrap();
        assert!(matches!(
            grabfile.entries[0].slice_mode,
            SliceMode::TailChars(100)
        ));
    }

    #[test]
    fn test_parse_entry_with_full() {
        let content = "src/*.rs --full";
        let file = create_temp_grabfile(content);
        let path = file.path().to_path_buf();

        let grabfile = GrabFile::parse(&path).unwrap();
        assert!(matches!(grabfile.entries[0].slice_mode, SliceMode::Full));
    }

    #[test]
    fn test_parse_empty_lines_ignored() {
        let content = "\n\nsrc/*.rs\n\n*.md\n\n";
        let file = create_temp_grabfile(content);
        let path = file.path().to_path_buf();

        let grabfile = GrabFile::parse(&path).unwrap();
        assert_eq!(grabfile.entries.len(), 2);
    }

    #[test]
    fn test_parse_invalid_option_error() {
        let content = "--invalid-option value\nsrc/*.rs";
        let file = create_temp_grabfile(content);
        let path = file.path().to_path_buf();

        let result = GrabFile::parse(&path);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("unknown option") || err_msg.contains("Invalid syntax"),
            "Expected error about unknown option, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_parse_missing_format_value_error() {
        let content = "--format\nsrc/*.rs";
        let file = create_temp_grabfile(content);
        let path = file.path().to_path_buf();

        let result = GrabFile::parse(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_global_ignore_option() {
        let content = "--ignore *.log\n--ignore temp/\nsrc/*.rs";
        let file = create_temp_grabfile(content);
        let path = file.path().to_path_buf();

        let grabfile = GrabFile::parse(&path).unwrap();
        assert_eq!(grabfile.global_options.ignores.len(), 2);
        assert!(grabfile
            .global_options
            .ignores
            .contains(&"*.log".to_string()));
        assert!(grabfile
            .global_options
            .ignores
            .contains(&"temp/".to_string()));
    }

    #[test]
    fn test_default_global_options() {
        let options = GlobalOptions::default();
        assert!(matches!(options.format, Format::Markdown));
        assert!(options.output.is_none());
        assert!(!options.copy);
        assert!(options.ignores.is_empty());
    }
}
