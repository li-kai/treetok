//! Tree rendering, flat listing, JSON serialisation, and formatting helpers.

mod format;

pub use format::format_number;
use format::{format_counts, format_dir_label, format_named_columns, format_single_count,
             format_tokens};

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use crate::tokenize::TokenizerId;
use crate::tree::Tree;
use crate::walk::FileKind;

/// How to format token counts in output.
pub enum CountFormat {
    /// One tokenizer, no label — just "N"
    Single,
    /// Explicit `-t` names — "name: N  name: N"
    Named,
    /// Multiple tokenizers, auto mode — "min – max"
    Range,
}

/// Rendering configuration derived from CLI flags.
#[allow(clippy::struct_excessive_bools)]
pub struct OutputOptions {
    /// Show only a flat file list (no tree connectors).
    pub flat: bool,
    /// Emit JSON instead of human-readable text.
    pub json: bool,
    /// Sort file entries by descending max token count.
    pub sort: bool,
    /// When `true`, emit ANSI color codes.
    pub color: bool,
    /// How to format token counts.
    pub count_format: CountFormat,
}

impl OutputOptions {
    /// Create a new output configuration from CLI flags.
    #[allow(clippy::fn_params_excessive_bools)]
    #[must_use]
    pub fn new(
        flat: bool,
        json: bool,
        sort: bool,
        no_color: bool,
        count_format: CountFormat,
    ) -> Self {
        let color = !no_color && std::env::var("NO_COLOR").is_err() && {
            use std::io::IsTerminal;
            std::io::stdout().is_terminal()
        };
        Self {
            flat,
            json,
            sort,
            color,
            count_format,
        }
    }
}

/// A token count that is either exact or an approximate range.
#[derive(Clone, Debug)]
pub enum TokenCount {
    /// A precise count from an exact tokenizer.
    Exact(usize),
    /// A [lo, hi] inclusive range from an approximate tokenizer (e.g. ctoc ±5 %).
    Approx {
        /// Lower bound of the estimated range.
        lo: usize,
        /// Upper bound of the estimated range.
        hi: usize,
    },
}

impl TokenCount {
    /// The lower bound (or exact value).
    fn lo(&self) -> usize {
        match self {
            Self::Exact(n) | Self::Approx { lo: n, .. } => *n,
        }
    }

    /// The upper bound (or exact value).
    fn hi(&self) -> usize {
        match self {
            Self::Exact(n) | Self::Approx { hi: n, .. } => *n,
        }
    }

    /// Construct an approximate range for a raw count using ±4.1 % with a
    /// minimum absolute slack of 2 tokens on each side.
    #[must_use]
    pub fn from_approx(count: usize) -> Self {
        let lo = (count * 959 / 1000).saturating_sub(2);
        let hi = (count * 1041).div_ceil(1000) + 2;
        Self::Approx { lo, hi }
    }

    /// Accumulate another count into `self` (same variant assumed).
    fn add(&mut self, other: &Self) {
        match (self, other) {
            (Self::Exact(a), Self::Exact(b)) => *a += b,
            (Self::Approx { lo: alo, hi: ahi }, Self::Approx { lo: blo, hi: bhi }) => {
                *alo += blo;
                *ahi += bhi;
            }
            _ => {}
        }
    }
}

/// A single file entry with associated token counts.
pub struct FileResult {
    /// Path relative to the walk root.
    pub rel_path: std::path::PathBuf,
    /// Content kind (reuses `walk::FileKind`).
    pub kind: FileKind,
    /// Token counts keyed by tokenizer id.  Empty for non-text files.
    pub tokens: BTreeMap<TokenizerId, TokenCount>,
}

// ─── Public entry points ──────────────────────────────────────────────────────

/// Write the chosen output format for `root_label` + `entries` to `out`.
pub fn write_output(
    out: &mut dyn Write,
    root_label: &str,
    entries: &[FileResult],
    opts: &OutputOptions,
) -> std::io::Result<()> {
    if opts.json {
        write_json(out, root_label, entries)
    } else if opts.flat {
        write_flat(out, entries, opts)
    } else {
        write_tree(out, root_label, entries, opts)
    }
}

