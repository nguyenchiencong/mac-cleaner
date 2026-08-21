# macOS Cleaner (`mac-cleaner`)

> **Safe, high-speed, verifiable storage reclamation tool for macOS written in Rust.**

`mac-cleaner` is a developer-focused, high-assurance storage cleaner for macOS. It discovers, plans, safely relocates to Trash, and restores reclaimable project build artifacts, package manager caches, and stale developer simulator data without risking uncommitted source code or user data.

---

## Key Features & Highlights

- **Verification-First Safety Lifecycle**: Never runs dangerous blind deletions (`rm -rf`). Every mutation proceeds through a 6-stage lifecycle with pre-flight fingerprinting.
- **Native macOS Trash Integration**: Reversible operations move items directly to macOS Trash (`~/.Trash` or volume `.Trashes`), creating full restore receipts.
- **Accurate APFS Allocated Size**: Computes physical disk block usage (`st_blocks * 512`) with hardlink deduplication and non-traversing symlink safety rather than misleading logical file sizes.
- **Immutable & Tamper-Proof Plans**: Generates structured, canonical SHA-256 hashed plan files for auditability, version control, and CI/cron automation.
- **Append-Only Run Journals**: Execution logs are persisted to JSONL journals (`~/.local/state/mac-cleaner/runs/<run_id>.jsonl`) for tracking, auditing, and instant rollback.
- **Collision-Safe Restoration**: Roll back any previous cleaning run with a single command without overwriting newly created files.
- **Zero Elevated Privileges Required**: Operates strictly within user-space permissions, adhering to Apple platform security best practices.

---

## Core Safety Model

`mac-cleaner` enforces a strict, multi-stage safety pipeline designed around the principle of zero accidental data loss:

```
┌────────┐     ┌────────┐     ┌──────────────────────────┐     ┌────────────────────┐     ┌─────────────────────┐     ┌───────────────────────┐
│  Scan  │ ──> │  Plan  │ ──> │ Pre-Flight Revalidation  │ ──> │ Native macOS Trash │ ──> │ Append-Only Journal │ ──> │ Collision-Safe Restore│
└────────┘     └────────┘     └──────────────────────────┘     └────────────────────┘     └─────────────────────┘     └───────────────────────┘
```

1. **Scan (Read-Only Inspection)**:
   Scanners discover candidates across configured workspace roots and user caches without modifying any files. Coverage status (`complete`, `partial`, `degraded`) and non-fatal scan diagnostics are tracked. Symlinks pointing outside project roots are never followed.
2. **Plan (Immutable Generation & Classification)**:
   Candidates are evaluated against risk tiers, age thresholds, and activity criteria to produce an immutable, canonical SHA-256 hashed `Plan` (`plan.json`). Degraded coverage automatically disables automated approvals.
3. **Pre-Flight Revalidation (Drift Detection)**:
   Immediately before applying any action, the engine inspects live filesystem metadata (device ID, inode, file type, physical size, modification time). If any candidate has drifted, mutated, or vanished between planning and execution, the action is skipped or the run is aborted (controlled via `--on-drift`).
4. **Native macOS Trash (Reversible Relocation)**:
   File and directory candidates are relocated to the native macOS Trash (`~/.Trash` or volume-specific `.Trashes`) rather than permanently deleted. A unique `TrashReceipt` is captured containing original paths and destination identifiers.
5. **Append-Only Run Journal (Audit Trail)**:
   Every execution intent, pre-flight check, trash relocation, and external command execution is appended in real-time to a structured JSONL journal (`~/.local/state/mac-cleaner/runs/<run_id>.jsonl`).
6. **Collision-Safe Restore (One-Command Rollback)**:
   Using the run journal and Trash receipts, `mac-cleaner restore <run-id>` restores trashed artifacts back to their original locations. If a new file has since been created at the destination, restoration avoids collisions safely.

---

## 4-Tier Risk Taxonomy

Every discovered candidate is classified into one of four standardized risk tiers:

