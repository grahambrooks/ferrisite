//! ferrisite: a pure-Rust static site generator. See DESIGN.md.

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "ferrisite", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build the site into the output directory.
    Build {
        /// Site root (the directory holding ferrisite.toml).
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Output directory.
        #[arg(long, default_value = "public")]
        out: PathBuf,
    },
    /// Compare two built sites: URL sets, internal links and feed items.
    Parity {
        /// The reference build (for example Hugo's public/).
        reference: PathBuf,
        /// The candidate build (ferrisite's output).
        candidate: PathBuf,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Build { .. } => bail!("not implemented yet (milestone M1, see DESIGN.md)"),
        Command::Parity { .. } => bail!("not implemented yet (milestone M2, see DESIGN.md)"),
    }
}
