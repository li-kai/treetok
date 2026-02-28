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
    } else if matches!(opts.count_format, CountFormat::Named) {
        write_tree_named(out, root_label, entries, opts)
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