| Risk Tier | Label | Description | Reversibility | Auto-Clean Policy | Examples |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Tier 1** | `Tier 1: Re-downloadable` | Remote package downloads, shared registries, and fetched toolchain caches. | **Reversible** (Trash) / Irreversible (subsystem tools) | **Auto-Approved** in safe plans | Homebrew cache, Cargo registry `.crate` archives, npm cache |
| **Tier 2** | `Tier 2: Rebuildable` | Project-specific build outputs and downloaded dependencies tied to source repos. | **Reversible** (Trash) | **Auto-Approved** when meeting inactivity/age thresholds | Rust `target/` directories, Node `node_modules/`, Xcode `DerivedData` |
| **Tier 3** | `Tier 3: Retained Artifact` | Retained diagnostic context, run logs, crash reports, orphaned or unavailable simulator instances. | **Reversible** (Trash) / Irreversible (`simctl`) | **Requires Explicit Selection** | Unavailable iOS simulators, system diagnostic logs |
| **Tier 4** | `Tier 4: User Data` | User source code, local configurations, active databases, uncommitted git state. | **Protected** | **Never Auto-Approved** (Guarded against cleaning) | Source files, `.git` internals, uncommitted repositories |

---

## Command Usage

### 1. `mac-cleaner scan`
Discovers reclaimable candidates across workspaces and caches without taking any action.

```bash
# Scan system and display formatted terminal table
mac-cleaner scan

# Output results in structured JSON format (ideal for scripts and pipelines)
mac-cleaner scan --json
mac-cleaner scan --format json

# Filter scanners by category (rust, node, xcode, brew, cargo, npm, cache)
mac-cleaner scan --category rust

# Include an additional workspace directory for this scan
mac-cleaner scan --search-dir ~/Projects
```

### 2. `mac-cleaner plan`
Evaluates candidates and generates an immutable, reviewable reclamation plan.

```bash
# Generate and display a plan in the terminal
mac-cleaner plan

# Filter projects inactive for more than 30 days
mac-cleaner plan --older-than 30d

# Filter by category and export plan to a JSON file
mac-cleaner plan --category node --output node-plan.json

# Output plan as JSON to stdout
mac-cleaner plan --json
```

### 3. `mac-cleaner apply`
Executes an approved plan with strict pre-flight revalidation and journals every action.

```bash
# Apply a saved plan (prompts for confirmation)
mac-cleaner apply plan.json

# Apply without interactive prompt (for CI/CD and automation)
mac-cleaner apply plan.json --yes

# Specify drift policy: 'skip' (default) to skip modified files, or 'abort' to stop execution immediately
mac-cleaner apply plan.json --on-drift abort

# Output execution summary as JSON
mac-cleaner apply plan.json --yes --json
```

### 4. `mac-cleaner restore`
Restores items moved to Trash during a previous execution run using its run ID or journal.

```bash
# Restore all recoverable items from a specific run ID
mac-cleaner restore run-20260820-113000-abcdef

# Restore a specific candidate or receipt by UUID
mac-cleaner restore run-20260820-113000-abcdef --candidate-id 550e8400-e29b-41d4-a716-446655440000

# Output restore summary as JSON
mac-cleaner restore run-20260820-113000-abcdef --json
```

### 5. `mac-cleaner history`
Displays audit records and summary statistics for all past execution runs.

```bash
# Display history table in terminal
mac-cleaner history

# Output history as JSON
mac-cleaner history --json
```

### 6. `mac-cleaner config`
Manages the user configuration profile located at `~/.config/mac-cleaner/config.toml`.

```bash
# Initialize a default configuration file
mac-cleaner config init

# Overwrite existing configuration file
mac-cleaner config init --force

# Inspect active configuration and workspace roots
mac-cleaner config show
```

### 7. `mac-cleaner clean`
Interactive all-in-one terminal wizard for scanning, reviewing with a multi-select menu, and executing reclamation.

```bash
# Launch interactive selector (default when running `mac-cleaner` with no arguments)
mac-cleaner clean

# Non-interactive clean auto-approving safe items
mac-cleaner clean --yes

# Clean with category filter and age threshold
mac-cleaner clean --category rust --older-than 14d
```

---

## Concrete Scanners

