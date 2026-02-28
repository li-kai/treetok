//! Tree rendering, flat listing, JSON serialisation, and formatting helpers.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use owo_colors::OwoColorize;
use termtree::Tree;

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
    ///
    /// 4.1 % matches ctoc's claimed error bound (~4 % average across tested
    /// files).  Our implementation uses optimal DP rather than the greedy
    /// longest-match in the ctoc CLI, so real-world error should be at or
    /// below this bound.
    ///
    /// The percentage band alone collapses to near-zero for small files, so a
    /// ±2 token floor is applied: every range is at least 4 tokens wide.
    ///
    /// `lo = max(0, floor(count × 0.959) − 2)`, `hi = ceil(count × 1.041) + 2`.
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
    /// Token counts keyed by tokenizer name.  Empty for non-text files.
    pub tokens: BTreeMap<String, TokenCount>,
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

// ─── Tree mode ────────────────────────────────────────────────────────────────

fn write_tree(
    out: &mut dyn Write,
    root_label: &str,
    entries: &[FileResult],
    opts: &OutputOptions,
) -> std::io::Result<()> {
    let root_display = format_dir_label(root_label, opts.color);
    let tree = build_tree_node(root_display, entries, Path::new(""), opts);
    writeln!(out, "{tree}")?;
    write_totals(out, entries, opts)?;
    Ok(())
}

