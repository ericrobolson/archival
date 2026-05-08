mod config;
mod constants;
mod differ;
mod generator;
mod hasher;
mod scanner;

use constants::*;

use clap::Parser;
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

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

    /// Add a glob pattern to the ignore list in .archival/archival.toml
    #[arg(long)]
    add_ignore: Option<String>,

    /// List all active ignore patterns (from .gitignore, .archival/archival.toml, and CLI)
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

    let mut ignores: Vec<String> = DEFAULT_IGNORES.iter().map(|s| s.to_string()).collect();
    ignores.extend(cfg.ignore.iter().cloned());
    ignores.extend(cli.ignore.iter().cloned());

    let llm_cmd = cli
        .llm_cmd
        .as_deref()
        .or(cfg.llm_cmd.as_deref());

    // Create instruction file inside .archival/
    let archival_dir = root.join(ARCHIVAL_DIR);
    fs::create_dir_all(&archival_dir).unwrap_or_else(|e| {
        eprintln!("error: cannot create .archival directory: {}", e);
        std::process::exit(1);
    });
    let instruction_file = archival_dir.join(INSTRUCTION_FILENAME);
    if !instruction_file.is_file() {
        let contents = INSTRUCTION_FILE_CONTENTS.replace("{summary-file}", SUMMARY_FILENAME);
        fs::write(&instruction_file, contents).unwrap();
    }

    // Handle --clean: delete all summary files and exit
    if cli.clean {
        clean_summaries(&root, cli.verbose);
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
        eprintln!("error: --llm-cmd is required (or set llm_cmd in .archival/archival.toml)");
        std::process::exit(1);
    });

    // 0. Collect file extensions and prompt user to allow or ignore each
    let extensions = scanner::collect_extensions(&root, &ignores);
    let new_ignores = review_extensions(&extensions, &ignores, &cfg.allows, &root);
    ignores.extend(new_ignores);

    // 1. Scan
    let nodes = scanner::scan(&root, &ignores);
    let total_file_count: usize = nodes.iter().map(|n| n.files.len()).sum();
    println!("Found {} files.", total_file_count);

    // Clean up orphan index files in .archival/
    if archival_dir.is_dir() {
        let orphan_indices: Vec<_> = walkdir::WalkDir::new(&archival_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_type().is_file()
                    && e.file_name().to_string_lossy() == SUMMARY_FILENAME
            })
            .filter(|e| {
                // Map back to source directory
                let index_parent = e.path().parent().unwrap();
                let rel = index_parent.strip_prefix(&archival_dir).unwrap_or(Path::new(""));
                let source_dir = root.join(rel);
                differ::is_orphan_summary(&source_dir, &root)
            })
            .map(|e| e.path().to_path_buf())
            .collect();

        for orphan in &orphan_indices {
            if cli.dry_run {
                println!("Would delete orphan: {}", orphan.display());
            } else {
                if cli.verbose {
                    println!("Deleting orphan: {}", orphan.display());
                }
                let _ = fs::remove_file(orphan);
            }
        }

        // Clean up empty directories left behind in .archival/
        if !cli.dry_run && !orphan_indices.is_empty() {
            clean_empty_archival_dirs(&archival_dir, cli.verbose);
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
        let diff = differ::diff(node, &root);
        if diff.is_dirty {
            dirty_leaves.push((node, diff));
        }
    }

    // Count files needing resummarization (including preliminary non-leaf check)
    let leaf_file_count: usize = dirty_leaves.iter().map(|(_, diff)| diff.changed_files.len()).sum();
    let mut non_leaf_preliminary_dirty: usize = 0;
    let mut non_leaf_preliminary_file_count: usize = 0;
    for node in &non_leaf_nodes {
        let diff = differ::diff(node, &root);
        if diff.is_dirty {
            non_leaf_preliminary_dirty += 1;
            non_leaf_preliminary_file_count += diff.changed_files.len();
        }
    }
    let total_needing_regen = leaf_file_count + non_leaf_preliminary_file_count;

    if dirty_leaves.is_empty() && non_leaf_preliminary_dirty == 0 {
        println!("Everything up to date.");
        return;
    }
    println!("{} files need resummarization.", total_needing_regen);

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

    // 4. Now diff and regenerate non-leaves bottom-up, parallelized by depth.
    // Nodes at the same depth don't depend on each other, so they can run in parallel.
    // Re-diff each one since leaf processing may have created/updated index files.
    let mut depth_groups: BTreeMap<usize, Vec<&scanner::DirNode>> = BTreeMap::new();
    for node in &non_leaf_nodes {
        let depth = node.path.components().count();
        depth_groups.entry(depth).or_default().push(node);
    }
    // Process deepest first (BTreeMap is ascending, so reverse)
    for (_depth, group) in depth_groups.iter().rev() {
        let dirty: Vec<_> = group
            .iter()
            .filter_map(|node| {
                let diff = differ::diff(node, &root);
                if diff.is_dirty { Some((*node, diff)) } else { None }
            })
            .collect();
        dirty.par_iter().for_each(|(node, diff)| {
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
/// Both choices are persisted to .archival/archival.toml before continuing.
/// Returns a list of newly added ignore patterns.
fn review_extensions(
    extensions: &BTreeMap<String, Vec<PathBuf>>,
    ignores: &[String],
    allows: &[String],
    root: &std::path::Path,
) -> Vec<String> {
    let needs_review: Vec<(&String, &Vec<PathBuf>)> = extensions
        .iter()
        .filter(|(ext, _)| !extension_is_ignored(ext, ignores) && !extension_is_allowed(ext, allows))
        .collect();

    if needs_review.is_empty() {
        return Vec::new();
    }

    println!("Found {} new file extension(s) to review:\n", needs_review.len());

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut new_ignores = Vec::new();

    for (ext, files) in &needs_review {
        if !files.is_empty() {
            println!("  *.{}", ext);
            println!("    Example files:");
            for file in files.iter().take(3) {
                if let Ok(rel) = file.strip_prefix(root) {
                    println!("      {}", rel.display());
                } else {
                    println!("      {}", file.display());
                }
            }
        } else {
            println!("  *.{}", ext);
        }

        loop {
            print!("    (a)llow or (i)gnore? ");
            io::stdout().flush().unwrap();

            let mut input = String::new();
            if reader.read_line(&mut input).is_err() || input.is_empty() {
                let pattern = format!("*.{}", ext);
                config::add_allow_pattern(root, &pattern);
                println!("    -> allowed");
                break;
            }

            match input.trim().to_lowercase().as_str() {
                "a" | "allow" => {
                    let pattern = format!("*.{}", ext);
                    config::add_allow_pattern(root, &pattern);
                    println!("    -> allowed (saved to .archival/archival.toml)");
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

/// Remove empty directories inside .archival/ bottom-up.
fn clean_empty_archival_dirs(archival_dir: &std::path::Path, verbose: bool) {
    // Collect all directories, sorted deepest-first so children are removed before parents
    let mut dirs: Vec<PathBuf> = walkdir::WalkDir::new(archival_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir() && e.path() != archival_dir)
        .map(|e| e.path().to_path_buf())
        .collect();
    dirs.sort_by(|a, b| {
        let da = a.components().count();
        let db = b.components().count();
        db.cmp(&da)
    });
    for dir in dirs {
        // Try to remove — only succeeds if empty
        if fs::remove_dir(&dir).is_ok() && verbose {
            println!("Removed empty archival dir: {}", dir.display());
        }
    }
}

fn clean_summaries(root: &std::path::Path, verbose: bool) {
    let archival_dir = root.join(ARCHIVAL_DIR);
    if archival_dir.is_dir() {
        if verbose {
            println!("Deleting: {}/", archival_dir.display());
        }
        if fs::remove_dir_all(&archival_dir).is_ok() {
            println!("Deleted {} directory.", ARCHIVAL_DIR);
        } else {
            eprintln!("warning: failed to delete {}", archival_dir.display());
        }
    } else {
        println!("Nothing to clean.");
    }
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

    // .archival/archival.toml patterns
    if !config_ignores.is_empty() {
        println!(".archival/archival.toml:");
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
