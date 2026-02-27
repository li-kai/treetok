//! Directory walking and file content loading.

use std::path::{Path, PathBuf};

/// Classification of a file's content type.
#[derive(Debug, Clone)]
pub enum FileKind {
    /// Valid UTF-8 text — will be tokenized.
    Text,
    /// Non-UTF-8 binary content — shown as `[binary]`.
    Binary,
    /// File exceeds the 3 MB size limit — shown as `[too large]`.
    TooLarge,
    /// Could not be read — shown as `[error]`.
    Error(String),
}

/// A single file discovered during a directory walk.
#[derive(Debug, Clone)]
pub struct FileEntry {
    /// Absolute path (used for reading).
    pub path: PathBuf,
    /// Path relative to the walk root (used for display).
    pub rel_path: PathBuf,
    /// Content classification.
    pub kind: FileKind,
    /// Full UTF-8 content, populated only for [`FileKind::Text`].
    pub content: Option<String>,
}

/// Options controlling the directory walk.
pub struct WalkOptions {
    /// Disable `.gitignore` / `.ignore` filtering when `true`.
    pub no_ignore: bool,
    /// Maximum depth to descend (`None` = unlimited).
    pub depth: Option<usize>,
}

/// Walk each path in `roots` and return one [`WalkResult`] per root.
#[must_use]
pub fn walk_paths(roots: &[PathBuf], opts: &WalkOptions) -> Vec<WalkResult> {
    roots.iter().map(|root| walk_one(root, opts)).collect()
}

/// Entries for a single root directory.
pub struct WalkResult {
    /// The root path (as given by the user).
    pub root: PathBuf,
    /// All files found beneath the root, sorted by relative path.
    pub entries: Vec<FileEntry>,
    /// Errors encountered during the walk (e.g. permission denied on a
    /// subdirectory).  These don't prevent the rest of the walk from
    /// completing.
    pub errors: Vec<WalkError>,
}

impl WalkResult {
    /// Returns `true` if the walk encountered any errors.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

/// A bundle of non-fatal walk errors, rendered as related diagnostics.
#[derive(Debug, thiserror::Error, miette::Diagnostic)]
#[error("errors while walking {root}")]
#[diagnostic(code(treetok::walk))]
pub struct WalkErrors {
    root: PathBuf,
    #[related]
    related: Vec<WalkError>,
}

impl WalkErrors {
    /// Build a diagnostic from a [`WalkResult`]'s errors (cloning them).
    /// Returns `None` if there are no errors.
    #[must_use]
    pub fn from_result(result: &WalkResult) -> Option<Self> {
        if result.errors.is_empty() {
            return None;
        }
        Some(Self {
            root: result.root.clone(),
            related: result
                .errors
                .iter()
                .map(|e| WalkError {
                    message: e.message.clone(),
                })
                .collect(),
        })
    }
}

/// A non-fatal error encountered while walking a directory tree.
#[derive(Debug, thiserror::Error, miette::Diagnostic)]
#[error("{message}")]
#[diagnostic(code(treetok::walk::entry))]
pub struct WalkError {
    message: String,
}

fn walk_one(root: &Path, opts: &WalkOptions) -> WalkResult {
    let mut builder = ignore::WalkBuilder::new(root);

    if opts.no_ignore {
        builder
            .ignore(false)
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false);
    }

    if let Some(depth) = opts.depth {
        builder.max_depth(Some(depth));
    }

    let (tx, rx) = std::sync::mpsc::channel();
    builder.build_parallel().run(|| {
        let tx = tx.clone();
        let root = root.to_path_buf();
        Box::new(move |result| {
            match result {
                Ok(dir_entry) => {
                    if !dir_entry.file_type().is_some_and(|ft| ft.is_file()) {
                        return ignore::WalkState::Continue;
                    }
                    let abs_path = dir_entry.path().to_path_buf();
                    let rel_path = abs_path
                        .strip_prefix(&root)
                        .unwrap_or(&abs_path)
                        .to_path_buf();
                    let _ = tx.send(Ok(process_file(abs_path, rel_path)));
                }
                Err(e) => {
                    let _ = tx.send(Err(WalkError {
                        message: e.to_string(),
                    }));
                }
            }
            ignore::WalkState::Continue
        })
    });
    drop(tx);

    let mut entries = Vec::new();
    let mut errors = Vec::new();
    for item in rx {
        match item {
            Ok(entry) => entries.push(entry),
            Err(e) => errors.push(e),
        }
    }

    // Sort for deterministic output regardless of thread scheduling.
    entries.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));

    WalkResult {
        root: root.to_path_buf(),
        entries,
        errors,
    }
}

/// Maximum file size we will read (3 MB).
const MAX_FILE_SIZE: u64 = 3 * 1024 * 1024;
/// Number of bytes read for UTF-8 sniffing.
const SNIFF_BYTES: usize = 8 * 1024;

