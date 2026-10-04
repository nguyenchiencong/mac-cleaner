# macOS Cleaner (`mac-cleaner`)

> Safe, high-speed, verifiable storage reclamation for macOS, written in Rust.

## Highlights

- **Safety lifecycle**: never blind-deletes — actions use an immutable plan and pre-flight revalidation; native Trash is the reversible default, while external and in-place purges are explicitly irreversible.
- **APFS-aware estimates**: reports allocated block estimates (`st_blocks * 512`) with hardlink deduplication, not misleading logical sizes.
- **Observed storage feedback**: apply/clean report the volume-wide change in available space across a run, separately from candidate estimates.
- **Immutable plans**: canonical SHA-256 hashed `plan.json` files for auditability and CI/cron automation.
- **JSONL run journals**: append-only execution logs at `~/.local/state/mac-cleaner/runs/<run_id>.jsonl` preserve action outcomes and Trash receipts for restoration.
- **User-space only**: zero elevated privileges required.

## Install / Build

Prerequisites: macOS 12+ (Monterey), Rust 1.75+ (stable).

```bash
git clone https://github.com/your-org/mac-cleaner.git
cd mac-cleaner
cargo build --release
# binary at target/release/mac-cleaner
```

Run the test suite:

```bash
cargo test --workspace
```

## Usage

```bash
mac-cleaner scan                        # discover reclaimable candidates (read-only)
mac-cleaner plan                        # generate an immutable reclamation plan
mac-cleaner apply plan.json --yes       # execute a plan non-interactively
mac-cleaner restore <run-id>            # roll back a previous run from Trash
mac-cleaner history                     # view past runs and stats
mac-cleaner config init                 # generate a starter config file
mac-cleaner clean                       # interactive scan → review → clean wizard (↑/↓, Space, a, Enter; Esc/q cancels)
```

All commands support a `--json` flag for structured output.

`apply` and `clean` report allocated estimates separately from the observed net change in available space when measurable. The latter is volume-wide and can include unrelated filesystem activity; APFS snapshots and shared extents can affect reclamation. Moving candidates to native Trash is reversible, but does not free disk space until Trash is emptied. External cleanup and in-place purges are irreversible and cannot be restored from Trash. Scanner errors or incomplete coverage are shown as diagnostics and prevent automatic approval. `apply --json` includes `observed_free_space_delta_bytes` and per-item `item_results`; `restore` reports each receipt's original path, Trash path, and outcome; `history` distinguishes Trash, external, and in-place purge outcomes, including mixed recoverability.

## Scanners

| Scanner | Tier | Target |
| :--- | :--- | :--- |
| Xcode DerivedData | Tier 2 | `~/Library/Developer/Xcode/DerivedData/*` |
| Unavailable Simulators | Tier 3 | `xcrun simctl delete unavailable` |
| Rust `target/` | Tier 2 | `<workspace>/**/target` |
| Node `node_modules/` | Tier 2 | `<workspace>/**/node_modules` |
| Homebrew Cache | Tier 1 | `~/Library/Caches/Homebrew` |
| Cargo Cache | Tier 1 | `~/.cargo/registry/cache`, `~/.cargo/git/db` |
| npm Cache | Tier 1 | `~/.npm/_cacache` |
| User App Caches | Tier 1 | `~/Library/Caches/*` |

Tier 1 = re-downloadable caches; Tier 2 = rebuildable build artifacts/deps; Tier 3 = retained artifacts requiring explicit selection. User data is never touched.

## Configuration

Config lives at `~/.config/mac-cleaner/config.toml` (init via `mac-cleaner config init`). Key settings: `workspace_roots`, `age_threshold_days`, and `exclusions`.

## Reference

- [`CONTEXT.md`](CONTEXT.md) — domain model and ubiquitous language
- [`docs/adr/`](docs/adr/) — architecture decision records

## License

MIT OR Apache-2.0
