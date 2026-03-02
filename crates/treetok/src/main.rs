//! treetok — display directory trees with LLM token counts.

use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::PathBuf;

use clap::Parser;

use treetok::output::{CountFormat, OutputOptions, TokenCount};
use treetok::tokenize::TokenizerId;
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
    paths: Vec<PathBuf>,

    /// Tokenizer(s) to use (repeatable).  Available: o200k, claude.
    #[arg(short = 't', value_name = "TOKENIZER")]
    tokenizers: Vec<String>,

    /// Output only the total token count as a bare number.
    #[arg(long, conflicts_with_all = ["json", "flat", "sort"])]
    count: bool,

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

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn read_stdin_or_exit() -> walk::WalkResult {
    match walk::read_stdin() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error reading stdin: {e}");
            std::process::exit(exitcode::IOERR);
        }
    }
}

// ─── Entry point ──────────────────────────────────────────────────────────────

fn main() {
    let mut cli = Cli::parse();

    let mut stdin_result: Option<walk::WalkResult> = None;

    // Handle explicit `-` path.
    let dash_count = cli.paths.iter().filter(|p| p.as_os_str() == "-").count();
    if dash_count > 1 {
        eprintln!("error: at most one `-` (stdin) path allowed");
        std::process::exit(exitcode::USAGE);
    }
    if dash_count == 1 {
        cli.paths.retain(|p| p.as_os_str() != "-");
        stdin_result = Some(read_stdin_or_exit());
    }

    // Auto-detect piped stdin when no paths given.
    if cli.paths.is_empty() && stdin_result.is_none() && !std::io::stdin().is_terminal() {
        stdin_result = Some(read_stdin_or_exit());
    }

    // Default to "." if no paths and no stdin.
    if cli.paths.is_empty() && stdin_result.is_none() {
        cli.paths.push(".".into());
    }

    let api_key = if cli.offline {
        None
    } else {
        tokenize::load_api_key()
    };
    let resolved = match tokenize::resolve_tokenizers(&cli.tokenizers, cli.offline, api_key) {
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
    let count_format = if resolved.count() == 1 {
        CountFormat::Single
    } else {
        CountFormat::Named
    };

    // Build output options (encapsulates color detection).
    let out_opts = OutputOptions::new(cli.flat, cli.json, cli.sort, cli.no_color, count_format);

    let walk_opts = walk::WalkOptions {
        no_ignore: cli.no_ignore,
        depth: cli.depth,
    };
    let mut walk_results = walk::walk_paths(&cli.paths, &walk_opts);

    // Prepend stdin result if present.
    if let Some(sr) = stdin_result {
        walk_results.insert(0, sr);
    }

    // Report any non-fatal walk errors as a miette diagnostic.
    for walk_result in &walk_results {
        if walk_result.has_errors() {
            let diag = walk::WalkErrors::from_result(walk_result);
            if let Some(d) = diag {
                eprintln!("{:?}", miette::Report::new(d));
            }
        }
    }

    if cli.count {
        // --count: accumulate totals across all walk results, print max.
        let mut totals: BTreeMap<TokenizerId, TokenCount> = BTreeMap::new();
        for walk_result in &walk_results {
            let results = tokenize::tokenize_entries(&walk_result.entries, &resolved);
            output::accumulate_totals(&results, &mut totals);
        }
        println!("{}", output::max_total(&totals));
    } else {
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
}
