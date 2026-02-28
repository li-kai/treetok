//! Pure formatting helpers for token counts, directory labels, and file rows.

use std::collections::BTreeMap;

use owo_colors::OwoColorize;

use crate::tokenize::TokenizerId;
use crate::walk::FileKind;

use super::{CountFormat, FileResult, TokenCount};

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

/// Format a single `TokenCount` as a plain string (no label, no brackets).
pub(super) fn format_single_count(tc: &TokenCount) -> String {
    match tc {
        TokenCount::Exact(n) => format_number(*n),
        TokenCount::Approx { lo, hi } => {
            format!("{} \u{2013} ~{}", format_number(*lo), format_number(*hi))
        }
    }
}

/// Build the pre-formatted column string for a Named tabular file row.
///
/// Each column is `"  " + right-aligned count`, matching the header layout
/// produced by `write_tree_named` and the flat Named table.
pub(super) fn format_named_columns(
    tokens: &BTreeMap<TokenizerId, TokenCount>,
    ids: &[TokenizerId],
    widths: &[usize],
) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    for (id, w) in ids.iter().zip(widths) {
        let cell = tokens.get(id).map(format_single_count).unwrap_or_default();
        write!(s, "  {:>w$}", cell, w = *w).unwrap();
    }
    s
}

pub(super) fn format_counts(
    counts: &BTreeMap<TokenizerId, TokenCount>,
    format: &CountFormat,
) -> String {
    if counts.is_empty() {
        return String::new();
    }
    match format {
        CountFormat::Named => counts
            .iter()
            .map(|(name, count)| match count {
                TokenCount::Exact(n) => format!("{name}: {}", format_number(*n)),
                TokenCount::Approx { lo, hi } => {
                    format!("{name}: {} \u{2013} {}", format_number(*lo), format_number(*hi))
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
            let approx = counts.values().any(|tc| matches!(tc, TokenCount::Approx { .. }));
            let tilde = if approx { "~" } else { "" };
            if min == max {
                format!("{tilde}{}", format_number(min))
            } else {
                format!("{} \u{2013} {tilde}{}", format_number(min), format_number(max))
            }
        }
    }
}

pub(super) fn format_tokens(entry: &FileResult, format: &CountFormat, color: bool) -> String {
    match &entry.kind {
        FileKind::Binary => dim("[binary]", color),
        FileKind::TooLarge => dim("[too large]", color),
        FileKind::Error(msg) => dim(&format!("[error: {msg}]"), color),
        FileKind::Text => format!("[{}]", format_counts(&entry.tokens, format)),
    }
}

pub(super) fn format_dir_label(name: &str, color: bool) -> String {
    let display = if name.ends_with('/') {
        name.to_string()
    } else {
        format!("{name}/")
    };
    if color { display.bold().to_string() } else { display }
}

fn dim(s: &str, color: bool) -> String {
    if color { s.dimmed().to_string() } else { s.to_string() }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use rstest::{fixture, rstest};

    use crate::tokenize::TokenizerId;
    use super::format_number;
    use super::super::{CountFormat, FileResult, OutputOptions, TokenCount, write_output};

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
