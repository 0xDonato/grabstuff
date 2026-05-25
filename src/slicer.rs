//! File content slicing and extraction.
//!
//! Provides efficient reading of file portions (head/tail) by lines or characters,
//! with optimizations for large files using reverse chunk reading.

use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::PathBuf;

const UTF8_MAX_BYTES_PER_CHAR: usize = 4;

/// Specifies how to slice file content.
#[derive(Clone, Debug, PartialEq)]
pub enum SliceMode {
    /// Read the entire file.
    Full,
    /// Read the first N lines.
    HeadLines(usize),
    /// Read the first N characters.
    HeadChars(usize),
    /// Read the last N lines.
    TailLines(usize),
    /// Read the last N characters.
    TailChars(usize),
}

/// Represents extracted file content with metadata.
pub struct FileContent {
    /// Path to the source file.
    pub path: PathBuf,
    /// Extracted content as a string.
    pub content: String,
    /// Information about which portion was extracted.
    pub slice_info: SliceInfo,
}

/// Describes the portion of a file that was extracted.
#[derive(Debug, PartialEq)]
pub enum SliceInfo {
    /// The entire file was read.
    Full,
    /// A range of lines was extracted.
    Lines { start: usize, end: usize },
    /// A range of characters was extracted.
    Chars { start: usize, end: usize },
}

impl SliceInfo {
    /// Returns a human-readable description of the slice.
    ///
    /// # Examples
    ///
    /// - `SliceInfo::Full` → `"full"`
    /// - `SliceInfo::Lines { start: 1, end: 50 }` → `"lines 1-50"`
    pub fn description(&self) -> String {
        match self {
            SliceInfo::Full => "full".to_string(),
            SliceInfo::Lines { start, end } => format!("lines {}-{}", start, end),
            SliceInfo::Chars { start, end } => format!("chars {}-{}", start, end),
        }
    }
}

/// Extracts portions of files according to a specified slice mode.
///
/// Uses buffered I/O and optimized algorithms for efficient processing
/// of both small and large files.
pub struct Slicer {
    mode: SliceMode,
}

impl Slicer {
    /// Creates a new `Slicer` with the specified slice mode.
    pub fn new(mode: SliceMode) -> Self {
        Slicer { mode }
    }

    /// Processes a file and extracts content according to the slice mode.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file to process
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read.
    pub fn process_file(&self, path: &PathBuf) -> Result<FileContent> {
        let (content, slice_info) = match &self.mode {
            SliceMode::Full => self.read_full(path)?,
            SliceMode::HeadLines(n) => self.read_head_lines(path, *n)?,
            SliceMode::HeadChars(n) => self.read_head_chars(path, *n)?,
            SliceMode::TailLines(n) => self.read_tail_lines(path, *n)?,
            SliceMode::TailChars(n) => self.read_tail_chars(path, *n)?,
        };

        Ok(FileContent {
            path: path.clone(),
            content,
            slice_info,
        })
    }

