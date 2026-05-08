# Archival

Rust CLI that walks a codebase directory tree, hashes file contents, and recursively regenerates per-directory `INDEX.md` documents (bottom-up) when content hashes change. Eliminates doc drift by keeping summaries and their hashes in a mirrored `.archival/` directory tree.

## Shell Integration

To install archival as a shell function you can call from any directory:

### 1. Build and install the binary

Run `make install` to build and install the binary to `~/bin/archival` or manually do it:
```sh
cargo build --release
mkdir -p ~/bin
cp target/release/archival ~/bin/archival
```

### 2. Add the shell function

Append the following to your `~/.zshrc` or `~/.bashrc`:

```sh
# Use this if you want to set the CLI args yourself
archival() {
  ~/bin/archival "${PWD}" "$@"
}
export -f archival

# Use this if you want a prebuilt command with default args
archival() {
  # ~/bin/archival "${PWD}" --llm-cmd "claude --print"  --verbose
  # ~/bin/archival "${PWD}" --llm-cmd "opencode run -m '$MODEL'" --verbose
  ~/bin/archival "${PWD}" --llm-cmd "claude --model sonnet --print"  --verbose
}
export -f archival
```

Then reload your shell:

```sh
source ~/.zshrc  # or source ~/.bashrc
```

### 3. Shell Usage

Now you can run `archival` from any project directory and it will index the current working directory:

```sh
cd ~/projects/my-app
archival
```

This is equivalent to running `~/bin/archival ./my-app --llm-cmd "claude --print"`.

## LLM Setup

In your AGENTS.md or CLAUDE.md file, add the following:
```
# Traversal Instructions
Index files are stored in the .archival/ directory, which mirrors the source directory structure.
When traversing, look at INDEX.md files in .archival/ before reading individual source files.
For example, the index for src/ is at .archival/src/INDEX.md.
If a file listed in an index looks relevant, then read it.
Otherwise skip it. Don't load the file tree like a maniac.
```

## Usage

```
archival <ROOT_DIR> --llm-cmd <COMMAND> [OPTIONS]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<ROOT_DIR>` | Root directory to index |

### Options

| Flag | Description |
|------|-------------|
| `--llm-cmd <COMMAND>` | Command to invoke for summary generation (required unless set in config) |
| `--ignore <PATTERN>` | Additional ignore patterns beyond `.gitignore` (repeatable) |
| `--config <PATH>` | Path to config file |
| `--dry-run` | Print what would be regenerated without writing |
| `-n`, `--max-dirs <N>` | Max directories to process per run (for incremental bootstrapping) |
| `--add-ignore <PATTERN>` | Add a glob pattern to the ignore list in `.archival/archival.toml` |
| `--list-ignores` | List all active ignore patterns and exit |
| `--chunk` | Enable chunking for large files (splits files >5000 chars into chunks) |
| `--clean` | Delete the entire `.archival/` directory, then exit |
| `-v`, `--verbose` | Print detailed progress |

## LLM Command Examples

The `--llm-cmd` flag specifies the external command archival calls to generate summaries. It receives content via stdin and should print a summary to stdout.

### Claude Code

```sh
archival ./my-project --llm-cmd "claude --print"
```

### Cursor

```sh
archival ./my-project --llm-cmd "cursor --print"
```

### OpenAI via llm CLI

```sh
archival ./my-project --llm-cmd "llm -m gpt-4o"
```

### Custom script

```sh
archival ./my-project --llm-cmd "./scripts/summarize.sh"
```

## Examples

### Basic run

```sh
archival ./my-project --llm-cmd "claude --print"
```

### Dry run (preview without writing)

```sh
archival ./my-project --llm-cmd "claude --print" --dry-run
```

### Verbose output

```sh
archival ./my-project --llm-cmd "claude --print" --verbose
```

### Ignore additional patterns

```sh
archival ./my-project --llm-cmd "claude --print" --ignore "target/" --ignore "*.log"
```

### Incremental bootstrapping (process 50 dirs per run)

```sh
archival ./my-project --llm-cmd "claude --print" -n 50
```

### Enable large file chunking

```sh
archival ./my-project --llm-cmd "claude --print" --chunk
```

### Persist an ignore pattern to config

```sh
archival ./my-project --add-ignore "*.log"
```

### List all active ignore patterns

```sh
archival ./my-project --list-ignores
```

### Delete all generated files