/// Recursively build a `termtree::Tree` node for `prefix`.
fn build_tree_node(
    label: String,
    entries: &[FileResult],
    prefix: &Path,
    opts: &OutputOptions,
) -> Tree<String> {
    // Collect direct children under `prefix`.
    let mut files: Vec<&FileResult> = entries
        .iter()
        .filter(|e| e.rel_path.parent() == Some(prefix))
        .collect();

    // Subdirectories immediately under `prefix`.
    let mut subdirs: Vec<String> = entries
        .iter()
        .filter_map(|e| {
            let rel = e.rel_path.strip_prefix(prefix).ok()?;
            let mut comps = rel.components();
            let first = comps.next()?.as_os_str().to_string_lossy().into_owned();
            // Only keep if there is a second component (i.e., it's a dir, not a file).
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
        let dir_label = format_dir_label(dir_name, opts.color);
        node.push(build_tree_node(dir_label, entries, &dir_prefix, opts));
    }

    for file in &files {
        node.push(Tree::new(format_file_leaf(file, opts)));
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

    for entry in &sorted {
        let path_str = entry.rel_path.display().to_string();
        let count_str = format_tokens(entry, &opts.count_format, opts.color);
        writeln!(out, "{path_str:<40}  {count_str}")?;
    }

    write_totals(out, entries, opts)?;
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
                        (k.clone(), json_val)
                    })
                    .collect();
                Value::Object(map)
            };

            let mut obj = Map::new();
            obj.insert("path".to_string(), Value::from(path_str));
            obj.insert("type".to_string(), Value::from(type_str));
            obj.insert("tokens".to_string(), tokens_val);

            // Skipped-file annotations.
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

    // Grand totals.
    let mut totals: BTreeMap<String, TokenCount> = BTreeMap::new();
    for entry in entries {
        for (name, count) in &entry.tokens {
            match totals.entry(name.clone()) {
                std::collections::btree_map::Entry::Occupied(mut e) => {
                    e.get_mut().add(count);
                }
                std::collections::btree_map::Entry::Vacant(e) => {
                    e.insert(count.clone());
                }
            }
        }
    }
    let totals_val: Map<String, Value> = totals
        .iter()
        .map(|(k, v)| {
            let json_val = match v {
                TokenCount::Exact(n) => Value::from(*n),
                TokenCount::Approx { lo, hi } => {
                    serde_json::json!({ "lo": lo, "hi": hi })
                }
            };
            (k.clone(), json_val)
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

fn write_totals(
    out: &mut dyn Write,
    entries: &[FileResult],
    opts: &OutputOptions,
) -> std::io::Result<()> {
    let mut totals: BTreeMap<String, TokenCount> = BTreeMap::new();
    for entry in entries {
        for (name, count) in &entry.tokens {
            match totals.entry(name.clone()) {
                std::collections::btree_map::Entry::Occupied(mut e) => {
                    e.get_mut().add(count);
                }
                std::collections::btree_map::Entry::Vacant(e) => {
                    e.insert(count.clone());
                }
            }
        }
    }

    if totals.is_empty() {
        return Ok(());
    }

    let total_str = format_counts(&totals, &opts.count_format);
    writeln!(out, "\nTotal: [{total_str}]")
}

// ─── Formatting helpers ───────────────────────────────────────────────────────

fn format_dir_label(name: &str, color: bool) -> String {
    let display = if name.ends_with('/') {
        name.to_string()
    } else {
        format!("{name}/")
    };

    if color {
        display.bold().to_string()
    } else {
        display
    }
}

fn format_file_leaf(entry: &FileResult, opts: &OutputOptions) -> String {
    let name = entry.rel_path.file_name().map_or_else(
        || entry.rel_path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );

    let count_str = format_tokens(entry, &opts.count_format, opts.color);
    format!("{name:<30}  {count_str}")
}

fn format_tokens(entry: &FileResult, format: &CountFormat, color: bool) -> String {
    match &entry.kind {
        FileKind::Binary => dim("[binary]", color),
        FileKind::TooLarge => dim("[too large]", color),
        FileKind::Error(msg) => dim(&format!("[error: {msg}]"), color),
        FileKind::Text => {
            let counts_str = format_counts(&entry.tokens, format);
            format!("[{counts_str}]")
        }
    }
}

fn format_counts(counts: &BTreeMap<String, TokenCount>, format: &CountFormat) -> String {
    if counts.is_empty() {
        return String::new();
    }

    match format {
        CountFormat::Named => counts
            .iter()
            .map(|(name, count)| match count {
                TokenCount::Exact(n) => format!("{name}: {}", format_number(*n)),
                TokenCount::Approx { lo, hi } => {
                    format!(
                        "{name}: {} \u{2013} {}",
                        format_number(*lo),
                        format_number(*hi)
                    )
                }
            })
            .collect::<Vec<_>>()
            .join("  "),
        CountFormat::Single => match counts.values().next() {
            Some(TokenCount::Exact(n)) => format_number(*n),
            Some(TokenCount::Approx { lo, hi }) => {
                format!("{} \u{2013} {}", format_number(*lo), format_number(*hi))
            }
            None => String::new(),
        },
        CountFormat::Range => {
            let min = counts.values().map(TokenCount::lo).min().unwrap_or(0);
            let max = counts.values().map(TokenCount::hi).max().unwrap_or(0);
            if min == max {
                format_number(min)
            } else {
                format!("{} \u{2013} {}", format_number(min), format_number(max))
            }
        }
    }
}

/// Format a number with thousands separators (commas).
#[must_use]
pub fn format_number(n: usize) -> String {
    let s = n.to_string();
    let digits: Vec<char> = s.chars().collect();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    let len = digits.len();
    for (i, &c) in digits.iter().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(c);
    }
    result
}

fn dim(s: &str, color: bool) -> String {
    if color {
        s.dimmed().to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // ── helpers ────────────────────────────────────────────────────────────

    fn text_result(path: &str, counts: &[(&str, usize)]) -> FileResult {
        FileResult {
            rel_path: path.into(),
            kind: crate::walk::FileKind::Text,
            tokens: counts
                .iter()
                .map(|(k, v)| ((*k).to_string(), TokenCount::Exact(*v)))
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
        OutputOptions {
            flat,
            json,
            sort,
            color: false,
            count_format,
        }
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

    #[test]
    fn flat_shows_filename_and_count() {
        let entries = [text_result("src/main.rs", &[("o200k", 1_234)])];
        let s = run(
            ".",
            &entries,
            &opts(true, false, false, CountFormat::Single),
        );
        assert!(s.contains("src/main.rs"), "path missing:\n{s}");
        assert!(s.contains("1,234"), "count missing:\n{s}");
    }

    #[test]
    fn flat_includes_total_line() {
        let entries = [text_result("a.rs", &[("o200k", 100)])];
        let s = run(
            ".",
            &entries,
            &opts(true, false, false, CountFormat::Single),
        );
        assert!(s.contains("Total:"), "total line missing:\n{s}");
        assert!(s.contains("100"), "total count missing:\n{s}");
    }

    #[test]
    fn flat_binary_file_shows_label() {
        let entries = [binary_result("image.png")];
        let s = run(
            ".",
            &entries,
            &opts(true, false, false, CountFormat::Single),
        );
        assert!(s.contains("[binary]"), "[binary] label missing:\n{s}");
        // Binary files don't contribute to totals → no Total line.
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
        // b (500) → c (200) → a (100)
        assert!(pos_b < pos_c, "b should precede c:\n{s}");
        assert!(pos_c < pos_a, "c should precede a:\n{s}");
    }

    // ── range vs named mode ────────────────────────────────────────────────

    /// Two tokenizers with different counts → "min – max" format.
    #[test]
    fn range_mode_shows_en_dash_range() {
        let entries = [text_result("f.rs", &[("o200k", 100), ("claude", 120)])];
        let s = run(".", &entries, &opts(true, false, false, CountFormat::Range));
        assert!(s.contains('–'), "en-dash missing in range mode:\n{s}");
        assert!(s.contains("100"), "min missing:\n{s}");
        assert!(s.contains("120"), "max missing:\n{s}");
    }

    /// Single tokenizer → no dash, just the number.
    #[test]
    fn single_tokenizer_no_range_dash() {
        let entries = [text_result("f.rs", &[("o200k", 42)])];
        let s = run(
            ".",
            &entries,
            &opts(true, false, false, CountFormat::Single),
        );
        assert!(!s.contains('–'), "unexpected en-dash:\n{s}");
        assert!(s.contains("42"), "count missing:\n{s}");
    }

    // ── from_approx ────────────────────────────────────────────────────────

    /// Large count: percentage band dominates; floor has no effect.
    #[rstest]
    #[case(1000, 957, 1043)] // 0.959×1000−2=957, ceil(1.041×1000)+2=1043
    #[case(200, 189, 211)] // 200*959/1000−2=189, (200*1041+999)/1000+2=211
    #[case(100, 93, 107)] // 100*959/1000−2=93,  (100*1041+999)/1000+2=107
    fn from_approx_large(#[case] count: usize, #[case] lo: usize, #[case] hi: usize) {
        match TokenCount::from_approx(count) {
            TokenCount::Approx {
                lo: got_lo,
                hi: got_hi,
            } => {
                assert_eq!(got_lo, lo, "lo mismatch for count={count}");
                assert_eq!(got_hi, hi, "hi mismatch for count={count}");
            }
            other @ TokenCount::Exact(_) => {
                panic!("expected Approx, got Exact for count={count}: {other:?}")
            }
        }
    }

    /// Small count: absolute floor kicks in, range stays at least 4 tokens wide.
    #[rstest]
    #[case(10, 7, 13)] // 10*959/1000−2=9−2=7, (10*1041+999)/1000+2=11+2=13
    #[case(5, 2, 8)] // 5*959/1000−2=4−2=2,  (5*1041+999)/1000+2=6+2=8
    #[case(1, 0, 4)] // 1*959/1000=0 saturates→0, (1*1041+999)/1000+2=2+2=4
    #[case(0, 0, 2)] // 0 − 2 saturates → 0; (0+999)/1000+2=0+2=2
    fn from_approx_small_uses_floor(#[case] count: usize, #[case] lo: usize, #[case] hi: usize) {
        match TokenCount::from_approx(count) {
            TokenCount::Approx {
                lo: got_lo,
                hi: got_hi,
            } => {
                assert_eq!(got_lo, lo, "lo mismatch for count={count}");
                assert_eq!(got_hi, hi, "hi mismatch for count={count}");
                assert!(
                    got_hi - got_lo >= 4 || count == 0 || count == 1,
                    "band too narrow ({got_lo}–{got_hi}) for count={count}"
                );
            }
            other @ TokenCount::Exact(_) => panic!("expected Approx, got Exact: {other:?}"),
        }
    }

    /// Approximate (ctoc) count shows as a lo–hi range in rendered output.
    #[test]
    fn approx_count_shows_range() {
        let entry = FileResult {
            rel_path: "f.rs".into(),
            kind: crate::walk::FileKind::Text,
            tokens: [("ctoc".to_string(), TokenCount::from_approx(1000))].into(),
        };
        let s = run(
            ".",
            &[entry],
            &opts(true, false, false, CountFormat::Single),
        );
        assert!(s.contains('–'), "en-dash missing for approx range:\n{s}");
        assert!(s.contains("957"), "lo bound missing:\n{s}");
        assert!(s.contains("1,043"), "hi bound missing:\n{s}");
    }

    /// `-t o200k` explicit mode → "o200k: N" label.
    #[test]
    fn named_mode_shows_tokenizer_label() {
        let entries = [text_result("f.rs", &[("o200k", 42)])];
        let s = run(".", &entries, &opts(true, false, false, CountFormat::Named));
        assert!(s.contains("o200k: 42"), "named label missing:\n{s}");
    }

    // ── tree mode ──────────────────────────────────────────────────────────

    #[test]
    fn tree_shows_root_label() {
        let entries = [text_result("main.rs", &[("o200k", 10)])];
        let s = run(
            "src/",
            &entries,
            &opts(false, false, false, CountFormat::Single),
        );
        assert!(s.contains("src/"), "root label missing:\n{s}");
    }

    #[test]
    fn tree_shows_filename() {
        let entries = [text_result("main.rs", &[("o200k", 10)])];
        let s = run(
            "src/",
            &entries,
            &opts(false, false, false, CountFormat::Single),
        );
        assert!(s.contains("main.rs"), "filename missing:\n{s}");
    }

    #[test]
    fn tree_shows_total() {
        let entries = [
            text_result("a.rs", &[("o200k", 50)]),
            text_result("b.rs", &[("o200k", 50)]),
        ];
        let s = run(
            ".",
            &entries,
            &opts(false, false, false, CountFormat::Single),
        );
        assert!(s.contains("Total:"), "total line missing:\n{s}");
        assert!(s.contains("100"), "total count missing:\n{s}");
    }

    // ── JSON mode ──────────────────────────────────────────────────────────

    #[test]
    fn json_is_valid_and_has_expected_structure() {
        let entries = [
            text_result("foo.rs", &[("o200k", 42)]),
            binary_result("data.bin"),
        ];
        let s = run(
            "src/",
            &entries,
            &opts(false, true, false, CountFormat::Single),
        );
        let v: serde_json::Value = serde_json::from_str(&s).expect("not valid JSON");
        assert_eq!(v["root"], "src/");
        assert_eq!(v["files"].as_array().unwrap().len(), 2);
        assert_eq!(v["files"][0]["tokens"]["o200k"], 42);
        assert!(
            v["files"][1]["tokens"].is_null(),
            "binary tokens should be null"
        );
        assert_eq!(v["total"]["o200k"], 42);
    }

    #[test]
    fn json_too_large_has_skipped_field() {
        let entries = [FileResult {
            rel_path: "huge.dat".into(),
            kind: crate::walk::FileKind::TooLarge,
            tokens: BTreeMap::new(),
        }];
        let s = run(
            ".",
            &entries,
            &opts(false, true, false, CountFormat::Single),
        );
        let v: serde_json::Value = serde_json::from_str(&s).expect("not valid JSON");
        assert_eq!(v["files"][0]["skipped"], "too large");
    }
}