    fn read_full(&self, path: &PathBuf) -> Result<(String, SliceInfo)> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read file: {}", path.display()))?;
        Ok((content, SliceInfo::Full))
    }

    fn read_head_lines(&self, path: &PathBuf, n: usize) -> Result<(String, SliceInfo)> {
        let file =
            File::open(path).with_context(|| format!("Failed to open file: {}", path.display()))?;
        let reader = BufReader::with_capacity(64 * 1024, file); // 64KB buffer

        if n == 0 {
            return Ok((String::new(), SliceInfo::Lines { start: 0, end: 0 }));
        }

        let mut lines = Vec::with_capacity(n.min(1024));
        for line in reader.lines().take(n) {
            lines.push(line.with_context(|| format!("Failed to read file: {}", path.display()))?);
        }

        let count = lines.len();
        let content = lines.join("\n");

        Ok((
            content,
            SliceInfo::Lines {
                start: 1,
                end: count,
            },
        ))
    }

    fn read_head_chars(&self, path: &PathBuf, n: usize) -> Result<(String, SliceInfo)> {
        let file =
            File::open(path).with_context(|| format!("Failed to open file: {}", path.display()))?;
        let mut reader = BufReader::with_capacity(64 * 1024, file);

        if n == 0 {
            return Ok((String::new(), SliceInfo::Chars { start: 0, end: 0 }));
        }

        let file_size = reader.get_ref().metadata()?.len() as usize;

        // Read enough bytes to get n chars (UTF-8 can be up to 4 bytes per char)
        let bytes_to_read = n.saturating_mul(UTF8_MAX_BYTES_PER_CHAR).min(file_size);
        let mut buffer = vec![0u8; bytes_to_read];
        let bytes_read = reader.read(&mut buffer)?;
        buffer.truncate(bytes_read);

        // Convert to string and take n chars
        let text = String::from_utf8_lossy(&buffer);
        let content: String = text.chars().take(n).collect();
        let char_count = content.chars().count();

        Ok((
            content,
            SliceInfo::Chars {
                start: 0,
                end: char_count,
            },
        ))
    }

    fn read_tail_lines(&self, path: &PathBuf, n: usize) -> Result<(String, SliceInfo)> {
        let file =
            File::open(path).with_context(|| format!("Failed to open file: {}", path.display()))?;
        let metadata = file.metadata()?;
        let file_size = metadata.len();

        if file_size == 0 || n == 0 {
            return Ok((String::new(), SliceInfo::Lines { start: 0, end: 0 }));
        }

        // For small files, just read the whole thing
        if file_size < 64 * 1024 {
            let content = std::fs::read_to_string(path)?;
            let lines: Vec<&str> = content.lines().collect();
            let total = lines.len();
            let skip = total.saturating_sub(n);
            let sliced = lines[skip..].join("\n");
            return Ok((
                sliced,
                SliceInfo::Lines {
                    start: skip + 1,
                    end: total,
                },
            ));
        }

        // For larger files, read from the end in chunks
        let mut file = file;
        let chunk_size = 64 * 1024u64;
        let mut lines_found = Vec::new();
        let mut pos = file_size;
        let mut trailing_fragment = String::new();

        while lines_found.len() < n && pos > 0 {
            let read_size = chunk_size.min(pos);
            pos -= read_size;
            file.seek(SeekFrom::Start(pos))?;

            let mut buffer = vec![0u8; read_size as usize];
            file.read_exact(&mut buffer)?;

            let chunk = String::from_utf8_lossy(&buffer);
            let mut chunk_str = chunk.to_string();
            chunk_str.push_str(&trailing_fragment);

            let chunk_lines: Vec<&str> = chunk_str.lines().collect();

            // First line might be partial if we're not at file start
            if pos > 0 && !chunk_lines.is_empty() {
                trailing_fragment = chunk_lines[0].to_string();
                for line in chunk_lines[1..].iter().rev() {
                    lines_found.push(line.to_string());
                    if lines_found.len() >= n {
                        break;
                    }
                }
            } else {
                for line in chunk_lines.iter().rev() {
                    lines_found.push(line.to_string());
                    if lines_found.len() >= n {
                        break;
                    }
                }
                trailing_fragment.clear();
            }
        }

        // Include trailing fragment if we reached the start
        if pos == 0 && !trailing_fragment.is_empty() && lines_found.len() < n {
            lines_found.push(trailing_fragment);
        }

        lines_found.reverse();
        let count = lines_found.len();
        let content = lines_found.join("\n");
        let total_lines = count_file_lines(path)?;
        let start = if count == 0 {
            0
        } else {
            total_lines.saturating_sub(count) + 1
        };

        Ok((
            content,
            SliceInfo::Lines {
                start,
                end: total_lines,
            },
        ))
    }

    fn read_tail_chars(&self, path: &PathBuf, n: usize) -> Result<(String, SliceInfo)> {
        let file =
            File::open(path).with_context(|| format!("Failed to open file: {}", path.display()))?;
        let metadata = file.metadata()?;
        let file_size = metadata.len() as usize;

        if file_size == 0 || n == 0 {
            return Ok((String::new(), SliceInfo::Chars { start: 0, end: 0 }));
        }

        // Read from end - estimate bytes needed (4 bytes per char max for UTF-8)
        let bytes_to_read = n.saturating_mul(UTF8_MAX_BYTES_PER_CHAR).min(file_size);
        let start_pos = file_size - bytes_to_read;

        let mut file = file;
        file.seek(SeekFrom::Start(start_pos as u64))?;

        let mut buffer = vec![0u8; bytes_to_read];
        file.read_exact(&mut buffer)?;

        let text = suffix_text(&buffer, start_pos > 0);
        let suffix_chars = text.chars().count();
        let skip = suffix_chars.saturating_sub(n);
        let content: String = text.chars().skip(skip).collect();
        let char_count = content.chars().count();
        let total_chars = count_file_chars(path)?;
        let start = total_chars.saturating_sub(char_count);

        Ok((
            content,
            SliceInfo::Chars {
                start,
                end: total_chars,
            },
        ))
    }
}

