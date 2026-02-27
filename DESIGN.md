# Design

CLI tool that displays directory trees with LLM token counts instead of file sizes.

## Problem to be solved

1. **Pre-paste budgeting** — see which files are token-heavy before sending to an LLM
2. **Refactoring signal** — identify files that are too large in token terms

## Priorities

Accuracy > Performance > UX > Maintainability > Binary size

## CLI interface

```
treetok [OPTIONS] [PATH...]
```

Multiple paths supported. Defaults to `.` if none given.

- No flags: show token count range (min–max across all available tokenizers)
- `-t <name>`: show exact count for a specific tokenizer (repeatable for side-by-side)
- `--sort`: sort by max token count descending
- `--json`: JSON output (see JSON schema below)
- `--flat`: flat list with full paths, no tree connectors
- `--no-ignore`: show files ignored by `.gitignore`
- `--depth <n>`: limit tree depth
- `--offline`: skip online tokenizers (Claude) even if API key is set

`--flat` + `--sort` combine naturally. `--flat` + `--depth` is a no-op (`--depth` ignored).

### Deferred (V1.1)

- `-` (stdin): read single file from stdin, output token count only

## Output

Default (range mode):
```
src/
├── main.rs        [1,178 – 1,234]
├── lib.rs         [845 – 892]
└── image.png      [binary]

Total: [2,023 – 2,126]
```

With `-t claude -t o200k`:
```
src/
├── main.rs        [claude: 1,234  o200k: 1,178]
├── lib.rs         [claude: 892    o200k: 845]
└── image.png      [binary]

Total: [claude: 2,126  o200k: 2,023]
```

Flat mode (`--flat`):
```
src/main.rs        [1,178 – 1,234]
src/lib.rs         [845 – 892]
src/image.png      [binary]

Total: [2,023 – 2,126]
```

### Range mode tokenizer set

"All available V1 tokenizers" = o200k always, plus Claude if `ANTHROPIC_API_KEY` is set. If only one tokenizer available, show a single number instead of a range.

### Display rules

- Grand total shown at bottom (excludes binary/skipped files)
- Directories are structural only — no per-directory totals
- Empty directories: hidden
- `--sort`: sorts entries within each directory level
- `.gitignore` respected by default (`.git/` always excluded)
- Files > 3 MB skipped with `[too large]` (checked via `stat` before reading)

## File type detection

Use content sniffing (first 8 KB), not extension. Categories:

1. **Text** (valid UTF-8) — tokenize normally
2. **Non-text** (everything else) — show `[binary]`, no count

Non-UTF-8 files are treated as binary.

### Deferred (V2)

Image token estimation (PNG, JPEG, GIF, WebP) via model-specific pixel formulas.

## Symlinks

- Follow symlinks (default behavior of `ignore` crate's `WalkBuilder`)
- Cycle detection: rely on `ignore` crate's built-in cycle detection (skips with warning)
- Broken symlinks: skip with warning to stderr

## Colors

Respect `NO_COLOR` env var and `--no-color` flag. When colors enabled:

- Directory names: **bold**
- Tree connectors (`├──`, `└──`, `│`): dim
- Token counts: default color
- `[binary]`, `[too large]`, `[error]`: dim

No color in `--json` mode. Auto-detect TTY (no color when piped).

## JSON schema

```
treetok --json | jq
```

```json
{
  "root": "src/",
  "files": [
    {
      "path": "src/main.rs",
      "type": "text",
      "tokens": {
        "o200k": 1189,
        "claude": 1234
      }
    },
    {
      "path": "src/data.bin",
      "type": "binary",
      "tokens": null
    }
  ],
  "total": {
    "o200k": 1189,
    "claude": 1234
  }
}
```

- `tokens: null` for binary files
- Skipped files: `"tokens": null, "skipped": "too large"`
- `total` excludes binary and skipped files

## Tokenization strategy

### V1 tokenizers

| Name | Method | Offline? |
|---|---|---|
| `claude` | Anthropic `count_tokens` API | No |
| `o200k` | `tiktoken-rs` (`o200k_base`) | Yes |

### Deferred (V2+)

| Name | Method |
|---|---|
| `qwen` | `tokenizers` crate (HuggingFace `tokenizer.json`) |
| `glm` | `tokenizers` crate (HuggingFace `tokenizer.json`) |
| `kimi` | `tiktoken-rs` via `CoreBPE::new()` |

HuggingFace tokenizers are ~20 MB each. V2 will download on first use to `~/.cache/treetok/`.

### Claude API details

- Endpoint: `POST https://api.anthropic.com/v1/messages/count_tokens`
- Headers: `x-api-key`, `anthropic-version: 2023-06-01`
- Request: `{"model": "claude-sonnet-4-6", "messages": [{"role": "user", "content": "..."}]}`
- Response: `{"input_tokens": 14}`
- Free, but rate-limited (100–8000 RPM depending on tier)
- No batching — one request per file
- Requires `ANTHROPIC_API_KEY`. If missing: skip Claude with a warning in range mode, error if `-t claude` explicit.

## Environment variables

| Variable | Purpose |
|---|---|
| `ANTHROPIC_API_KEY` | Claude tokenizer API key |
| `NO_COLOR` | Disable colored output (any value) |

No config file in V1. Defer to V2 if needed.

## Error handling

- File unreadable (permissions): print warning to stderr, continue with other files
- Tokenizer failure: print warning to stderr, show `[error]` for that file
- Claude API key missing: warn and skip (range mode) or error (explicit `-t claude`)
- Claude API rate limit: back off and retry (3 attempts)
- Network failure: warn and skip Claude column
- No valid tokenizers available: exit with error

## Exit codes

Uses `exitcode` crate (sysexits.h conventions):

| Code | Meaning |
|---|---|
| 0 | Success |
| 64 | Bad CLI usage |
| 66 | Input path not found |
| 69 | Claude API unavailable (when explicitly requested) |
| 74 | I/O error |

## Crates

| Purpose | Crate |
|---|---|
| CLI | `clap` (derive) |
| Directory walking | `ignore` |
| Tree rendering | `termtree` |
| OpenAI tokenizer | `tiktoken-rs` |
| HTTP (Claude API) | `reqwest` |
| Terminal colors | `owo-colors` |
| Exit codes | `exitcode` |
