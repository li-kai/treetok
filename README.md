# treetok

Like `tree`, but shows token counts instead of file sizes. Use it to budget context and identify files to refactor.

```console
$ treetok src/
                      CLAUDE   OPENAI
src/
├── output/
│   ├── format.rs      5,971    4,770
│   └── mod.rs         5,289    4,060
├── tokenize/
│   ├── error.rs         473      375
│   ├── local.rs       1,171      932
│   ├── mod.rs         2,336    1,830
│   ├── remote.rs      1,204      907
│   ├── resolve.rs     2,037    1,593
│   └── run.rs         1,097      827
├── lib.rs                44       30
├── main.rs            1,089      837
├── tree.rs            3,539    2,719
└── walk.rs            4,933    3,785

TOTAL                 29,183   22,869
```

## Installation

**macOS (Homebrew)**

```bash
brew install li-kai/treetok/treetok
```

**Pre-built binaries**

Download the latest binary for your platform from the [GitHub Releases][releases] page.

**Nix**

```bash
nix build
./result/bin/treetok --help
```

**Cargo**

```bash
cargo install --git https://github.com/li-kai/treetok treetok
```

[releases]: https://github.com/li-kai/treetok/releases

## Usage

```bash
treetok [OPTIONS] [PATHS...]
```

```bash
# Show token counts for a directory
treetok src/

# Sort by token count, largest first
treetok --sort src/

# Output JSON
treetok --json src/

# Flat list instead of tree
treetok --flat src/

# Limit tree depth
treetok --depth 2 src/
```

### Options

| Flag | Description |
|------|-------------|
| `--sort` | Sort by token count, largest first |
| `--json` | Output JSON |
| `--flat` | Flat file list instead of tree |
| `--no-ignore` | Include files ignored by `.gitignore` |
| `--depth <N>` | Limit tree depth |
| `--offline` | Skip the Claude tokenizer |
| `--no-color` | Disable colored output |
| `-t <NAME>` | Use a specific tokenizer |

### Tokenizers

By default, treetok shows all available tokenizers side-by-side. Use `-t` to select one:

| Name | Requires | Notes |
|------|----------|-------|
| `claude` | `ANTHROPIC_API_KEY` | Claude tokenizer |
| `o200k` | — | OpenAI tokenizer, works offline |

```bash
treetok -t claude src/
treetok -t o200k src/
```

## Anthropic API key

The Claude tokenizer requires an API key from [console.anthropic.com/account/keys][api-keys]. Set it as an environment variable:

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
# or
export TREETOK_API_KEY="sk-ant-..."
```

Add it to `.env` and make sure `.env` is in `.gitignore`.

[nix]: https://nixos.org/download/
[api-keys]: https://console.anthropic.com/account/keys
