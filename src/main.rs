mod config;
mod constants;
mod differ;
mod generator;
mod hasher;
mod scanner;

use constants::*;

use clap::Parser;
use rayon::prelude::*;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

/// Recursively generates per-directory summary docs, bottom-up.
#[derive(Parser)]
#[command(name = "archival")]
struct Cli {
    /// Root directory to index
    root_dir: PathBuf,

    /// Additional ignore patterns beyond .gitignore (repeatable)
    #[arg(long, action = clap::ArgAction::Append)]
    ignore: Vec<String>,

    /// Path to config file
    #[arg(long)]
    config: Option<PathBuf>,

    /// Print what would be regenerated without writing
    #[arg(long)]
    dry_run: bool,

    /// Command to invoke for summary generation (receives file/dir context as args)
    #[arg(long)]
    llm_cmd: Option<String>,

    /// Max directories to process per run (for incremental bootstrapping)
    #[arg(short = 'n', long)]
    max_dirs: Option<usize>,

    /// Add a glob pattern to the ignore list in .archival.toml
    #[arg(long)]
    add_ignore: Option<String>,

    /// List all active ignore patterns (from .gitignore, .archival.toml, and CLI)
    #[arg(long)]
    list_ignores: bool,

    /// Enable chunking for large files (splits files >5000 chars into chunks)
    #[arg(long)]
    chunk: bool,

    /// Delete all index files and exit
    #[arg(long)]
    clean: bool,

    /// Print detailed progress
    #[arg(short, long)]
    verbose: bool,
}

fn main() {
    let cli = Cli::parse();

    let root = cli.root_dir.canonicalize().unwrap_or_else(|e| {
        eprintln!("error: cannot resolve root directory: {}", e);
        std::process::exit(1);
    });

    // Load config file
    let config_path = cli
        .config
        .clone()
        .or_else(|| config::Config::find(&root));
    let cfg = config_path
        .as_ref()
        .and_then(|p| config::Config::load(p))
        .unwrap_or_default();

    cfg.save(&root);

    let mut ignores: Vec<String> = DEFAULT_IGNORES.iter().map(|s| s.to_string()).collect();
    ignores.extend(cfg.ignore.iter().cloned());
    ignores.extend(cli.ignore.iter().cloned());

    let llm_cmd = cli
        .llm_cmd
        .as_deref()
        .or(cfg.llm_cmd.as_deref());

    // Create instruction file
    let instruction_file = root.join(INSTRUCTION_FILENAME);
    if !instruction_file.is_file() {
        let contents = INSTRUCTION_FILE_CONTENTS.replace("{summary-file}", SUMMARY_FILENAME);
        fs::write(&instruction_file, contents).unwrap();
    }

    // Handle --clean: delete all summary files and exit
    if cli.clean {
        clean_summaries(&root, cli.verbose);
        fs::remove_file(instruction_file).unwrap();        
        return;
    }

    // Handle --add-ignore: persist pattern and exit
    if let Some(pattern) = &cli.add_ignore {
        config::add_ignore_pattern(&root, pattern);
        return;
    }

    // Handle --list-ignores: print and exit
    if cli.list_ignores {
        list_ignores(&root, &cfg.ignore, &cli.ignore);
        return;
    }

    // Require llm_cmd for actual indexing
    let llm_cmd = llm_cmd.unwrap_or_else(|| {
        eprintln!("error: --llm-cmd is required (or set llm_cmd in .archival.toml)");
        std::process::exit(1);
    });

    // 0. Collect file extensions and prompt user to allow or ignore each
    let extensions = scanner::collect_extensions(&root, &ignores);
    let new_ignores = review_extensions(&extensions, &ignores, &cfg.allows, &root);
    ignores.extend(new_ignores);

    // 1. Scan
    if cli.verbose {
        println!("Scanning {}...", root.display());
    }
    let nodes = scanner::scan(&root, &ignores);
    if cli.verbose {
        println!("Found {} directories to check.", nodes.len());
    }

    // Clean up orphan index files
    for node in &nodes {
        if differ::is_orphan_summary(&node.path) {
            let orphan = node.path.join(SUMMARY_FILENAME);
            if cli.dry_run {
                println!("Would delete orphan: {}", orphan.display());
            } else {
                if cli.verbose {
                    println!("Deleting orphan: {}", orphan.display());
                }
                let _ = fs::remove_file(&orphan);
            }
        }
    }

    // 2. Separate nodes into leaves (no subdirs) and non-leaves.
    // Nodes are already in post-order (deepest first) from the scanner.
    let (leaf_nodes, non_leaf_nodes): (Vec<_>, Vec<_>) = nodes
        .iter()
        .partition(|node| node.subdirs.is_empty());

    // 3. Diff and regenerate leaves first (parallelized).
    let mut dirty_leaves: Vec<(&scanner::DirNode, differ::DiffResult)> = Vec::new();
    for node in &leaf_nodes {
        let diff = differ::diff(node);
        if diff.is_dirty {
            dirty_leaves.push((node, diff));
        }
    }

    if cli.verbose && !dirty_leaves.is_empty() {
        println!("{} leaf directories need regeneration.", dirty_leaves.len());
    }

    // Apply --max-dirs limit to leaves
    if let Some(max) = cli.max_dirs {
        dirty_leaves.truncate(max);
    }

    dirty_leaves.par_iter().for_each(|(node, diff)| {
        generator::generate_summary(
            node,
            diff,
            llm_cmd,
            &root,
            cli.dry_run,
            cli.verbose,
            cli.chunk,
        );
    });

    // 4. Now diff and regenerate non-leaves bottom-up.
    // Re-diff each one since leaf processing may have created/updated index files.
    let mut non_leaf_count = 0;
    for node in &non_leaf_nodes {
        let diff = differ::diff(node);
        if !diff.is_dirty {
            continue;
        }
        non_leaf_count += 1;
        generator::generate_summary(
            node,
            &diff,
            llm_cmd,
            &root,
            cli.dry_run,
            cli.verbose,
            cli.chunk,
        );
    }

    if dirty_leaves.is_empty() && non_leaf_count == 0 {
        if cli.verbose {
            println!("Everything up to date.");
        }
        return;
    }

    if cli.verbose {
        println!("{} non-leaf directories regenerated.", non_leaf_count);
    }

    println!("Done.");
}

