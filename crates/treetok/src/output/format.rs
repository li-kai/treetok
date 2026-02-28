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
