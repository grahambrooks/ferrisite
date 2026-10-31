//! The ferrisite command line. See DESIGN.md.

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::time::Instant;

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
        /// Output directory. Emptied first, but only if ferrisite created it.
        #[arg(long, default_value = "public")]
        out: PathBuf,
        /// Include pages marked `draft: true`.
        #[arg(long)]
        drafts: bool,
        /// Include pages dated in the future.
        #[arg(long)]
        future: bool,
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
        Command::Build {
            root,
            out,
            drafts,
            future,
        } => {
            let started = Instant::now();
            let report = ferrisite::build::build(&ferrisite::build::Options {
                root,
                out: out.clone(),
                drafts,
                future,
            })?;
            println!(
                "Built {} pages, {} list pages, {} files (+{} static) into {} in {} ms",
                report.pages,
                report.lists,
                report.files,
                report.static_files,
                out.display(),
                started.elapsed().as_millis()
            );
            if !report.ignored.is_empty() {
                println!(
                    "Ignored {} non-content files under content/: {}",
                    report.ignored.len(),
                    report.ignored.join(", ")
                );
            }
            Ok(())
        }
        Command::Parity { .. } => bail!("not implemented yet (milestone M2, see DESIGN.md)"),
    }
}