// ─── Sorting helper ──────────────────────────────────────────────────────────

fn sort_by_tokens(entries: &mut [&FileResult]) {
    entries.sort_by(|a, b| {
        let a_max = a.tokens.values().map(TokenCount::hi).max().unwrap_or(0);
        let b_max = b.tokens.values().map(TokenCount::hi).max().unwrap_or(0);
        b_max.cmp(&a_max).then_with(|| a.rel_path.cmp(&b.rel_path))
    });
}

// ─── Column-width helpers ─────────────────────────────────────────────────────

/// Sorted list of every tokenizer id present across all entries.
fn all_tokenizer_ids(entries: &[FileResult]) -> Vec<TokenizerId> {
    let mut ids: std::collections::BTreeSet<TokenizerId> = std::collections::BTreeSet::new();
    for e in entries {
        ids.extend(e.tokens.keys().copied());
    }
    ids.into_iter().collect()
}

/// For each tokenizer (in `ids` order), the maximum display width of its
/// formatted count string across all entries.  The width is also at least as
/// wide as the tokenizer's display name (for the header row).
fn max_count_widths(entries: &[FileResult], ids: &[TokenizerId]) -> Vec<usize> {
    let mut widths: Vec<usize> = ids.iter().map(|id| id.to_string().chars().count()).collect();
    for e in entries {
        for (i, id) in ids.iter().enumerate() {
            if let Some(tc) = e.tokens.get(id) {
                let w = format_single_count(tc).chars().count();
                widths[i] = widths[i].max(w);
            }
        }
    }
    widths
}

// ─── Tree node type ───────────────────────────────────────────────────────────

/// The label stored in every `Tree` node.
enum TreeNode {
    /// A directory label (e.g. `"src/"`).
    Dir(String),
    /// A file leaf: display name + pre-formatted count string.
    File {
        name: String,
        /// Formatted count block, e.g. `"[1,234]"` or `"[binary]"`.
        counts: String,
    },
}

impl std::fmt::Display for TreeNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dir(s) => write!(f, "{s}"),
            Self::File { name, counts } => write!(f, "{name:<30}  {counts}"),
        }
    }
}

// ─── Tree mode ────────────────────────────────────────────────────────────────

/// The column at which the count block should start in tree output.
///
/// Equals `max(prefix_width + filename_width) + 2` across all entries,
/// where `prefix_width = 4 × path-component-count` (each depth level adds
/// the 4-char connector `"├── "` / `"│   "`).
fn name_col_width(entries: &[FileResult]) -> usize {
    entries
        .iter()
        .map(|e| {
            let depth = e.rel_path.components().count();
            let prefix_w = 4 * depth;
            let name_w = e
                .rel_path
                .file_name()
                .map(|n| n.to_string_lossy().chars().count())
                .unwrap_or_else(|| e.rel_path.display().to_string().chars().count());
            prefix_w + name_w
        })
        .max()
        .unwrap_or(28)
        + 2
}

fn write_tree(
    out: &mut dyn Write,
    root_label: &str,
    entries: &[FileResult],
    opts: &OutputOptions,
) -> std::io::Result<()> {
    if matches!(opts.count_format, CountFormat::Named) {
        return write_tree_named(out, root_label, entries, opts);
    }

    let name_col = name_col_width(entries);
    let root_display = format_dir_label(root_label, opts.color);
    let tree = build_tree_node(
        TreeNode::Dir(root_display),
        entries,
        Path::new(""),
        opts,
        &|file| format_tokens(file, &opts.count_format, opts.color),
    );

    tree.render(out, &|out, prefix_width, node| match node {
        TreeNode::Dir(name) => write!(out, "{name}"),
        TreeNode::File { name, counts } => {
            let pad = name_col.saturating_sub(prefix_width + name.chars().count());
            write!(out, "{name}{:pad$}{counts}", "", pad = pad)
        }
    })?;

    write_totals(out, entries, opts)?;
    Ok(())
}

