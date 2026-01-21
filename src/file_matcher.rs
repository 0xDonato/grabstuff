//! File discovery and pattern matching.
//!
//! Provides glob-based file matching with support for ignore patterns,
//! binary file detection, and file size limits.

use anyhow::Result;
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use std::path::PathBuf;

/// Maximum file size to process (10MB).
const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// Number of bytes to check for binary content detection.
const BINARY_CHECK_SIZE: usize = 512;

/// Matches files against glob patterns while respecting ignore rules.
///
/// Automatically skips binary files and files exceeding the size limit.
pub struct FileMatcher {
    ignore_set: GlobSet,
}

impl FileMatcher {
    /// Creates a new `FileMatcher` with the specified ignore patterns.
    ///
    /// # Arguments
    ///
    /// * `ignore_patterns` - Glob patterns for files/directories to exclude
    pub fn new(ignore_patterns: &[String]) -> Self {
        let ignore_set = build_globset(ignore_patterns);
        FileMatcher { ignore_set }
    }

    /// Matches files against the given glob patterns.
    ///
    /// Returns a sorted, deduplicated list of matching file paths.
    /// Automatically excludes ignored files, binary files, and files over 10MB.
    ///
    /// # Arguments
    ///
    /// * `patterns` - Glob patterns to match (e.g., `"src/*.rs"`, `"docs/"`)
    ///
    /// # Errors
    ///
    /// Returns an error if a glob pattern is invalid.
    pub fn match_patterns(&self, patterns: &[String]) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();

        for pattern in patterns {
            self.collect_files(pattern, &mut files)?;
        }

        // Sort and deduplicate
        files.sort();
        files.dedup();

        Ok(files)
    }

    fn collect_files(&self, pattern: &str, files: &mut Vec<PathBuf>) -> Result<()> {
        // Determine the base directory and glob pattern
        let (base_dir, glob_pattern) = parse_pattern(pattern);

        // Build the glob matcher for this pattern
        let matcher = Glob::new(&glob_pattern)
            .map_err(|e| anyhow::anyhow!("Invalid glob pattern '{}': {}", pattern, e))?
            .compile_matcher();

        // Use ignore crate's WalkBuilder for fast parallel directory walking
        let walker = WalkBuilder::new(&base_dir)
            .hidden(false) // Don't skip hidden files by default
            .git_ignore(false) // We handle our own ignores
            .git_global(false)
            .git_exclude(false)
            .parents(false)
            .follow_links(false)
            .build();

        for entry in walker.flatten() {
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            // Check if path matches the glob pattern
            let relative = path.strip_prefix(&base_dir).unwrap_or(path);
            if !matcher.is_match(relative) && !matcher.is_match(path) {
                continue;
            }

            // Check ignore patterns
            if self.is_ignored(path) {
                continue;
            }

            // Check file validity (size and binary)
            if !self.is_valid_file(path) {
                continue;
            }

            files.push(path.to_path_buf());
        }

        Ok(())
    }

    fn is_ignored(&self, path: &std::path::Path) -> bool {
        let path_str = path.to_string_lossy();

        // Check compiled globset
        if self.ignore_set.is_match(path) {
            return true;
        }

        // Also check path components for directory patterns
        for component in path.components() {
            let comp_str = component.as_os_str().to_string_lossy();
            if self.ignore_set.is_match(comp_str.as_ref()) {
                return true;
            }
        }

        // Check common ignored directories inline for speed
        if path_str.contains("/.git/")
            || path_str.contains("/node_modules/")
            || path_str.contains("/target/")
            || path_str.ends_with("/.env")
        {
            return true;
        }

        false
    }

    fn is_valid_file(&self, path: &std::path::Path) -> bool {
        // Check file size first (cheap operation)
        let metadata = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(_) => return false,
        };

        if metadata.len() > MAX_FILE_SIZE {
            eprintln!("Warning: skipping {} (exceeds 10MB limit)", path.display());
            return false;
        }

        // Quick binary check - only read first 512 bytes
        if let Ok(file) = std::fs::File::open(path) {
            use std::io::Read;
            let mut buffer = [0u8; BINARY_CHECK_SIZE];
            let mut reader = std::io::BufReader::new(file);
            if let Ok(bytes_read) = reader.read(&mut buffer) {
                // Use memchr for fast null byte detection
                if memchr::memchr(0, &buffer[..bytes_read]).is_some() {
                    return false;
                }
            }
        }

        true
    }
}

fn build_globset(patterns: &[String]) -> GlobSet {
    let mut builder = GlobSetBuilder::new();

    for pattern in patterns {
        // Normalize pattern for globset
        let normalized = if pattern.ends_with('/') {
            format!("**/{}", pattern.trim_end_matches('/'))
        } else if !pattern.contains('/') && !pattern.contains('*') {
            // Simple name like "node_modules" - match anywhere
            format!("**/{}", pattern)
        } else {
            pattern.clone()
        };

        if let Ok(glob) = Glob::new(&normalized) {
            builder.add(glob);
        }
    }

    builder.build().unwrap_or_else(|_| GlobSet::empty())
}