/// Check if a file extension is matched by any ignore pattern.
fn extension_is_ignored(ext: &str, ignores: &[String]) -> bool {
    let ext_glob = format!("*.{}", ext);
    for pattern in ignores {
        let pat = pattern.trim_end_matches('/');
        if pat == ext_glob {
            return true;
        }
        if pat.starts_with("*.") && pat[2..] == *ext {
            return true;
        }
    }
    false
}

/// Check if a file extension is already in the allows list.
fn extension_is_allowed(ext: &str, allows: &[String]) -> bool {
    let ext_glob = format!("*.{}", ext);
    allows.iter().any(|a| a == &ext_glob)
}

/// Interactively review each file extension found in the tree.
/// Extensions already covered by an ignore or allow rule are skipped.
/// For each remaining extension, the user chooses to allow or ignore it.
/// Both choices are persisted to .archival.toml before continuing.
/// Returns a list of newly added ignore patterns.
fn review_extensions(
    extensions: &[String],
    ignores: &[String],
    allows: &[String],
    root: &std::path::Path,
) -> Vec<String> {
    let needs_review: Vec<&String> = extensions
        .iter()
        .filter(|ext| !extension_is_ignored(ext, ignores) && !extension_is_allowed(ext, allows))
        .collect();

    if needs_review.is_empty() {
        return Vec::new();
    }

    println!("Found {} new file extension(s) to review:\n", needs_review.len());

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut new_ignores = Vec::new();

    for ext in &needs_review {
        loop {
            print!("  *.{} — (a)llow or (i)gnore? ", ext);
            io::stdout().flush().unwrap();

            let mut input = String::new();
            if reader.read_line(&mut input).is_err() || input.is_empty() {
                // EOF or error — default to allow
                let pattern = format!("*.{}", ext);
                config::add_allow_pattern(root, &pattern);
                println!("  -> allowed");
                break;
            }

            match input.trim().to_lowercase().as_str() {
                "a" | "allow" => {
                    let pattern = format!("*.{}", ext);
                    config::add_allow_pattern(root, &pattern);
                    println!("  -> allowed (saved to .archival.toml)");
                    break;
                }
                "i" | "ignore" => {
                    let pattern = format!("*.{}", ext);
                    config::add_ignore_pattern(root, &pattern);
                    new_ignores.push(pattern);
                    break;
                }
                _ => {
                    println!("  Please enter 'a' to allow or 'i' to ignore.");
                }
            }
        }
    }

    if !new_ignores.is_empty() {
        println!("\nAdded {} ignore pattern(s). Continuing with updated rules.\n", new_ignores.len());
    } else {
        println!();
    }

    new_ignores
}

fn clean_summaries(root: &std::path::Path, verbose: bool) {
    let mut count = 0;
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            // Skip .git directories
            !(e.file_type().is_dir() && e.file_name().to_string_lossy().starts_with(".git"))
        })
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file()
            && entry.file_name().to_string_lossy() == SUMMARY_FILENAME
        {
            let path = entry.path();
            if verbose {
                println!("Deleting: {}", path.display());
            }
            if fs::remove_file(path).is_ok() {
                count += 1;
            }
        }
    }
    println!("Deleted {} {} file(s).", count, SUMMARY_FILENAME);
}

fn list_ignores(root: &std::path::Path, config_ignores: &[String], cli_ignores: &[String]) {
    // .gitignore patterns
    let gitignore_path = root.join(".gitignore");
    if gitignore_path.is_file() {
        println!(".gitignore:");
        if let Ok(content) = fs::read_to_string(&gitignore_path) {
            for line in content.lines() {
                let line = line.trim();
                if !line.is_empty() && !line.starts_with('#') {
                    println!("  {}", line);
                }
            }
        }
        println!();
    }

    // .archival.toml patterns
    if !config_ignores.is_empty() {
        println!(".archival.toml:");
        for pat in config_ignores {
            println!("  {}", pat);
        }
        println!();
    }

    // CLI patterns
    if !cli_ignores.is_empty() {
        println!("CLI (--ignore):");
        for pat in cli_ignores {
            println!("  {}", pat);
        }
        println!();
    }
}