fn write_tree_named(
    out: &mut dyn Write,
    root_label: &str,
    entries: &[FileResult],
    opts: &OutputOptions,
) -> std::io::Result<()> {
    let ids = all_tokenizer_ids(entries);
    let widths = max_count_widths(entries, &ids);
    let name_col = name_col_width(entries);

    // Header row — blank padding to name_col, then right-aligned column labels.
    write!(out, "{:<name_col$}", "", name_col = name_col)?;
    for (id, w) in ids.iter().zip(&widths) {
        write!(out, "  {:>w$}", id.to_string().to_uppercase(), w = w)?;
    }
    writeln!(out)?;

    // Build and render the tree.
    let root_display = format_dir_label(root_label, opts.color);
    let tree = build_tree_node(
        TreeNode::Dir(root_display),
        entries,
        Path::new(""),
        opts,
        &|file| match &file.kind {
            FileKind::Text => format_named_columns(&file.tokens, &ids, &widths),
            _ => format_tokens(file, &CountFormat::Named, opts.color),
        },
    );

    tree.render(out, &|out, prefix_width, node| match node {
        TreeNode::Dir(name) => write!(out, "{name}"),
        TreeNode::File { name, counts } => {
            let pad = name_col.saturating_sub(prefix_width + name.chars().count());
            write!(out, "{name}{:pad$}{counts}", "", pad = pad)
        }
    })?;

    // Totals row.
    let mut totals: BTreeMap<TokenizerId, TokenCount> = BTreeMap::new();
    accumulate_totals(entries, &mut totals);
    if !totals.is_empty() {
        write!(out, "\n{:<name_col$}", "TOTAL", name_col = name_col)?;
        for (id, w) in ids.iter().zip(&widths) {
            let cell = totals.get(id).map(format_single_count).unwrap_or_default();
            write!(out, "  {:>w$}", cell, w = w)?;
        }
        writeln!(out)?;
    }

    Ok(())
}

