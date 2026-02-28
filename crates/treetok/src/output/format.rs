//! Pure formatting helpers for token counts, directory labels, and file rows.

use std::collections::BTreeMap;

use owo_colors::OwoColorize;

use crate::tokenize::TokenizerId;
use crate::walk::FileKind;

use super::{CountFormat, FileResult, TokenCount};

/// Layout info for one Named tabular column, tracking lo/hi sub-widths
/// so that approximate ranges can be sub-aligned across rows.
#[derive(Default)]
pub(super) struct ColLayout {
    /// Max char-width of the lo part (or exact value) across all rows.
    pub lo_w: usize,
    /// Max char-width of the hi part across all rows. 0 if column has no Approx values.
    pub hi_w: usize,
}

impl ColLayout {
    /// Total display width of the widest cell.
    pub fn total_width(&self) -> usize {
        if self.hi_w > 0 {
            self.lo_w + 3 + self.hi_w // 3 for " – "
        } else {
            self.lo_w
        }
    }
}

/// Format a single `TokenCount` for a Named tabular cell, aligning lo/hi parts independently.
pub(super) fn format_count_cell(tc: &TokenCount, layout: &ColLayout) -> String {
    if layout.hi_w > 0 {
        // Column has approx values — sub-align
        match tc {
            TokenCount::Approx { lo, hi } => {
                let lo_s = format_number(*lo);
                let hi_s = format_number(*hi);
                format!(
                    "{:>lo_w$} \u{2013} {:>hi_w$}",
                    lo_s,
                    hi_s,
                    lo_w = layout.lo_w,
                    hi_w = layout.hi_w
                )
            }
            TokenCount::Exact(n) => {
                // Right-align exact value to total column width
                let s = format_number(*n);
                format!("{:>w$}", s, w = layout.total_width())
            }
        }
    } else {
        let s = format_single_count(tc);
        format!("{:>w$}", s, w = layout.total_width())
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

/// Format a single `TokenCount` as a plain string (no label, no brackets).
pub(super) fn format_single_count(tc: &TokenCount) -> String {
    match tc {
        TokenCount::Exact(n) => format_number(*n),
        TokenCount::Approx { lo, hi } => {
            format!("{} \u{2013} {}", format_number(*lo), format_number(*hi))
        }
    }
}

/// Build the pre-formatted column string for a Named tabular row.
///
/// Each column is `"  " + sub-aligned count`, matching the header layout.
/// Works for both data rows and the TOTAL row.
pub(super) fn format_named_columns(
    tokens: &BTreeMap<TokenizerId, TokenCount>,
    ids: &[TokenizerId],
    layouts: &[ColLayout],
) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    for (id, layout) in ids.iter().zip(layouts) {
        let cell = tokens.get(id).map_or_else(
            || format!("{:>w$}", "", w = layout.total_width()),
            |tc| format_count_cell(tc, layout),
        );
        let _ = write!(s, "  {cell}");
    }
    s
}

/// Build the header column string for a Named tabular table.
///
/// Each column is `"  " + right-aligned tokenizer label`.
pub(super) fn format_named_header(ids: &[TokenizerId], layouts: &[ColLayout]) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    for (id, layout) in ids.iter().zip(layouts) {
        let _ = write!(s, "  {:>w$}", id, w = layout.total_width());
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
            let approx = counts
                .values()
                .any(|tc| matches!(tc, TokenCount::Approx { .. }));
            let tilde = if approx { "~" } else { "" };
            if min == max {
                format!("{tilde}{}", format_number(min))
            } else {
                format!(
                    "{} \u{2013} {tilde}{}",
                    format_number(min),
                    format_number(max)
                )
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
    if color {
        display.bold().to_string()
    } else {
        display
    }
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
    use std::collections::BTreeMap;

    use rstest::{fixture, rstest};

    use super::super::{CountFormat, FileResult, OutputOptions, TokenCount, write_output};
    use super::format_number;
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

    /// One file with approx (Ctoc) and exact (O200k) columns — used by alignment tests.
    fn approx_entry() -> FileResult {
        FileResult {
            rel_path: "f.rs".into(),
            kind: crate::walk::FileKind::Text,
            tokens: [
                (TokenizerId::Ctoc, TokenCount::from_approx(6_000)),
                (TokenizerId::O200k, TokenCount::Exact(4_754)),
            ]
            .into(),
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
        assert!(s.contains("Claude"), "Claude header missing:\n{s}");
        assert!(s.contains("OpenAI"), "OpenAI header missing:\n{s}");
    }

    #[test]
    fn flat_named_has_total_row() {
        let entries = [
            text_result("a.rs", &[("o200k", 100)]),
            text_result("b.rs", &[("o200k", 200)]),
        ];
        let s = run(".", &entries, &opts(true, false, false, CountFormat::Named));
        assert!(s.contains("Total"), "Total row missing:\n{s}");
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
        assert!(
            s.contains("   42") || s.contains("  42"),
            "right-align missing:\n{s}"
        );
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
        assert!(s.contains("OpenAI"), "label missing:\n{s}");
    }

    // ── tree Named tabular mode ────────────────────────────────────────────

    /// Return the 0-based **display column** (char index) of the first
    /// occurrence of `needle` in `haystack`, or `None` if absent.
    fn char_col(haystack: &str, needle: &str) -> Option<usize> {
        let byte_pos = haystack.find(needle)?;
        Some(haystack[..byte_pos].chars().count())
    }

    /// End column (exclusive) of `needle` in `haystack`, counted in chars.
    fn char_end_col(haystack: &str, needle: &str) -> Option<usize> {
        Some(char_col(haystack, needle)? + needle.chars().count())
    }

    #[test]
    fn tree_named_header_labels_right_aligned() {
        // Right-aligned header labels must share the same right edge as the
        // right-aligned data values in each column.  The en-dash in approx
        // ranges is multi-byte (3 bytes) but single-char, so alignment must
        // use char counts, not byte offsets.
        let entries = [approx_entry()];
        let s = run(
            ".",
            &entries,
            &opts(false, false, false, CountFormat::Named),
        );

        let header = s.lines().next().unwrap();
        let data_row = s.lines().find(|l| l.contains("f.rs")).unwrap();

        // First column: "Claude~" right edge == "6,248" right edge.
        let h1 = char_end_col(header, "Claude~").expect("Claude~ not in header");
        let d1 = char_end_col(data_row, "6,248").expect("6,248 not in data row");
        assert_eq!(
            h1, d1,
            "first column right edges differ (header {h1} vs data {d1}).\n\
             header:   {header}\n\
             data row: {data_row}"
        );

        // Second column: "OpenAI" right edge == "4,754" right edge.
        let h2 = char_end_col(header, "OpenAI").expect("OpenAI not in header");
        let d2 = char_end_col(data_row, "4,754").expect("4,754 not in data row");
        assert_eq!(
            h2, d2,
            "second column right edges differ (header {h2} vs data {d2}).\n\
             header:   {header}\n\
             data row: {data_row}"
        );
    }

    #[test]
    fn flat_named_header_labels_right_aligned() {
        // Same right-alignment check for flat output.
        let entries = [approx_entry()];
        let s = run(".", &entries, &opts(true, false, false, CountFormat::Named));

        let header = s.lines().next().unwrap();
        let data_row = s.lines().find(|l| l.contains("f.rs")).unwrap();

        let h1 = char_end_col(header, "Claude~").expect("Claude~ not in header");
        let d1 = char_end_col(data_row, "6,248").expect("6,248 not in data row");
        assert_eq!(
            h1, d1,
            "first column right edges differ (header {h1} vs data {d1}).\n\
             header:   {header}\n\
             data row: {data_row}"
        );

        let h2 = char_end_col(header, "OpenAI").expect("OpenAI not in header");
        let d2 = char_end_col(data_row, "4,754").expect("4,754 not in data row");
        assert_eq!(
            h2, d2,
            "second column right edges differ (header {h2} vs data {d2}).\n\
             header:   {header}\n\
             data row: {data_row}"
        );
    }

    #[test]
    fn tree_named_header_width_matches_total() {
        // When data values are wider than column labels (e.g. approximate
        // ranges like "5,752 – 6,248" are 13 chars vs label "Claude~" at 7),
        // header columns must be padded to the same width as data columns.
        // Otherwise the header row is narrower and subsequent columns are
        // shifted left, misaligning with data.
        let entries = [approx_entry()];
        let s = run(
            ".",
            &entries,
            &opts(false, false, false, CountFormat::Named),
        );

        let header = s.lines().next().unwrap();
        let total = s.lines().find(|l| l.starts_with("Total")).unwrap();

        assert_eq!(
            header.chars().count(),
            total.chars().count(),
            "header and Total row have different display widths — columns \
             are misaligned.\n  header: '{header}'\n  total:  '{total}'"
        );
    }

    #[test]
    fn flat_named_header_width_matches_total() {
        // Same check as tree_named but for flat output.
        let entries = [approx_entry()];
        let s = run(".", &entries, &opts(true, false, false, CountFormat::Named));

        let header = s.lines().next().unwrap();
        let total = s.lines().find(|l| l.starts_with("Total")).unwrap();

        assert_eq!(
            header.chars().count(),
            total.chars().count(),
            "header and Total row have different display widths — columns \
             are misaligned.\n  header: '{header}'\n  total:  '{total}'"
        );
    }

    #[test]
    fn tree_named_tabular_has_header_columns_and_totals() {
        let entries = [
            text_result("src/main.rs", &[("o200k", 1_234), ("claude", 1_180)]),
            text_result("src/lib.rs", &[("o200k", 456), ("claude", 420)]),
        ];
        let s = run(
            ".",
            &entries,
            &opts(false, false, false, CountFormat::Named),
        );
        assert!(s.contains("OpenAI"), "OpenAI header missing:\n{s}");
        assert!(s.contains("Claude"), "Claude header missing:\n{s}");
        assert!(s.contains("Total"), "Total row missing:\n{s}");
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
        let s = run(
            ".",
            &entries,
            &opts(false, false, false, CountFormat::Named),
        );
        // Use char count (not byte length): box-drawing glyphs are 1 display
        // column but 3 bytes, so chars().count() gives the display width.
        let line_lengths: Vec<usize> = s
            .lines()
            .filter(|l| l.contains(".rs"))
            .map(|l| l.chars().count())
            .collect();
        assert_eq!(line_lengths.len(), 2, "expected 2 file lines:\n{s}");
        assert_eq!(
            line_lengths[0], line_lengths[1],
            "right edges not aligned:\n{s}"
        );
    }

    // ── from_approx ────────────────────────────────────────────────────────

    #[rstest]
    // Large: percentage band dominates
    #[case(1000, 957, 1043)]
    #[case(200, 189, 211)]
    #[case(100, 93, 107)]
    // Small: absolute ±2 floor keeps band ≥ 4 tokens wide
    #[case(10, 7, 13)]
    #[case(5, 2, 8)]
    #[case(1, 0, 4)]
    #[case(0, 0, 2)]
    fn from_approx_bounds(#[case] count: usize, #[case] lo: usize, #[case] hi: usize) {
        let TokenCount::Approx {
            lo: got_lo,
            hi: got_hi,
        } = TokenCount::from_approx(count)
        else {
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
        assert_eq!(
            bracket_cols[0], bracket_cols[1],
            "count brackets not aligned:\n{s}"
        );
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
        assert!(
            v["files"][1]["tokens"].is_null(),
            "binary tokens should be null"
        );
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
