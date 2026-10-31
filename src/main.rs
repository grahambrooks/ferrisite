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
        /// Build for this base URL instead of ferrisite.toml's (for example
        /// http://localhost:8000/ to preview with a static file server).
        #[arg(long)]
        base_url: Option<String>,
    },
    /// Check a built site: internal links, and with --agents the agent-readable output.
    Check {
        /// The built site to check.
        #[arg(default_value = "public")]
        out: PathBuf,
        /// Site root, for the base URL in ferrisite.toml.
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Also check Markdown copies, alternate links and llms.txt.
        #[arg(long)]
        agents: bool,
        /// How many problems to print.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Build, serve on localhost and rebuild on change (drafts included).
    Serve {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Output directory (emptied first, but only if ferrisite created it).
        #[arg(long, default_value = ".ferrisite-serve")]
        out: PathBuf,
        #[arg(long, default_value_t = 1313)]
        port: u16,
        /// Leave drafts and future-dated pages out.
        #[arg(long)]
        no_drafts: bool,
    },
    /// Compare two built sites: URL sets, internal links and feed items.
    Parity {
        /// The reference build (for example Hugo's public/).
        reference: PathBuf,
        /// The candidate build (ferrisite's output).
        candidate: PathBuf,
        /// parity.toml: base URL, ignore patterns with reasons, content regions.
        #[arg(long)]
        config: Option<PathBuf>,
        /// How many examples to print per check.
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Build {
            root,
            out,
            drafts,
            future,
            base_url,
        } => {
            let started = Instant::now();
            let report = ferrisite::build::build(&ferrisite::build::Options {
                root,
                out: out.clone(),
                drafts,
                future,
                base_url,
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
        Command::Check {
            out,
            root,
            agents,
            limit,
        } => {
            let config = ferrisite::config::Config::load(&root)?;
            let report =
                ferrisite::check::run(&out, &config.base_url, agents, &config.check.known_broken)?;
            println!("{} internal links checked", report.links_checked);
            for (name, list) in [
                ("broken links", &report.broken_links),
                (
                    "known broken links (accepted in ferrisite.toml)",
                    &report.known_broken,
                ),
                ("agent-readable output problems", &report.agent_problems),
            ] {
                println!("{}: {}", name, list.len());
                for p in list.iter().take(limit) {
                    println!("  {p}");
                }
            }
            if report.failed() {
                bail!("check failed");
            }
            println!("check ok");
            Ok(())
        }
        Command::Serve {
            root,
            out,
            port,
            no_drafts,
        } => ferrisite::serve::serve(root, out, port, !no_drafts),
        Command::Parity {
            reference,
            candidate,
            config,
            limit,
        } => {
            let cfg = ferrisite::parity::load_config(config.as_deref())?;
            let report = ferrisite::parity::compare(&reference, &candidate, &cfg)?;
            report.print(limit);
            if report.failed() {
                bail!("parity failed");
            }
            println!("parity ok");
            Ok(())
        }
    }
}