fn parse_pattern(pattern: &str) -> (PathBuf, String) {
    // Handle directory patterns
    if pattern.ends_with('/') {
        return (PathBuf::from(pattern), "**/*".to_string());
    }

    // Find the first glob character
    let glob_chars = ['*', '?', '[', '{'];
    let first_glob = pattern
        .char_indices()
        .find(|(_, c)| glob_chars.contains(c))
        .map(|(i, _)| i);

    match first_glob {
        Some(idx) => {
            // Split at the last '/' before the glob
            let prefix = &pattern[..idx];
            let last_slash = prefix.rfind('/').map(|i| i + 1).unwrap_or(0);

            let base = if last_slash > 0 {
                PathBuf::from(&pattern[..last_slash - 1])
            } else {
                PathBuf::from(".")
            };

            let glob = if last_slash > 0 {
                pattern[last_slash..].to_string()
            } else {
                pattern.to_string()
            };

            (base, glob)
        }
        None => {
            // No glob characters - exact file path
            let path = PathBuf::from(pattern);
            if path.is_dir() {
                (path, "**/*".to_string())
            } else if let Some(parent) = path.parent() {
                let file_name = path.file_name().unwrap().to_string_lossy().to_string();
                (parent.to_path_buf(), file_name)
            } else {
                (PathBuf::from("."), pattern.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_file_matcher_new() {
        let ignores = vec!["*.log".to_string(), "temp/".to_string()];
        let matcher = FileMatcher::new(&ignores);
        // Matcher should be created successfully
        assert!(!matcher.ignore_set.is_empty());
    }

    #[test]
    fn test_file_matcher_empty_ignores() {
        let ignores: Vec<String> = vec![];
        let matcher = FileMatcher::new(&ignores);
        assert!(matcher.ignore_set.is_empty());
    }

    #[test]
    fn test_match_patterns_with_glob() {
        let dir = tempdir().unwrap();
        let rs_file = dir.path().join("test.rs");
        let txt_file = dir.path().join("test.txt");

        fs::write(&rs_file, "fn main() {}").unwrap();
        fs::write(&txt_file, "hello").unwrap();

        let matcher = FileMatcher::new(&[]);
        let pattern = format!("{}/*.rs", dir.path().display());
        let files = matcher.match_patterns(&[pattern]).unwrap();

        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("test.rs"));
    }

    #[test]
    fn test_match_patterns_deduplication() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("test.rs");
        fs::write(&file, "content").unwrap();

        let matcher = FileMatcher::new(&[]);
        let pattern = format!("{}/*.rs", dir.path().display());
        // Same pattern twice
        let files = matcher.match_patterns(&[pattern.clone(), pattern]).unwrap();

        assert_eq!(files.len(), 1); // Should be deduplicated
    }

    #[test]
    fn test_ignore_patterns() {
        let dir = tempdir().unwrap();
        let keep_file = dir.path().join("keep.rs");
        let ignore_file = dir.path().join("ignore.log");

        fs::write(&keep_file, "content").unwrap();
        fs::write(&ignore_file, "content").unwrap();

        let matcher = FileMatcher::new(&["*.log".to_string()]);
        let pattern = format!("{}/*", dir.path().display());
        let files = matcher.match_patterns(&[pattern]).unwrap();

        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("keep.rs"));
    }

    #[test]
    fn test_parse_pattern_with_glob() {
        let (base, glob) = parse_pattern("src/*.rs");
        assert_eq!(base, PathBuf::from("src"));
        assert_eq!(glob, "*.rs");
    }

    #[test]
    fn test_parse_pattern_directory() {
        let (base, glob) = parse_pattern("docs/");
        assert_eq!(base, PathBuf::from("docs/"));
        assert_eq!(glob, "**/*");
    }

    #[test]
    fn test_parse_pattern_double_star() {
        let (base, glob) = parse_pattern("src/**/*.rs");
        assert_eq!(base, PathBuf::from("src"));
        assert_eq!(glob, "**/*.rs");
    }

    #[test]
    fn test_build_globset_normalizes_patterns() {
        let patterns = vec!["node_modules/".to_string(), "*.log".to_string()];
        let globset = build_globset(&patterns);
        assert!(!globset.is_empty());
    }

    #[test]
    fn test_binary_file_detection() {
        let dir = tempdir().unwrap();
        let binary_file = dir.path().join("binary.bin");
        let text_file = dir.path().join("text.txt");

        // Write a file with null bytes (binary)
        fs::write(&binary_file, b"hello\x00world").unwrap();
        fs::write(&text_file, "hello world").unwrap();

        let matcher = FileMatcher::new(&[]);
        let pattern = format!("{}/*", dir.path().display());
        let files = matcher.match_patterns(&[pattern]).unwrap();

        // Only text file should be included
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("text.txt"));
    }
}