```sh
archival ./my-project --clean
```

## Config File

Config lives at `.archival/archival.toml` inside your project root:

```toml
ignore = ["target/", ".git/", "node_modules/", "*.o"]
allows = ["*.rs", "*.toml"]
llm_cmd = "claude --print"
```

- **ignore** — Glob patterns to exclude (on top of `.gitignore`, which is always respected)
- **allows** — File extensions the user has approved (populated interactively on first run)
- **llm_cmd** — Default LLM command (overridden by `--llm-cmd` on the CLI)

CLI arguments take precedence over config values.

## Extension Review

On each run, archival scans the tree for file extensions. Extensions not already in `allows` or `ignore` in `.archival/archival.toml` are presented interactively:

```
Found 3 new file extension(s) to review:

  *.rs — (a)llow or (i)gnore? a
  -> allowed (saved to .archival/archival.toml)
  *.o — (a)llow or (i)gnore? i
  -> ignored (saved to .archival/archival.toml)
```

Both choices are persisted so you are only asked once per extension.

## How It Works

1. **Setup** — Creates `.archival/` directory and `.archival/INSTRUCTIONS.md` with AI traversal guidance (if not already present).
2. **Extension review** — Scans file extensions and prompts the user to allow or ignore any that aren't already categorized in `.archival/archival.toml`.
3. **Directory scan** — Walks the tree bottom-up via the `ignore` crate, building a list of directories with their files and subdirectories. Respects `.gitignore` and ignore patterns. Skips symlinks, empty directories, zero-byte files, and archival-owned files (`INDEX.md`).
4. **Orphan cleanup** — Deletes index files in `.archival/` for source directories that contain no content.
5. **Leaf diff & regenerate** — Diffs leaf directories (no subdirs) and regenerates dirty ones in parallel with rayon.
6. **Non-leaf diff & regenerate** — Re-diffs each non-leaf directory bottom-up (since leaf processing may have created new index files) and regenerates dirty ones sequentially.
7. **Stitch** — Assembles each `INDEX.md` from a template with file bullets, subdirectory sections, and a trailing `# SYSTEM-HASHES` block. Index files are written to `.archival/` mirroring the source tree structure (e.g., the index for `src/` is at `.archival/src/INDEX.md`).

### Change detection

- Files: SHA-256 of raw bytes
- Subdirectories: SHA-256 of the subdirectory's index file content, excluding everything after the ignore/hash section (prevents cascading regeneration)
- Hashes are stored in each index file — no external state file
- New subdirectories with no index file yet are marked as changed

### Batching and chunking

- Multiple changed files in the same directory are batched into a single LLM call
- With `--chunk`: files exceeding 5000 characters are split into chunks, each chunk summarized separately, then stitched into one summary. Without `--chunk` (default): the entire file is passed to the LLM.

### Orphan cleanup

If a source directory contains only archival-owned files and nothing else, the corresponding index file in `.archival/` is deleted.

## Generated Index Format

Index files are stored in `.archival/` mirroring the source tree. For example, `src/tracker/` has its index at `.archival/src/tracker/INDEX.md`:

```markdown
<!-- Do not edit below this line. This section is auto-generated by archival. -->

# src/tracker
<!--AI: Index files are in the .archival/ directory mirroring the source tree. Look at INDEX.md files there before reading individual source files. If a file listed looks relevant, then read it. Otherwise skip it. -->

- **src/tracker/TrackerFile.h** — Declares binary serialization interface for tracker data.
- **src/tracker/TrackerFile.cpp** — Implements little-endian i32 read/write for phrases, patterns, and songs.

## utils/
Hex parsing, value clamping, and string formatting helpers.
Used by TrackerFile and TrackerCommandHandler for data conversion.

<!--AI: Ignore the below section. It is used only for system tracking.-->
# SYSTEM-HASHES

dir:utils 7890abcdef1234567890abcdef...
file:TrackerFile.cpp e5f67890abcdef1234567890ab...
file:TrackerFile.h d4e5f67890abcdef1234567890...
```

## Make Targets

```sh
make build          # debug build
make release        # release build
make run ARGS="./my-project --llm-cmd 'claude --print'"
make test           # run tests
make clean          # remove build artifacts
make install        # release build + copy to ~/bin/archival
make uninstall      # remove ~/bin/archival
make archival-clean # delete all generated archival files
make archival-test  # run archival on the current project with verbose output
```