/// Recursively build a `Tree<TreeNode>` for `prefix`.
///
/// `fmt_counts` is called for every file leaf to produce the pre-formatted
/// count string stored in `TreeNode::File::counts`.
fn build_tree_node(
    label: TreeNode,
    entries: &[FileResult],
    prefix: &Path,
    opts: &OutputOptions,
    fmt_counts: &dyn Fn(&FileResult) -> String,
) -> Tree<TreeNode> {
    let mut files: Vec<&FileResult> = entries
        .iter()
        .filter(|e| e.rel_path.parent() == Some(prefix))
        .collect();

    let mut subdirs: Vec<String> = entries
        .iter()
        .filter_map(|e| {
            let rel = e.rel_path.strip_prefix(prefix).ok()?;
            let mut comps = rel.components();
            let first = comps.next()?.as_os_str().to_string_lossy().into_owned();
            comps.next()?;
            Some(first)
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    if opts.sort {
        sort_by_tokens(&mut files);
        subdirs.sort();
    }

    let mut node = Tree::new(label);

    for dir_name in &subdirs {
        let dir_prefix = prefix.join(dir_name);
        let dir_label = TreeNode::Dir(format_dir_label(dir_name, opts.color));
        node.push(build_tree_node(dir_label, entries, &dir_prefix, opts, fmt_counts));
    }

    for file in &files {
        let name = file
            .rel_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| file.rel_path.display().to_string());
        let counts = fmt_counts(file);
        node.push(Tree::new(TreeNode::File { name, counts }));
    }

    node
}

// ─── Flat mode ────────────────────────────────────────────────────────────────

fn write_flat(
    out: &mut dyn Write,
    entries: &[FileResult],
    opts: &OutputOptions,
) -> std::io::Result<()> {
    let mut sorted: Vec<&FileResult> = entries.iter().collect();
    if opts.sort {
        sort_by_tokens(&mut sorted);
    }

    let path_w = sorted
        .iter()
        .map(|e| e.rel_path.display().to_string().chars().count())
        .max()
        .unwrap_or(0)
        .max(4); // at least wide enough for "PATH"

    match &opts.count_format {
        // Named format: emit a proper multi-column table with a header row.
        CountFormat::Named => {
            let ids = all_tokenizer_ids(entries);
            let widths = max_count_widths(entries, &ids);

            // Header.
            write!(out, "{:<path_w$}", "PATH", path_w = path_w)?;
            for (id, w) in ids.iter().zip(&widths) {
                write!(out, "  {:>w$}", id.to_string().to_uppercase(), w = w)?;
            }
            writeln!(out)?;

            // Rows.
            for entry in &sorted {
                let path_str = entry.rel_path.display().to_string();
                match &entry.kind {
                    FileKind::Text => {
                        write!(out, "{path_str:<path_w$}", path_w = path_w)?;
                        for (id, w) in ids.iter().zip(&widths) {
                            let cell = entry
                                .tokens
                                .get(id)
                                .map(format_single_count)
                                .unwrap_or_default();
                            write!(out, "  {:>w$}", cell, w = w)?;
                        }
                        writeln!(out)?;
                    }
                    _ => {
                        let label = format_tokens(entry, &opts.count_format, opts.color);
                        writeln!(out, "{path_str:<path_w$}  {label}", path_w = path_w)?;
                    }
                }
            }

            // Totals row.
            let mut totals: BTreeMap<TokenizerId, TokenCount> = BTreeMap::new();
            accumulate_totals(entries, &mut totals);
            if !totals.is_empty() {
                write!(out, "\n{:<path_w$}", "TOTAL", path_w = path_w)?;
                for (id, w) in ids.iter().zip(&widths) {
                    let cell = totals
                        .get(id)
                        .map(format_single_count)
                        .unwrap_or_default();
                    write!(out, "  {:>w$}", cell, w = w)?;
                }
                writeln!(out)?;
            }
        }

        // Single / Range: align the count block start to a fixed column.
        _ => {
            for entry in &sorted {
                let path_str = entry.rel_path.display().to_string();
                let count_str = format_tokens(entry, &opts.count_format, opts.color);
                writeln!(out, "{path_str:<path_w$}  {count_str}", path_w = path_w)?;
            }
            write_totals(out, entries, opts)?;
        }
    }

    Ok(())
}

// ─── JSON mode ────────────────────────────────────────────────────────────────

fn write_json(
    out: &mut dyn Write,
    root_label: &str,
    entries: &[FileResult],
) -> std::io::Result<()> {
    use serde_json::{Map, Value};

    let files: Vec<Value> = entries
        .iter()
        .map(|e| {
            let path_str = e.rel_path.display().to_string();
            let type_str = match &e.kind {
                FileKind::Text => "text",
                FileKind::Binary => "binary",
                FileKind::TooLarge => "too_large",
                FileKind::Error(_) => "error",
            };

            let tokens_val: Value = if e.tokens.is_empty() {
                Value::Null
            } else {
                let map: Map<String, Value> = e
                    .tokens
                    .iter()
                    .map(|(k, v)| {
                        let json_val = match v {
                            TokenCount::Exact(n) => Value::from(*n),
                            TokenCount::Approx { lo, hi } => {
                                serde_json::json!({ "lo": lo, "hi": hi })
                            }
                        };
                        (k.as_str().to_string(), json_val)
                    })
                    .collect();
                Value::Object(map)
            };

            let mut obj = Map::new();
            obj.insert("path".to_string(), Value::from(path_str));
            obj.insert("type".to_string(), Value::from(type_str));
            obj.insert("tokens".to_string(), tokens_val);

            match &e.kind {
                FileKind::TooLarge => {
                    obj.insert("skipped".to_string(), Value::from("too large"));
                }
                FileKind::Error(msg) => {
                    obj.insert("skipped".to_string(), Value::from(msg.as_str()));
                }
                _ => {}
            }

            Value::Object(obj)
        })
        .collect();

    let mut totals: BTreeMap<TokenizerId, TokenCount> = BTreeMap::new();
    accumulate_totals(entries, &mut totals);
    let totals_val: Map<String, Value> = totals
        .iter()
        .map(|(k, v)| {
            let json_val = match v {
                TokenCount::Exact(n) => Value::from(*n),
                TokenCount::Approx { lo, hi } => {
                    serde_json::json!({ "lo": lo, "hi": hi })
                }
            };
            (k.as_str().to_string(), json_val)
        })
        .collect();

    let output = serde_json::json!({
        "root": root_label,
        "files": files,
        "total": Value::Object(totals_val),
    });

    let json_str =
        serde_json::to_string_pretty(&output).map_err(|e| std::io::Error::other(e.to_string()))?;

    writeln!(out, "{json_str}")
}

// ─── Totals ───────────────────────────────────────────────────────────────────

fn accumulate_totals(entries: &[FileResult], totals: &mut BTreeMap<TokenizerId, TokenCount>) {
    for entry in entries {
        for (name, count) in &entry.tokens {
            match totals.entry(*name) {
                std::collections::btree_map::Entry::Occupied(mut e) => {
                    e.get_mut().add(count);
                }
                std::collections::btree_map::Entry::Vacant(e) => {
                    e.insert(count.clone());
                }
            }
        }
    }
}

fn write_totals(
    out: &mut dyn Write,
    entries: &[FileResult],
    opts: &OutputOptions,
) -> std::io::Result<()> {
    let mut totals: BTreeMap<TokenizerId, TokenCount> = BTreeMap::new();
    accumulate_totals(entries, &mut totals);

    if totals.is_empty() {
        return Ok(());
    }

    let total_str = format_counts(&totals, &opts.count_format);
    writeln!(out, "\nTotal: [{total_str}]")
}

#[cfg(test)]
mod tests {
    use rstest::{fixture, rstest};

    use super::*;
    use crate::tokenize::TokenizerId;

    // ── fixtures and helpers ───────────────────────────────────────────────

    fn text_result(path: &str, counts: &[(&str, usize)]) -> FileResult {
        FileResult {
            rel_path: path.into(),
            kind: crate::walk::FileKind::Text,
            tokens: counts
                .iter()
                .map(|(k, v)| (k.parse::<TokenizerId>().unwrap(), TokenCount::Exact(*v)))
                .collect(),
        }
    }

    fn binary_result(path: &str) -> FileResult {
        FileResult {
            rel_path: path.into(),
            kind: crate::walk::FileKind::Binary,
            tokens: BTreeMap::new(),
        }
    }

    fn opts(flat: bool, json: bool, sort: bool, count_format: CountFormat) -> OutputOptions {
        OutputOptions { flat, json, sort, color: false, count_format }
    }

    /// Flat, single-tokenizer, unsorted — the most common test configuration.
    #[fixture]
    fn flat_opts() -> OutputOptions {
        opts(true, false, false, CountFormat::Single)
    }

    /// Tree, single-tokenizer, unsorted.
    #[fixture]
    fn tree_opts() -> OutputOptions {
        opts(false, false, false, CountFormat::Single)
    }

    /// JSON, single-tokenizer.
    #[fixture]
    fn json_opts() -> OutputOptions {
        opts(false, true, false, CountFormat::Single)
    }

    fn run(root: &str, entries: &[FileResult], o: &OutputOptions) -> String {
        let mut out: Vec<u8> = Vec::new();
        write_output(&mut out, root, entries, o).unwrap();
        String::from_utf8(out).unwrap()
    }

    // ── format_number ──────────────────────────────────────────────────────

    #[rstest]
    #[case(0, "0")]
    #[case(9, "9")]
    #[case(999, "999")]
    #[case(1_000, "1,000")]
    #[case(1_234, "1,234")]
    #[case(12_345, "12,345")]
    #[case(123_456, "123,456")]
    #[case(1_000_000, "1,000,000")]
    #[case(1_234_567, "1,234,567")]
    #[case(10_000_000, "10,000,000")]
    fn format_number_inserts_commas(#[case] n: usize, #[case] expected: &str) {
        assert_eq!(format_number(n), expected);
    }

    // ── flat mode ──────────────────────────────────────────────────────────

    #[rstest]
    fn flat_shows_filename_and_count(flat_opts: OutputOptions) {
        let entries = [text_result("src/main.rs", &[("o200k", 1_234)])];
        let s = run(".", &entries, &flat_opts);
        assert!(s.contains("src/main.rs"), "path missing:\n{s}");
        assert!(s.contains("1,234"), "count missing:\n{s}");
    }

    #[rstest]
    fn flat_includes_total_line(flat_opts: OutputOptions) {
        let entries = [text_result("a.rs", &[("o200k", 100)])];
        let s = run(".", &entries, &flat_opts);
        assert!(s.contains("Total:"), "total line missing:\n{s}");
        assert!(s.contains("100"), "total count missing:\n{s}");
    }

    #[rstest]
    fn flat_binary_file_shows_label(flat_opts: OutputOptions) {
        let entries = [binary_result("image.png")];
        let s = run(".", &entries, &flat_opts);
        assert!(s.contains("[binary]"), "[binary] label missing:\n{s}");
        assert!(!s.contains("Total:"), "unexpected Total line:\n{s}");
    }

    #[test]
    fn flat_sort_orders_by_count_descending() {
        let entries = [
            text_result("a.rs", &[("o200k", 100)]),
            text_result("b.rs", &[("o200k", 500)]),
            text_result("c.rs", &[("o200k", 200)]),
        ];
        let s = run(".", &entries, &opts(true, false, true, CountFormat::Single));
        let pos_a = s.find("a.rs").unwrap();
        let pos_b = s.find("b.rs").unwrap();
        let pos_c = s.find("c.rs").unwrap();
        assert!(pos_b < pos_c, "b should precede c:\n{s}");
        assert!(pos_c < pos_a, "c should precede a:\n{s}");
    }

    // ── flat Named tabular mode ────────────────────────────────────────────

    #[test]
    fn flat_named_has_header_row() {
        let entries = [text_result("f.rs", &[("claude", 100), ("o200k", 120)])];
        let s = run(".", &entries, &opts(true, false, false, CountFormat::Named));
        assert!(s.contains("CLAUDE"), "CLAUDE header missing:\n{s}");
        assert!(s.contains("OPENAI"), "OPENAI header missing:\n{s}");
    }

    #[test]
    fn flat_named_has_total_row() {
        let entries = [
            text_result("a.rs", &[("o200k", 100)]),
            text_result("b.rs", &[("o200k", 200)]),
        ];
        let s = run(".", &entries, &opts(true, false, false, CountFormat::Named));
        assert!(s.contains("TOTAL"), "TOTAL row missing:\n{s}");
        assert!(s.contains("300"), "total count missing:\n{s}");
    }

    #[test]
    fn flat_named_columns_are_right_aligned() {
        // "1,234" is wider than "42"; the narrower value should gain leading spaces.
        let entries = [
            text_result("a.rs", &[("o200k", 42)]),
            text_result("b.rs", &[("o200k", 1_234)]),
        ];
        let s = run(".", &entries, &opts(true, false, false, CountFormat::Named));
        assert!(s.contains("   42") || s.contains("  42"), "right-align missing:\n{s}");
    }

    // ── range mode ─────────────────────────────────────────────────────────

    #[test]
    fn range_mode_shows_en_dash_range() {
        let entries = [text_result("f.rs", &[("o200k", 100), ("claude", 120)])];
        let s = run(".", &entries, &opts(true, false, false, CountFormat::Range));
        assert!(s.contains('–'), "en-dash missing:\n{s}");
        assert!(s.contains("100"), "min missing:\n{s}");
        assert!(s.contains("120"), "max missing:\n{s}");
        assert!(!s.contains('~'), "unexpected tilde for exact counts:\n{s}");
    }

    #[test]
    fn range_mode_approx_shows_tilde_on_max() {
        let entry = FileResult {
            rel_path: "f.rs".into(),
            kind: crate::walk::FileKind::Text,
            tokens: [(TokenizerId::Ctoc, TokenCount::from_approx(1000))].into(),
        };
        let s = run(".", &[entry], &opts(true, false, false, CountFormat::Range));
        assert!(s.contains("957"), "lo bound missing:\n{s}");
        assert!(s.contains("~1,043"), "tilde+hi bound missing:\n{s}");
        assert!(s.contains('–'), "en-dash missing:\n{s}");
    }

    #[test]
    fn range_mode_mixed_exact_and_approx_shows_tilde() {
        let entry = FileResult {
            rel_path: "f.rs".into(),
            kind: crate::walk::FileKind::Text,
            tokens: [
                (TokenizerId::Ctoc, TokenCount::from_approx(120)),
                (TokenizerId::O200k, TokenCount::Exact(100)),
            ]
            .into(),
        };
        let s = run(".", &[entry], &opts(true, false, false, CountFormat::Range));
        assert!(s.contains("100"), "min missing:\n{s}");
        assert!(s.contains("~127"), "tilde+max missing:\n{s}");
    }

    #[rstest]
    fn single_tokenizer_no_range_dash(flat_opts: OutputOptions) {
        let entries = [text_result("f.rs", &[("o200k", 42)])];
        let s = run(".", &entries, &flat_opts);
        assert!(!s.contains('–'), "unexpected en-dash:\n{s}");
        assert!(s.contains("42"), "count missing:\n{s}");
    }

    #[rstest]
    fn approx_count_shows_range(flat_opts: OutputOptions) {
        let entry = FileResult {
            rel_path: "f.rs".into(),
            kind: crate::walk::FileKind::Text,
            tokens: [(TokenizerId::Ctoc, TokenCount::from_approx(1000))].into(),
        };
        let s = run(".", &[entry], &flat_opts);
        assert!(s.contains('–'), "en-dash missing for approx range:\n{s}");
        assert!(s.contains("957"), "lo bound missing:\n{s}");
        assert!(s.contains("1,043"), "hi bound missing:\n{s}");
    }

    #[test]
    fn named_mode_shows_tokenizer_label() {
        let entries = [text_result("f.rs", &[("o200k", 42)])];
        let s = run(".", &entries, &opts(true, false, false, CountFormat::Named));
        assert!(s.contains("42"), "count missing:\n{s}");
        assert!(s.contains("OPENAI") || s.contains("OpenAI"), "label missing:\n{s}");
    }

    // ── tree Named tabular mode ────────────────────────────────────────────

    #[test]
    fn tree_named_tabular_has_header_columns_and_totals() {
        let entries = [
            text_result("src/main.rs", &[("o200k", 1_234), ("claude", 1_180)]),
            text_result("src/lib.rs", &[("o200k", 456), ("claude", 420)]),
        ];
        let s = run(".", &entries, &opts(false, false, false, CountFormat::Named));
        assert!(s.contains("OPENAI"), "OPENAI header missing:\n{s}");
        assert!(s.contains("CLAUDE"), "CLAUDE header missing:\n{s}");
        assert!(s.contains("TOTAL"), "TOTAL row missing:\n{s}");
        assert!(s.contains("1,234"), "count 1,234 missing:\n{s}");
        assert!(s.contains("1,180"), "count 1,180 missing:\n{s}");
        // Totals: 1_234 + 456 = 1_690 and 1_180 + 420 = 1_600
        assert!(s.contains("1,690"), "o200k total missing:\n{s}");
        assert!(s.contains("1,600"), "claude total missing:\n{s}");
    }

    #[test]
    fn tree_named_tabular_columns_aligned() {
        // Files at different depths — right-aligned columns mean every file
        // row has the same total display width (right edge is fixed).
        let entries = [
            text_result("top.rs", &[("o200k", 1_000)]),
            text_result("sub/deep.rs", &[("o200k", 999)]),
        ];
        let s = run(".", &entries, &opts(false, false, false, CountFormat::Named));
        // Use char count (not byte length): box-drawing glyphs are 1 display
        // column but 3 bytes, so chars().count() gives the display width.
        let line_lengths: Vec<usize> = s
            .lines()
            .filter(|l| l.contains(".rs"))
            .map(|l| l.chars().count())
            .collect();
        assert_eq!(line_lengths.len(), 2, "expected 2 file lines:\n{s}");
        assert_eq!(line_lengths[0], line_lengths[1], "right edges not aligned:\n{s}");
    }

    // ── from_approx ────────────────────────────────────────────────────────

    #[rstest]
    // Large: percentage band dominates
    #[case(1000, 957, 1043)]
    #[case(200,  189, 211)]
    #[case(100,   93, 107)]
    // Small: absolute ±2 floor keeps band ≥ 4 tokens wide
    #[case(10,    7,  13)]
    #[case(5,     2,   8)]
    #[case(1,     0,   4)]
    #[case(0,     0,   2)]
    fn from_approx_bounds(#[case] count: usize, #[case] lo: usize, #[case] hi: usize) {
        let TokenCount::Approx { lo: got_lo, hi: got_hi } = TokenCount::from_approx(count) else {
            panic!("expected Approx for count={count}");
        };
        assert_eq!(got_lo, lo, "lo mismatch for count={count}");
        assert_eq!(got_hi, hi, "hi mismatch for count={count}");
        assert!(
            got_hi - got_lo >= 4 || count == 0,
            "band too narrow ({got_lo}–{got_hi}) for count={count}",
        );
    }

    // ── tree mode ──────────────────────────────────────────────────────────

    #[rstest]
    fn tree_shows_root_and_filename(tree_opts: OutputOptions) {
        let entries = [text_result("main.rs", &[("o200k", 10)])];
        let s = run("src/", &entries, &tree_opts);
        assert!(s.contains("src/"), "root label missing:\n{s}");
        assert!(s.contains("main.rs"), "filename missing:\n{s}");
    }

    #[rstest]
    fn tree_shows_total(tree_opts: OutputOptions) {
        let entries = [
            text_result("a.rs", &[("o200k", 50)]),
            text_result("b.rs", &[("o200k", 50)]),
        ];
        let s = run(".", &entries, &tree_opts);
        assert!(s.contains("Total:"), "total line missing:\n{s}");
        assert!(s.contains("100"), "total count missing:\n{s}");
    }

    #[rstest]
    fn tree_counts_aligned_across_depths(tree_opts: OutputOptions) {
        let entries = [
            text_result("top.rs", &[("o200k", 1_000)]),
            text_result("sub/deep.rs", &[("o200k", 999)]),
        ];
        let s = run(".", &entries, &tree_opts);
        // Use char count, not byte offset: box-drawing glyphs (│ └ ─) are
        // each 3 bytes but 1 display column.
        let bracket_cols: Vec<usize> = s
            .lines()
            .filter(|l| l.contains(".rs"))
            .map(|l| l.chars().take_while(|&c| c != '[').count())
            .collect();
        assert_eq!(bracket_cols.len(), 2, "expected 2 file lines:\n{s}");
        assert_eq!(bracket_cols[0], bracket_cols[1], "count brackets not aligned:\n{s}");
    }

    // ── JSON mode ──────────────────────────────────────────────────────────

    #[rstest]
    fn json_is_valid_and_has_expected_structure(json_opts: OutputOptions) {
        let entries = [
            text_result("foo.rs", &[("o200k", 42)]),
            binary_result("data.bin"),
        ];
        let s = run("src/", &entries, &json_opts);
        let v: serde_json::Value = serde_json::from_str(&s).expect("not valid JSON");
        assert_eq!(v["root"], "src/");
        assert_eq!(v["files"].as_array().unwrap().len(), 2);
        assert_eq!(v["files"][0]["tokens"]["o200k"], 42);
        assert!(v["files"][1]["tokens"].is_null(), "binary tokens should be null");
        assert_eq!(v["total"]["o200k"], 42);
    }

    #[rstest]
    fn json_too_large_has_skipped_field(json_opts: OutputOptions) {
        let entries = [FileResult {
            rel_path: "huge.dat".into(),
            kind: crate::walk::FileKind::TooLarge,
            tokens: BTreeMap::new(),
        }];
        let s = run(".", &entries, &json_opts);
        let v: serde_json::Value = serde_json::from_str(&s).expect("not valid JSON");
        assert_eq!(v["files"][0]["skipped"], "too large");
    }
}
