# grabstuff

**Give coding agents controllable context — in one pass, instead of reading one file at a time.**

Agents waste turns (and tokens) opening files one by one to assemble the context they need. `grabstuff` lets you select across your whole codebase with glob patterns, **slice** each match down to just the part that matters (first N lines, last N chars, full file), and **compile** it all into a single output — Markdown, plain text, or JSON — ready to drop into a prompt. One command, all the context, sliced exactly how you want it.

```sh
# Pull the top of every Rust file + the full README into one Markdown blob
grabstuff "src/**/*.rs" --head 50 lines "README.md" --full --tokens
```

## Why

- **All context in one shot.** Replace dozens of sequential file reads with a single query over the repo.
- **Controllable context.** Slice each file (`--head`/`--tail` by lines or chars) so you spend tokens only on what's relevant, not whole files.
- **Token-aware.** `--tokens` reports the size of the compiled output so you can stay within a budget.
- **Agent-friendly output.** Clean Markdown/JSON with per-file headers and line ranges, so the model knows exactly what it's looking at.
- **Reproducible.** Save a query as a `.grab` script and re-run it whenever you need that context again.

## Install

### Prebuilt binaries (recommended)

Download the archive for your platform from the [latest release](https://github.com/0xDonato/grabstuff/releases/latest), extract it, and put `grabstuff` on your `PATH`. Binaries are published for Linux (x86_64/aarch64), macOS (Intel/Apple Silicon), and Windows (x86_64), each with a SHA-256 checksum.

### From source

```sh
cargo install --git https://github.com/0xDonato/grabstuff
# or
git clone https://github.com/0xDonato/grabstuff && cd grabstuff && cargo install --path .
```

## Usage

```
grabstuff [OPTIONS] [PATTERNS]...
```

### Select files

Glob patterns, one or many:

```sh
grabstuff "src/*.rs"            # all Rust files in src
grabstuff "**/*.md"            # all markdown, recursively
grabstuff "src/" "docs/"       # multiple roots
```

### Slice each match

| Option | Result |
|--------|--------|
| `--head N lines` | first N lines |
| `--head N chars` | first N characters |
| `--tail N lines` | last N lines |
| `--tail N chars` | last N characters |
| `--full` | entire file (default) |

```sh
grabstuff "src/**/*.rs" --head 80 lines
grabstuff "logs/*.log" --tail 200 lines
```

### Output format & destination

| Option | Effect |
|--------|--------|
| `--format md` | Markdown with file headers + line ranges (default) |
| `--format plain` | raw concatenated text |
| `--format json` | structured JSON (great for tools/agents) |
| `-o, --output FILE` | write to a file |
| `--copy` | copy to clipboard |
| `--tokens` | append a token count of the output |

```sh
grabstuff "src/**/*.rs" --format json --tokens -o context.json
grabstuff "*.md" --full --copy
```

### Ignore patterns

```sh
grabstuff "src/**/*.rs" --ignore "**/*_test.rs"
```

`.git/`, `node_modules/`, `target/`, and `.env` are ignored by default.

## Grabfiles

Save a reusable query as a `.grab` script — one glob + options per line, with global options at the end:

```sh
#!/usr/bin/env grabstuff

# Comments start with #
src/*.rs    --head 50 lines
README.md   --full
docs/*.md   --full

--format md
--tokens
--output context.md
```

Run it:

```sh
grabstuff run context.grab
./context.grab          # if marked executable
```

## Configuration

Optional global config at `~/.grabstuff/config.yaml`, overridable per-project with `.grabstuff.yaml` (create one with `grabstuff init`):

```yaml
defaults:
  format: md
  ignore:
    - node_modules/
    - .git/
    - target/
```

## Example output (Markdown)

```markdown
# src/main.rs (lines 1-50)

fn main() {
    println!("Hello");
}

---

# README.md (full)

# My Project
...

---

# Tokens: 1,523
```

## Limits

Local files only — no network sources, no LLM integration (pipe the output to your own tooling). Max 10 MB per file, 100 MB total output.

## License

MIT