fn count_file_lines(path: &PathBuf) -> Result<usize> {
    let file =
        File::open(path).with_context(|| format!("Failed to open file: {}", path.display()))?;
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut buffer = Vec::new();
    let mut count = 0;

    loop {
        let bytes_read = reader.read_until(b'\n', &mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        count += 1;
        buffer.clear();
    }

    Ok(count)
}

fn count_file_chars(path: &PathBuf) -> Result<usize> {
    let content =
        std::fs::read(path).with_context(|| format!("Failed to read file: {}", path.display()))?;
    Ok(String::from_utf8_lossy(&content).chars().count())
}

fn suffix_text(buffer: &[u8], may_start_mid_char: bool) -> std::borrow::Cow<'_, str> {
    if !may_start_mid_char {
        return String::from_utf8_lossy(buffer);
    }

    let max_offset = buffer.len().min(UTF8_MAX_BYTES_PER_CHAR);
    for offset in 0..max_offset {
        if let Ok(text) = std::str::from_utf8(&buffer[offset..]) {
            return std::borrow::Cow::Borrowed(text);
        }
    }

    String::from_utf8_lossy(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file.flush().unwrap();
        file
    }

    #[test]
    fn test_slice_info_description() {
        assert_eq!(SliceInfo::Full.description(), "full");
        assert_eq!(
            SliceInfo::Lines { start: 1, end: 50 }.description(),
            "lines 1-50"
        );
        assert_eq!(
            SliceInfo::Chars { start: 0, end: 100 }.description(),
            "chars 0-100"
        );
    }

    #[test]
    fn test_read_full() {
        let content = "line1\nline2\nline3";
        let file = create_temp_file(content);
        let path = file.path().to_path_buf();

        let slicer = Slicer::new(SliceMode::Full);
        let result = slicer.process_file(&path).unwrap();

        assert_eq!(result.content, content);
        assert_eq!(result.slice_info, SliceInfo::Full);
    }

    #[test]
    fn test_read_head_lines() {
        let content = "line1\nline2\nline3\nline4\nline5";
        let file = create_temp_file(content);
        let path = file.path().to_path_buf();

        let slicer = Slicer::new(SliceMode::HeadLines(3));
        let result = slicer.process_file(&path).unwrap();

        assert_eq!(result.content, "line1\nline2\nline3");
        assert_eq!(result.slice_info, SliceInfo::Lines { start: 1, end: 3 });
    }

    #[test]
    fn test_read_head_chars() {
        let content = "Hello, World!";
        let file = create_temp_file(content);
        let path = file.path().to_path_buf();

        let slicer = Slicer::new(SliceMode::HeadChars(5));
        let result = slicer.process_file(&path).unwrap();

        assert_eq!(result.content, "Hello");
        assert_eq!(result.slice_info, SliceInfo::Chars { start: 0, end: 5 });
    }

    #[test]
    fn test_zero_length_slices_report_empty_ranges() {
        let content = "line1\nline2\nline3";
        let file = create_temp_file(content);
        let path = file.path().to_path_buf();

        let cases = [
            SliceMode::HeadLines(0),
            SliceMode::TailLines(0),
            SliceMode::HeadChars(0),
            SliceMode::TailChars(0),
        ];

        for mode in cases {
            let result = Slicer::new(mode.clone()).process_file(&path).unwrap();
            assert_eq!(result.content, "");
            match mode {
                SliceMode::HeadLines(_) | SliceMode::TailLines(_) => {
                    assert_eq!(result.slice_info, SliceInfo::Lines { start: 0, end: 0 });
                }
                SliceMode::HeadChars(_) | SliceMode::TailChars(_) => {
                    assert_eq!(result.slice_info, SliceInfo::Chars { start: 0, end: 0 });
                }
                SliceMode::Full => unreachable!(),
            }
        }
    }

    #[test]
    fn test_large_char_counts_do_not_overflow() {
        let content = "Hello, 世界!";
        let file = create_temp_file(content);
        let path = file.path().to_path_buf();

        let head = Slicer::new(SliceMode::HeadChars(usize::MAX))
            .process_file(&path)
            .unwrap();
        let tail = Slicer::new(SliceMode::TailChars(usize::MAX))
            .process_file(&path)
            .unwrap();

        assert_eq!(head.content, content);
        assert_eq!(head.slice_info, SliceInfo::Chars { start: 0, end: 10 });
        assert_eq!(tail.content, content);
        assert_eq!(tail.slice_info, SliceInfo::Chars { start: 0, end: 10 });
    }

    #[test]
    fn test_read_tail_lines_small_file() {
        let content = "line1\nline2\nline3\nline4\nline5";
        let file = create_temp_file(content);
        let path = file.path().to_path_buf();

        let slicer = Slicer::new(SliceMode::TailLines(2));
        let result = slicer.process_file(&path).unwrap();

        assert_eq!(result.content, "line4\nline5");
    }

    #[test]
    fn test_read_tail_lines_large_file_reports_exact_line_numbers() {
        let content = (1..=20_000)
            .map(|n| format!("line{}", n))
            .collect::<Vec<_>>()
            .join("\n");
        let file = create_temp_file(&content);
        let path = file.path().to_path_buf();

        let slicer = Slicer::new(SliceMode::TailLines(3));
        let result = slicer.process_file(&path).unwrap();

        assert_eq!(result.content, "line19998\nline19999\nline20000");
        assert_eq!(
            result.slice_info,
            SliceInfo::Lines {
                start: 19998,
                end: 20000
            }
        );
    }

    #[test]
    fn test_read_tail_chars() {
        let content = "Hello, World!";
        let file = create_temp_file(content);
        let path = file.path().to_path_buf();

        let slicer = Slicer::new(SliceMode::TailChars(6));
        let result = slicer.process_file(&path).unwrap();

        assert_eq!(result.content, "World!");
    }

    #[test]
    fn test_read_tail_chars_reports_character_offsets() {
        let content = "ééé";
        let file = create_temp_file(content);
        let path = file.path().to_path_buf();

        let slicer = Slicer::new(SliceMode::TailChars(1));
        let result = slicer.process_file(&path).unwrap();

        assert_eq!(result.content, "é");
        assert_eq!(result.slice_info, SliceInfo::Chars { start: 2, end: 3 });
    }

    #[test]
    fn test_empty_file() {
        let file = create_temp_file("");
        let path = file.path().to_path_buf();

        let slicer = Slicer::new(SliceMode::Full);
        let result = slicer.process_file(&path).unwrap();

        assert_eq!(result.content, "");
        assert_eq!(result.slice_info, SliceInfo::Full);
    }

    #[test]
    fn test_head_lines_more_than_file() {
        let content = "line1\nline2";
        let file = create_temp_file(content);
        let path = file.path().to_path_buf();

        let slicer = Slicer::new(SliceMode::HeadLines(100));
        let result = slicer.process_file(&path).unwrap();

        assert_eq!(result.content, "line1\nline2");
        assert_eq!(result.slice_info, SliceInfo::Lines { start: 1, end: 2 });
    }

    #[test]
    fn test_unicode_chars() {
        let content = "Hello, 世界!";
        let file = create_temp_file(content);
        let path = file.path().to_path_buf();

        let slicer = Slicer::new(SliceMode::HeadChars(9));
        let result = slicer.process_file(&path).unwrap();

        assert_eq!(result.content, "Hello, 世界");
    }
}
