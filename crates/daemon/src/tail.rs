use anyhow::{Context, Result};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Tracks byte offsets per file for incremental reads
pub struct FileTailer {
    offsets: HashMap<PathBuf, u64>,
}

impl FileTailer {
    pub fn new() -> Self {
        Self {
            offsets: HashMap::new(),
        }
    }

    /// Restore offsets from saved state
    pub fn with_offsets(offsets: HashMap<String, u64>) -> Self {
        let offsets = offsets
            .into_iter()
            .map(|(k, v)| (PathBuf::from(k), v))
            .collect();
        Self { offsets }
    }

    /// Export offsets for persistence
    pub fn export_offsets(&self) -> HashMap<String, u64> {
        self.offsets
            .iter()
            .map(|(k, v)| (k.to_string_lossy().to_string(), *v))
            .collect()
    }

    /// Read new lines from a file since the last read.
    /// Returns the new lines and updates the internal offset.
    pub fn read_new_lines(&mut self, path: &Path) -> Result<Vec<String>> {
        let metadata = std::fs::metadata(path)
            .with_context(|| format!("Cannot stat {}", path.display()))?;
        let file_size = metadata.len();

        let current_offset = self.offsets.get(path).copied().unwrap_or(0);

        // Detect file truncation (e.g., file was replaced)
        if file_size < current_offset {
            tracing::info!(
                "File truncated ({}B < {}B offset), resetting: {}",
                file_size,
                current_offset,
                path.display()
            );
            self.offsets.insert(path.to_path_buf(), 0);
            return self.read_new_lines(path);
        }

        // No new data
        if file_size == current_offset {
            return Ok(Vec::new());
        }

        let mut file = std::fs::File::open(path)
            .with_context(|| format!("Cannot open {}", path.display()))?;
        file.seek(SeekFrom::Start(current_offset))
            .with_context(|| format!("Cannot seek in {}", path.display()))?;

        let reader = BufReader::new(&file);
        let mut lines = Vec::new();
        let mut bytes_read = 0u64;

        for line_result in reader.lines() {
            match line_result {
                Ok(line) => {
                    // +1 for the newline character
                    bytes_read += line.len() as u64 + 1;
                    if !line.is_empty() {
                        lines.push(line);
                    }
                }
                Err(e) => {
                    tracing::warn!("Error reading line from {}: {}", path.display(), e);
                    break;
                }
            }
        }

        self.offsets
            .insert(path.to_path_buf(), current_offset + bytes_read);

        Ok(lines)
    }

    /// Get the current offset for a file
    pub fn offset(&self, path: &Path) -> u64 {
        self.offsets.get(path).copied().unwrap_or(0)
    }

    /// Reset offset for a file (e.g., after full upload)
    pub fn reset(&mut self, path: &Path) {
        self.offsets.remove(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        std::fs::write(&path, "{\"a\":1}\n{\"b\":2}\n").unwrap();

        let mut tailer = FileTailer::new();
        let lines = tailer.read_new_lines(&path).unwrap();
        assert_eq!(lines, vec!["{\"a\":1}", "{\"b\":2}"]);
    }

    #[test]
    fn test_incremental_reads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");

        // First write
        std::fs::write(&path, "{\"a\":1}\n").unwrap();
        let mut tailer = FileTailer::new();
        let lines = tailer.read_new_lines(&path).unwrap();
        assert_eq!(lines, vec!["{\"a\":1}"]);

        // Append more
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(b"{\"b\":2}\n{\"c\":3}\n").unwrap();

        let lines = tailer.read_new_lines(&path).unwrap();
        assert_eq!(lines, vec!["{\"b\":2}", "{\"c\":3}"]);
    }

    #[test]
    fn test_no_new_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        std::fs::write(&path, "{\"a\":1}\n").unwrap();

        let mut tailer = FileTailer::new();
        let _ = tailer.read_new_lines(&path).unwrap();

        // No new data
        let lines = tailer.read_new_lines(&path).unwrap();
        assert!(lines.is_empty());
    }

    #[test]
    fn test_truncation_detection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");

        // Write some data
        std::fs::write(&path, "{\"a\":1}\n{\"b\":2}\n{\"c\":3}\n").unwrap();
        let mut tailer = FileTailer::new();
        let _ = tailer.read_new_lines(&path).unwrap();

        // Truncate and write less
        std::fs::write(&path, "{\"x\":1}\n").unwrap();
        let lines = tailer.read_new_lines(&path).unwrap();
        assert_eq!(lines, vec!["{\"x\":1}"]);
    }

    #[test]
    fn test_offset_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        std::fs::write(&path, "{\"a\":1}\n{\"b\":2}\n").unwrap();

        let mut tailer = FileTailer::new();
        let _ = tailer.read_new_lines(&path).unwrap();

        let offsets = tailer.export_offsets();
        assert!(offsets.contains_key(&path.to_string_lossy().to_string()));

        // Restore and verify
        let mut tailer2 = FileTailer::with_offsets(offsets);
        let lines = tailer2.read_new_lines(&path).unwrap();
        assert!(lines.is_empty()); // No new data
    }
}
