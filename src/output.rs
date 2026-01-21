//! Output destination handling.
//!
//! Supports writing to stdout, files, and the system clipboard.

use anyhow::{Context, Result};
use std::path::PathBuf;

/// Destination for formatted output.
#[derive(Debug)]
pub enum OutputDestination {
    /// Write to standard output.
    Stdout,
    /// Write to a file at the specified path.
    File(PathBuf),
    /// Copy to the system clipboard.
    Clipboard,
}

/// Maximum output size (100MB).
const MAX_OUTPUT_SIZE: usize = 100 * 1024 * 1024;

/// Writes content to the specified destination.
///
/// # Arguments
///
/// * `content` - The content to write
/// * `destination` - Where to write the content
///
/// # Errors
///
/// Returns an error if:
/// - Content exceeds 100MB
/// - File cannot be written
/// - Clipboard cannot be accessed
pub fn write_output(content: &str, destination: OutputDestination) -> Result<()> {
    // Check output size limit
    if content.len() > MAX_OUTPUT_SIZE {
        anyhow::bail!(
            "Output exceeds maximum size of 100MB ({} bytes)",
            content.len()
        );
    }

    match destination {
        OutputDestination::Stdout => {
            print!("{}", content);
            Ok(())
        }
        OutputDestination::File(path) => {
            std::fs::write(&path, content)
                .with_context(|| format!("Failed to write to file: {}", path.display()))?;
            eprintln!("Output written to {}", path.display());
            Ok(())
        }
        OutputDestination::Clipboard => {
            let mut clipboard = arboard::Clipboard::new().context("Failed to access clipboard")?;
            clipboard
                .set_text(content)
                .context("Failed to copy to clipboard")?;
            eprintln!("Output copied to clipboard ({} chars)", content.len());
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_write_to_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("output.txt");

        let content = "test content";
        let result = write_output(content, OutputDestination::File(file_path.clone()));

        assert!(result.is_ok());
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), content);
    }

    #[test]
    fn test_output_size_limit() {
        let large_content = "x".repeat(101 * 1024 * 1024); // 101MB
        let result = write_output(&large_content, OutputDestination::Stdout);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("100MB"));
    }

    #[test]
    fn test_output_within_limit() {
        let content = "x".repeat(1024); // 1KB
                                        // Just verify it doesn't error on size check
                                        // (Stdout writing would go to actual stdout, so we test with file)
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("output.txt");

        let result = write_output(&content, OutputDestination::File(file_path));
        assert!(result.is_ok());
    }
}