| Scanner | Target Paths / Commands | Risk Tier | Strategy | Description |
| :--- | :--- | :--- | :--- | :--- |
| **Xcode DerivedData** | `~/Library/Developer/Xcode/DerivedData/*` | Tier 2 | Native Trash | Inspects intermediate build artifacts, module caches, and index data generated by Xcode. |
| **Unavailable Simulators** | `xcrun simctl delete unavailable` | Tier 3 | External Command | Identifies and purges orphaned and unavailable iOS/watchOS/tvOS runtime simulator devices. |
| **Rust `target/`** | `<workspace>/**/target` | Tier 2 | Native Trash | Identifies cargo build artifacts in repos matching `Cargo.toml` project markers, assessing project git/mtime activity. |
| **Node `node_modules/`**| `<workspace>/**/node_modules` | Tier 2 | Native Trash | Discovers `node_modules` folders verified by `package.json` boundaries and checks last modified dates. |
| **Homebrew Cache** | `~/Library/Caches/Homebrew` | Tier 1 | Native Trash | Scans downloaded Homebrew bottle packages and source archives. |
| **Cargo Cache** | `~/.cargo/registry/cache`, `~/.cargo/git/db` | Tier 1 | Native Trash | Reclaims cached `.crate` tarballs and fetched git dependency checkouts. |
| **npm Cache** | `~/.npm/_cacache` | Tier 1 | Native Trash | Inspects content-addressable package data and index records in npm cache. |
| **User Application Caches** | `~/Library/Caches/*` | Tier 1 | Native Trash | Discovers eligible app caches using safe allowlist filtering (e.g. Google Chrome, Slack, VSCode caches). |

---

## Configuration (`config.toml`)

`mac-cleaner` stores its settings in `~/.config/mac-cleaner/config.toml`. Run `mac-cleaner config init` to generate a starter configuration:

```toml
# Configured workspace roots containing source repositories
[[workspace_roots]]
path = "~/Dev"
recursive = true
max_depth = 5

[[workspace_roots]]
path = "~/Projects"
recursive = true
max_depth = 4

# Minimum inactivity threshold in days for automated approvals
age_threshold_days = 30

# Path exclusion patterns (e.g. globs or substrings)
exclusions = [
    "**/node_modules/**/node_modules",
    "**/production-service/**",
]

# Scanner activation toggles
enabled_scanners = []
disabled_scanners = []
```

---

## Architecture & Reference

The project is structured as a decoupled Cargo workspace:

```
mac-cleaner/
├── CONTEXT.md                    # Ubiquitous domain model, invariants, and terminology
├── Cargo.toml                    # Root workspace definition
├── crates/
│   ├── mac-cleaner-core/         # Pure engine library: scanning, APFS size, planning, revalidation, journaling, trash
│   └── mac-cleaner-cli/          # Terminal CLI application, arg parsing (clap), interactive UI (dialoguer)
└── docs/
    └── adr/                      # Architecture Decision Records (ADRs)
        ├── 0001-cli-first-rust-engine.md
        ├── 0002-safe-revalidation-and-trash-lifecycle.md
        ├── 0003-unprivileged-user-space-v1.md
        ├── 0004-apfs-allocated-size-and-reclamation-estimates.md
        ├── 0005-direct-process-execution-and-irreversible-actions.md
        ├── 0006-scanner-contracts-and-coverage-aware-planning.md
        ├── 0007-cargo-workspace-engine-separation.md
        ├── 0008-explicit-configuration-management.md
        ├── 0009-abstracted-trash-backend-and-receipts.md
        └── 0010-multi-layer-sandbox-verification.md
```

- For full domain modeling concepts and strict ubiquitous language rules, refer to [`CONTEXT.md`](CONTEXT.md).
- For design rationale, trade-offs, and technical decisions, refer to the [`docs/adr/`](docs/adr/) directory.

---

## Build & Test Instructions

### Prerequisites
- macOS 12.0 (Monterey) or higher
- Rust 1.75+ (stable toolchain)

### Building from Source

```bash
# Clone the repository
git clone https://github.com/your-org/mac-cleaner.git
cd mac-cleaner

# Build optimized release binary
cargo build --release

# The compiled binary is available at target/release/mac-cleaner
./target/release/mac-cleaner --help
```

### Running Test Suite

`mac-cleaner` includes multi-layer unit, integration, and sandbox tests that verify filesystem safety invariants without mutating your real system files:

```bash
# Run all workspace unit and integration tests
cargo test --workspace

# Run tests with logging enabled
RUST_LOG=debug cargo test --workspace -- --nocapture
```

---

## License

MIT OR Apache-2.0
