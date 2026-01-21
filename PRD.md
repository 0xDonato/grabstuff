# grabstuff MVP Specification

## Overview

grabstuff is a CLI tool that queries, slices, and compiles content from local files into a single output. It uses a simple line-by-line script syntax that can be executed directly or run as standalone `.grab` files.

---

## Core Features

### 1. File Selection

Select files using glob patterns:
- `src/*.rs` — all Rust files in src
- `**/*.md` — all markdown files recursively
- `config.yaml` — single file

### 2. Slice Modes

Extract portions of file content:

| Mode | Description |
|------|-------------|
| `--head N lines` | First N lines |
| `--head N chars` | First N characters |
| `--tail N lines` | Last N lines |
| `--tail N chars` | Last N characters |
| `--full` | Entire file (default) |

### 3. Output Formats

| Format | Description |
|--------|-------------|
| `--format md` | Markdown with file headers (default) |
| `--format plain` | Raw concatenated text |
| `--format json` | JSON structure |

### 4. Output Destinations

| Option | Description |
|--------|-------------|
| stdout | Default, prints to terminal |
| `--output FILE` | Write to file |
| `--copy` | Copy to clipboard |

### 5. Token Counting

| Option | Description |
|--------|-------------|
| `--tokens` | Display token count in output |

---

## Usage Modes

### Direct CLI

```
grabstuff "src/*.rs" --head 50 lines
grabstuff "*.md" --full --output context.md
grabstuff "src/" "docs/" --head 100 lines --copy
```

### Grabfile Scripts

A `.grab` file contains one glob + options per line:

```
#!/usr/bin/env grabstuff

# Comments start with #
src/*.rs --head 50 lines
README.md --full
docs/*.md --full

--format md
--output context.md
```

Run with:
```
grabstuff run script.grab
./script.grab  # if executable
```

---

## Output Format (Markdown)

```markdown
# src/main.rs (lines 1-50)

fn main() {
    println!("Hello");
}

---

# README.md (full)

# My Project

This is my project...

---

# Tokens: 1,523
```

---

## Ignore Patterns

Exclude files from selection:

```
grabstuff "src/**/*.rs" --ignore "**/*_test.rs"
```

Default ignores (unless overridden):
- `.git/`
- `node_modules/`
- `target/`
- `.env`

---

## Configuration

Optional config at `~/.grabstuff/config.yaml`:

```yaml
defaults:
  format: md
  ignore:
    - node_modules/
    - .git/
    - target/
```

Project-level config at `.grabstuff.yaml` overrides global.

---

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | No files matched |
| 2 | Invalid syntax |
| 3 | File read error |

---

## Constraints

- No network/cloud sources (v1 is local only)
- No LLM integration (user pipes to external tools)
- No variables or DAGs (future version)
- No parallel execution (future version)
- Maximum file size: 10MB per file
- Maximum total output: 100MB

---

## Commands Summary

| Command | Description |
|---------|-------------|
| `grabstuff <globs> [options]` | Query and output files |
| `grabstuff run <file.grab>` | Run a grabfile |
| `grabstuff init` | Create default `.grabstuff.yaml` |
| `grabstuff --help` | Show help |
| `grabstuff --version` | Show version |

---

## Success Criteria

1. Single binary, no dependencies
2. Runs on Mac, Linux, Windows
3. Installable via Homebrew and cargo
4. Executes in under 1 second for typical projects
5. Intuitive enough to use without documentation