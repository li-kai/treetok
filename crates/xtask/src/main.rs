//! Workspace development tasks.
//!
//! Run with `cargo xtask <subcommand>` (or `just <recipe>`).

use std::{
    io::{self, Write},
    path::PathBuf,
};

fn main() {
    let subcommand = std::env::args().nth(1);
    match subcommand.as_deref() {
        Some("update-ctoc") => update_ctoc(),
        Some(other) => {
            eprintln!("error: unknown subcommand {other:?}");
            eprintln!("available: update-ctoc");
            std::process::exit(1);
        }
        None => {
            eprintln!("usage: cargo xtask <subcommand>");
            eprintln!("available: update-ctoc");
            std::process::exit(1);
        }
    }
}

/// Download the ctoc vocab from GitHub, convert to a compact binary format,
/// and write it to `crates/treetok/src/tokenize/ctoc_vocab.bin`.
///
/// # Binary format
///
/// The file is a sequence of variable-length records with no header:
///
/// ```text
/// [length: u16 LE][token_bytes: u8 × length] ...
/// ```
///
/// The reader iterates until EOF; no entry count is stored.
fn update_ctoc() {
    const URL: &str = "https://raw.githubusercontent.com/rohangpta/ctoc/main/vocab.json";

    // ── download ─────────────────────────────────────────────────────────────

    eprint!("Downloading {URL} … ");
    let resp = reqwest::blocking::get(URL).expect("HTTP request failed");
    if !resp.status().is_success() {
        eprintln!("HTTP {}", resp.status());
        std::process::exit(1);
    }
    let body = resp.text().expect("failed to read response body");
    eprintln!("done.");

    // ── parse ─────────────────────────────────────────────────────────────────

    let root: serde_json::Value = serde_json::from_str(&body).expect("failed to parse vocab.json");
    let tokens: Vec<String> = serde_json::from_value(
        root.get("verified")
            .expect("missing \"verified\" key in vocab.json")
            .clone(),
    )
    .expect("\"verified\" is not an array of strings");

    eprintln!("{} tokens parsed.", tokens.len());

    // ── encode ────────────────────────────────────────────────────────────────
    //
    // Each token: [length: u16 LE][bytes: u8 × length]

    let mut buf: Vec<u8> = Vec::with_capacity(tokens.len() * 4);
    for token in &tokens {
        let bytes = token.as_bytes();
        let len = u16::try_from(bytes.len()).expect("token longer than 65535 bytes");
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(bytes);
    }

    // ── write ─────────────────────────────────────────────────────────────────

    let out_path = workspace_root().join("crates/treetok/src/tokenize/ctoc_vocab.bin");

    let mut file = std::fs::File::create(&out_path)
        .unwrap_or_else(|e| panic!("cannot create {}: {e}", out_path.display()));
    file.write_all(&buf)
        .unwrap_or_else(|e| panic!("write failed: {e}"));

    eprintln!("Written {} bytes to {}", buf.len(), out_path.display());
}

/// Resolve the workspace root as the parent of this package's manifest dir.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent() // crates/
        .expect("crates/ parent missing")
        .parent() // workspace root
        .expect("workspace root missing")
        .to_path_buf()
}

// Keep the compiler happy about unused import on non-unix.
#[allow(dead_code)]
fn _assert_io(_: io::Error) {}