/// Process a file, classifying its content and loading text if applicable.
#[must_use]
pub fn process_file(path: PathBuf, rel_path: PathBuf) -> FileEntry {
    // Stat the file first to check size without reading.
    let meta = match std::fs::metadata(&path) {
        Err(e) => {
            return FileEntry {
                path,
                rel_path,
                kind: FileKind::Error(e.to_string()),
                content: None,
            };
        }
        Ok(m) => m,
    };

    if meta.len() > MAX_FILE_SIZE {
        return FileEntry {
            path,
            rel_path,
            kind: FileKind::TooLarge,
            content: None,
        };
    }

    // Sniff the first 8 KB for UTF-8 validity.
    let sniff = match read_first_bytes(&path, SNIFF_BYTES) {
        Err(e) => {
            return FileEntry {
                path,
                rel_path,
                kind: FileKind::Error(e.to_string()),
                content: None,
            };
        }
        Ok(b) => b,
    };

    if !is_valid_utf8_sniff(&sniff) {
        return FileEntry {
            path,
            rel_path,
            kind: FileKind::Binary,
            content: None,
        };
    }

    // Read the full file as a string.
    match std::fs::read_to_string(&path) {
        Ok(content) => FileEntry {
            path,
            rel_path,
            kind: FileKind::Text,
            content: Some(content),
        },
        Err(e) => FileEntry {
            path,
            rel_path,
            kind: FileKind::Error(e.to_string()),
            content: None,
        },
    }
}

/// Returns `true` if `bytes` is valid UTF-8 or only has an incomplete
/// multi-byte sequence at the very end (i.e. the sniff buffer was truncated
/// mid-character).  Returns `false` for any genuinely invalid byte sequence.
fn is_valid_utf8_sniff(bytes: &[u8]) -> bool {
    match std::str::from_utf8(bytes) {
        Ok(_) => true,
        Err(e) => e.error_len().is_none(), // None = unexpected end, not invalid bytes
    }
}

