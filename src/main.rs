mod config;
mod file_matcher;
mod formatter;
mod grabfile;
mod output;
mod slicer;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use rayon::prelude::*;
use std::path::PathBuf;
use std::process::ExitCode;

use config::Config;
use file_matcher::FileMatcher;
use formatter::{Format, Formatter};
use grabfile::GrabFile;
use output::OutputDestination;
use slicer::{FileContent, SliceMode, Slicer};

#[derive(Parser)]
#[command(name = "grabstuff")]
#[command(version = "0.1.0")]
#[command(about = "Query, slice, and compile content from local files")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Glob patterns to match files
    #[arg(value_name = "PATTERNS")]
    patterns: Vec<String>,

    /// Take first N lines
    #[arg(long, value_names = ["N", "UNIT"], num_args = 2)]
    head: Option<Vec<String>>,

    /// Take last N lines
    #[arg(long, value_names = ["N", "UNIT"], num_args = 2)]
    tail: Option<Vec<String>>,

    /// Include full file content (default)
    #[arg(long)]
    full: bool,

    /// Output format (overrides config default)
    #[arg(long, value_enum)]
    format: Option<FormatArg>,

    /// Write output to file
    #[arg(long, short)]
    output: Option<PathBuf>,

    /// Copy output to clipboard
    #[arg(long)]
    copy: bool,

    /// Display token count in output
    #[arg(long)]
    tokens: bool,

    /// Ignore patterns (can be specified multiple times)
    #[arg(long)]
    ignore: Vec<String>,
}

#[derive(Clone, ValueEnum)]
enum FormatArg {
    Md,
    Plain,
    Json,
}

