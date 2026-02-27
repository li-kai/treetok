//! treetok — display directory trees with LLM token counts.

use std::path::PathBuf;

use clap::Parser;

use treetok::output::{CountFormat, OutputOptions};
use treetok::{output, tokenize, walk};

// ─── CLI ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Parser)]
#[command(
    name = "treetok",
    about = "Display directory trees with LLM token counts",
    version
)]
#[allow(clippy::struct_excessive_bools)]
struct Cli {
    /// Paths to display (default: current directory).
    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,

    /// Tokenizer(s) to use (repeatable).  Available: o200k, claude.
    #[arg(short = 't', value_name = "TOKENIZER")]
    tokenizers: Vec<String>,

    /// Sort entries by token count (descending).
    #[arg(long)]
    sort: bool,

    /// Output JSON instead of a tree.
    #[arg(long)]
    json: bool,

    /// Output a flat file list instead of a tree.
    #[arg(long)]
    flat: bool,

    /// Include files ignored by .gitignore.
    #[arg(long)]
    no_ignore: bool,

    /// Limit tree depth.
    #[arg(long, value_name = "N")]
    depth: Option<usize>,

    /// Skip online tokenizers (Claude) even if `ANTHROPIC_API_KEY` is set.
    #[arg(long)]
    offline: bool,

    /// Disable ANSI colors.
    #[arg(long)]
    no_color: bool,
}

// ─── Entry point ──────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();

    // Resolve tokenizers.
    let resolved = match tokenize::resolve_tokenizers(&cli.tokenizers, cli.offline) {
        Ok(t) => t,
        Err(tokenize::TokenizeError::NoApiKey) => {
            eprintln!("error: ANTHROPIC_API_KEY is not set (required by -t claude)");
            std::process::exit(exitcode::UNAVAILABLE);
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(exitcode::USAGE);
        }
    };

    // Determine count format.
    let count_format = if !cli.tokenizers.is_empty() {
        CountFormat::Named
    } else if resolved.count() == 1 {
        CountFormat::Single
    } else {
        CountFormat::Range
    };

    // Build output options (encapsulates color detection).
    let out_opts = OutputOptions::new(cli.flat, cli.json, cli.sort, cli.no_color, count_format);

    // Walk directories.
    let walk_opts = walk::WalkOptions {
        no_ignore: cli.no_ignore,
        depth: cli.depth,
    };
    let walk_results = walk::walk_paths(&cli.paths, &walk_opts);

    // Report any non-fatal walk errors as a miette diagnostic.
    for walk_result in &walk_results {
        if walk_result.has_errors() {
            // Clone the errors for reporting; entries are still needed below.
            let diag = walk::WalkErrors::from_result(walk_result);
            if let Some(d) = diag {
                eprintln!("{:?}", miette::Report::new(d));
            }
        }
    }

    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    for walk_result in &walk_results {
        let results = tokenize::tokenize_entries(&walk_result.entries, &resolved);

        let root_label = walk_result.root.display().to_string();

        if let Err(e) = output::write_output(&mut out, &root_label, &results, &out_opts) {
            eprintln!("error writing output: {e}");
            std::process::exit(exitcode::IOERR);
        }
    }
}