fn read_first_bytes(path: &Path, limit: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0u8; limit];
    let n = file.read(&mut buf)?;
    buf.truncate(n);
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // ── helpers ────────────────────────────────────────────────────────────

    fn temp_file(dir: &std::path::Path, name: &str, content: &[u8]) -> PathBuf {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
        path
    }

    // ── is_valid_utf8_sniff ────────────────────────────────────────────────

    /// Pure ASCII and complete multi-byte sequences are valid.
    #[rstest]
    #[case(b"hello" as &[u8])] // ASCII
    #[case(b"caf\xC3\xA9" as &[u8])] // é  (U+00E9, 2 bytes) – complete
    #[case(b"\xE2\x80\x93" as &[u8])] // –  (U+2013, 3 bytes) – complete
    #[case(b"" as &[u8])] // empty buffer
    fn utf8_sniff_accepts_valid(#[case] bytes: &[u8]) {
        assert!(is_valid_utf8_sniff(bytes));
    }

    /// A buffer truncated mid-sequence (incomplete tail) should still pass —
    /// this is the case the fix addresses.
    #[rstest]
    #[case(b"ok\xC3" as &[u8])] // first byte of é cut off
    #[case(b"ok\xE2\x80" as &[u8])] // first two bytes of – cut off
    #[case(b"ok\xF0\x9F\x98" as &[u8])] // first three bytes of a 4-byte emoji cut off
    fn utf8_sniff_accepts_truncated_tail(#[case] bytes: &[u8]) {
        assert!(is_valid_utf8_sniff(bytes));
    }

    /// Genuinely invalid byte sequences must be rejected.
    #[rstest]
    #[case(b"\xFF\xFE" as &[u8])] // invalid UTF-8 bytes
    #[case(b"hi\x80there" as &[u8])] // continuation byte with no lead
    #[case(b"\xC3\x28" as &[u8])] // invalid 2-byte sequence
    fn utf8_sniff_rejects_invalid(#[case] bytes: &[u8]) {
        assert!(!is_valid_utf8_sniff(bytes));
    }

    // ── file-kind detection ────────────────────────────────────────────────

    /// Text content (valid UTF-8) should produce `FileKind::Text`.
    #[rstest]
    #[case(b"hello, world\n" as &[u8])]
    #[case(b"# comment\nfn main() {}" as &[u8])]
    #[case(b"" as &[u8])] // empty file is valid UTF-8
    fn text_file_detected_as_text(#[case] content: &[u8]) {
        let dir = tempfile::tempdir().unwrap();
        let path = temp_file(dir.path(), "f", content);
        let entry = process_file(path, "f".into());
        assert!(
            matches!(entry.kind, FileKind::Text),
            "expected Text, got {:?}",
            entry.kind
        );
    }

    /// Non-UTF-8 bytes should produce `FileKind::Binary`.
    #[rstest]
    #[case(b"\xFF\xFE\x00\x01" as &[u8])]
    #[case(b"\x80\x81\x82\x83" as &[u8])]
    fn binary_file_detected_as_binary(#[case] content: &[u8]) {
        let dir = tempfile::tempdir().unwrap();
        let path = temp_file(dir.path(), "f.bin", content);
        let entry = process_file(path, "f.bin".into());
        assert!(
            matches!(entry.kind, FileKind::Binary),
            "expected Binary, got {:?}",
            entry.kind
        );
    }

    /// Text file content should be loaded into `entry.content`.
    #[test]
    fn text_content_is_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let path = temp_file(dir.path(), "hello.txt", b"hello world");
        let entry = process_file(path, "hello.txt".into());
        assert_eq!(entry.content.as_deref(), Some("hello world"));
    }

    /// Binary files must have `content == None`.
    #[test]
    fn binary_content_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = temp_file(dir.path(), "x.bin", b"\xFF\xFE");
        let entry = process_file(path, "x.bin".into());
        assert!(entry.content.is_none());
    }

    // ── walk_paths ─────────────────────────────────────────────────────────

    /// Walk a two-file directory; both files should appear.
    #[test]
    fn walk_finds_all_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"a").unwrap();
        std::fs::write(dir.path().join("b.txt"), b"b").unwrap();

        let opts = WalkOptions {
            no_ignore: true,
            depth: None,
        };
        let results = walk_paths(&[dir.path().to_path_buf()], &opts);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entries.len(), 2);
    }

    /// `--depth 1` should exclude files inside subdirectories.
    #[test]
    fn walk_respects_depth_limit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("top.txt"), b"top").unwrap();
        std::fs::write(dir.path().join("sub").join("deep.txt"), b"deep").unwrap();

        let opts = WalkOptions {
            no_ignore: true,
            depth: Some(1),
        };
        let results = walk_paths(&[dir.path().to_path_buf()], &opts);
        let entries = &results[0].entries;

        assert_eq!(entries.len(), 1);
        assert!(entries[0].rel_path.ends_with("top.txt"));
    }

    /// Relative paths stored in entries should not include the walk root prefix.
    #[test]
    fn walk_rel_paths_strip_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("readme.md"), b"# hi").unwrap();

        let opts = WalkOptions {
            no_ignore: true,
            depth: None,
        };
        let results = walk_paths(&[dir.path().to_path_buf()], &opts);

        let rel = &results[0].entries[0].rel_path;
        assert_eq!(rel.as_os_str(), "readme.md");
    }

    // ── deterministic sort order ──────────────────────────────────────────

    /// Entries are sorted by relative path regardless of filesystem order.
    #[test]
    fn walk_entries_sorted_by_rel_path() {
        let dir = tempfile::tempdir().unwrap();
        // Create files in reverse-alphabetical order.
        std::fs::write(dir.path().join("z.txt"), b"z").unwrap();
        std::fs::write(dir.path().join("a.txt"), b"a").unwrap();
        std::fs::write(dir.path().join("m.txt"), b"m").unwrap();

        let opts = WalkOptions {
            no_ignore: true,
            depth: None,
        };
        let results = walk_paths(&[dir.path().to_path_buf()], &opts);
        let names: Vec<&str> = results[0]
            .entries
            .iter()
            .map(|e| e.rel_path.to_str().unwrap())
            .collect();

        assert_eq!(names, vec!["a.txt", "m.txt", "z.txt"]);
    }

    /// Entries in subdirectories sort correctly with their full relative paths.
    #[test]
    fn walk_entries_sorted_across_subdirs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("b_dir")).unwrap();
        std::fs::create_dir(dir.path().join("a_dir")).unwrap();
        std::fs::write(dir.path().join("b_dir").join("file.txt"), b"b").unwrap();
        std::fs::write(dir.path().join("a_dir").join("file.txt"), b"a").unwrap();
        std::fs::write(dir.path().join("root.txt"), b"r").unwrap();

        let opts = WalkOptions {
            no_ignore: true,
            depth: None,
        };
        let results = walk_paths(&[dir.path().to_path_buf()], &opts);
        let names: Vec<&str> = results[0]
            .entries
            .iter()
            .map(|e| e.rel_path.to_str().unwrap())
            .collect();

        assert_eq!(names, vec!["a_dir/file.txt", "b_dir/file.txt", "root.txt"]);
    }

    /// Walk errors are collected, not silently discarded.
    #[test]
    fn walk_collects_errors() {
        // WalkResult should have an errors field (even if empty for valid dirs).
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ok.txt"), b"ok").unwrap();

        let opts = WalkOptions {
            no_ignore: true,
            depth: None,
        };
        let results = walk_paths(&[dir.path().to_path_buf()], &opts);

        assert!(results[0].errors.is_empty());
        assert_eq!(results[0].entries.len(), 1);
    }
}