impl From<FormatArg> for Format {
    fn from(arg: FormatArg) -> Self {
        match arg {
            FormatArg::Md => Format::Markdown,
            FormatArg::Plain => Format::Plain,
            FormatArg::Json => Format::Json,
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Run a grabfile script
    Run {
        /// Path to the .grab file
        file: PathBuf,
    },
    /// Initialize a .grabstuff.yaml config file
    Init,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::from(0),
        Err(e) => {
            eprintln!("Error: {e}");
            // Determine exit code based on error
            if e.to_string().contains("No files matched") {
                ExitCode::from(1)
            } else if e.to_string().contains("Invalid syntax") {
                ExitCode::from(2)
            } else {
                ExitCode::from(3)
            }
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::load()?;

    match cli.command {
        Some(Commands::Run { file }) => run_grabfile(&file, &config),
        Some(Commands::Init) => init_config(),
        None => {
            if cli.patterns.is_empty() {
                println!("{}", get_help());
                return Ok(());
            }
            run_query(&cli, &config)
        }
    }
}

fn run_query(cli: &Cli, config: &Config) -> Result<()> {
    // Collect ignore patterns
    let mut ignores = config.defaults.ignore.clone();
    ignores.extend(cli.ignore.clone());

    // Match files
    let matcher = FileMatcher::new(&ignores);
    let files = matcher.match_patterns(&cli.patterns)?;

    if files.is_empty() {
        anyhow::bail!("No files matched the given patterns");
    }

    // Determine slice mode
    let slice_mode = parse_slice_mode(&cli.head, &cli.tail, cli.full)?;

    // Process files in parallel
    let slicer = Slicer::new(slice_mode);
    let results: Vec<Result<FileContent>> = files
        .par_iter()
        .map(|path| slicer.process_file(path))
        .collect();

    // Collect results, propagating any errors
    let mut file_contents = Vec::with_capacity(results.len());
    for result in results {
        file_contents.push(result?);
    }

    // Sort by path to maintain consistent ordering
    file_contents.sort_by(|a, b| a.path.cmp(&b.path));

    // Format output
    let format: Format = cli
        .format
        .clone()
        .map(Into::into)
        .unwrap_or_else(|| config.defaults.format.clone().into());
    let formatter = Formatter::new(format).with_tokens(cli.tokens);
    let output = formatter.format(&file_contents)?;

    // Write output
    let destination = if cli.copy {
        OutputDestination::Clipboard
    } else if let Some(ref path) = cli.output {
        OutputDestination::File(path.clone())
    } else {
        OutputDestination::Stdout
    };

    output::write_output(&output, destination)?;

    Ok(())
}

fn run_grabfile(path: &PathBuf, config: &Config) -> Result<()> {
    let grabfile = GrabFile::parse(path).context("Failed to parse grabfile")?;
    grabfile.execute(config)
}

fn init_config() -> Result<()> {
    let config_path = std::env::current_dir()?.join(".grabstuff.yaml");
    if config_path.exists() {
        anyhow::bail!("Config file already exists at {}", config_path.display());
    }

    let default_config = r#"defaults:
  format: md
  ignore:
    - node_modules/
    - .git/
    - target/
    - .env
"#;

    std::fs::write(&config_path, default_config)?;
    println!("Created config file at {}", config_path.display());
    Ok(())
}

fn parse_slice_mode(
    head: &Option<Vec<String>>,
    tail: &Option<Vec<String>>,
    full: bool,
) -> Result<SliceMode> {
    if full {
        return Ok(SliceMode::Full);
    }

    if let Some(args) = head {
        if args.len() != 2 {
            anyhow::bail!("Invalid syntax: --head requires N and unit (lines/chars)");
        }
        let n: usize = args[0]
            .parse()
            .context("Invalid syntax: N must be a number")?;
        match args[1].to_lowercase().as_str() {
            "lines" | "line" => return Ok(SliceMode::HeadLines(n)),
            "chars" | "char" | "characters" => return Ok(SliceMode::HeadChars(n)),
            _ => anyhow::bail!("Invalid syntax: unit must be 'lines' or 'chars'"),
        }
    }

    if let Some(args) = tail {
        if args.len() != 2 {
            anyhow::bail!("Invalid syntax: --tail requires N and unit (lines/chars)");
        }
        let n: usize = args[0]
            .parse()
            .context("Invalid syntax: N must be a number")?;
        match args[1].to_lowercase().as_str() {
            "lines" | "line" => return Ok(SliceMode::TailLines(n)),
            "chars" | "char" | "characters" => return Ok(SliceMode::TailChars(n)),
            _ => anyhow::bail!("Invalid syntax: unit must be 'lines' or 'chars'"),
        }
    }

    Ok(SliceMode::Full)
}

fn get_help() -> &'static str {
    r#"grabstuff - Query, slice, and compile content from local files

WHAT IT DOES:
    grabstuff finds local text files from one or more glob patterns, extracts
    either the full file or a requested head/tail slice, and combines the
    results into Markdown, plain text, or JSON. It is meant for quickly building
    project context that can be saved, copied, or piped into another tool.

WHEN RUN WITH NO ARGUMENTS:
    grabstuff prints this overview and exits successfully. It does not scan the
    current directory, read files, create config, write output files, or copy
    anything to the clipboard until you provide patterns or a subcommand.

USAGE:
    grabstuff <PATTERNS>... [OPTIONS]
    grabstuff run <file.grab>
    grabstuff init

HOW A QUERY RUNS:
    1. Loads config from .grabstuff.yaml, then ~/.grabstuff/config.yaml, then
       built-in defaults.
    2. Expands each pattern, skipping ignored paths, binary files, and files
       larger than 10MB.
    3. Applies the selected slice mode to every matched file.
    4. Sorts results by path for stable output.
    5. Formats the result and writes it to stdout, a file, or the clipboard.

EXAMPLES:
    grabstuff "src/*.rs" --head 50 lines
    grabstuff "*.md" --full --output context.md
    grabstuff "src/" "docs/" --head 100 lines --copy
    grabstuff "Cargo.toml" --format json --tokens
    grabstuff run context.grab
    grabstuff init

PATTERNS:
    "src/*.rs"       Match Rust files directly in src
    "src/**/*.rs"    Match Rust files recursively under src
    "docs/"          Match all files under docs
    "Cargo.toml"     Match one file

OPTIONS:
    --head <N> <lines|chars>    Take first N lines or characters
    --tail <N> <lines|chars>    Take last N lines or characters
    --full                      Include entire file (default)
    --format <md|plain|json>    Output format (default: config or md)
    --output <FILE>             Write to file instead of stdout
    --copy                      Copy to clipboard
    --tokens                    Display token count in output
    --ignore <PATTERN>          Exclude files matching pattern
    --help                      Show this help
    --version                   Show version

CONFIG:
    Project config: .grabstuff.yaml
    Global config:  ~/.grabstuff/config.yaml

    defaults:
      format: md
      ignore:
        - .git/
        - node_modules/
        - target/
        - .env

GRABFILES:
    A .grab file can store repeated queries:

    --format md
    --output context.md
    src/*.rs --head 80 lines
    docs/ --full

    Run it with: grabstuff run context.grab"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_arg_help_explains_behavior() {
        let help = get_help();

        assert!(help.contains("WHEN RUN WITH NO ARGUMENTS:"));
        assert!(help.contains("does not scan the"));
        assert!(help.contains("HOW A QUERY RUNS:"));
        assert!(help.contains("GRABFILES:"));
    }
}
