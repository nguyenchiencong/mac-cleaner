# macOS Cleaner

A domain model for discovering, assessing, and reclaiming storage space on macOS with verifiable safety and recoverability.

## Language

### Core Concepts

**Candidate**:
A specific filesystem path or external resource discovered during a scan that is eligible for assessment and potential reclamation.
_Avoid_: Junk file, target item, clutter

**Scanner**:
An inspection module responsible for discovering candidates and measuring their allocated size and metadata without modifying the filesystem.
_Avoid_: Cleaner plugin, detector, spider

**Risk Tier**:
The safety classification assigned to a candidate defining its reversibility and operational impact (Tier 1 Re-downloadable, Tier 2 Rebuildable, Tier 3 Retained Artifact, Tier 4 User Data).
_Avoid_: Severity, danger level, safety score

**Plan**:
An immutable, serialized set of approved actions generated from scanned candidates, ready for user review or execution.
_Avoid_: Queue, changeset, batch

**Manifest**:
An immutable audit record created during execution that logs every action taken, timestamps, original paths, and destination trash identifiers for tracking and restoration.
_Avoid_: History log, audit trail, transaction log

**Execution Strategy**:
The specific mechanism used to process an approved action on a candidate, such as moving to native Trash or delegating to an official subsystem command.
_Avoid_: Deletion method, cleaner engine, purger

**Reclamation**:
The act of freeing physical disk capacity by moving, pruning, or purging validated candidates.
_Avoid_: Cleaning, scrubbing, wiping

### Discovery & Traversal

**Configuration Profile**:
A validated set of workspace roots, age thresholds, exclusions, and scanner toggles defining scanning behavior.
_Avoid_: User settings, preferences, runtime options

**Workspace Root**:
A top-level directory explicitly configured or recognized as containing source code repositories.
_Avoid_: Scan path, search directory, project folder

**Project Marker**:
A filesystem artifact (such as a manifest or version control directory) used to establish project boundaries and context.
_Avoid_: Anchor file, signature, indicator

**Coverage Status**:
An indicator of whether a candidate's underlying path was inspected completely, or whether permission barriers degraded evidence confidence.
_Avoid_: Scan completeness, error flag

**Scan Diagnostic**:
A structured non-fatal record capturing skipped paths, permission boundaries, or filesystem anomalies encountered during traversal.
_Avoid_: Scan error, warning log, traversal issue

**Sandbox Harness**:
An isolated test environment simulating filesystem hierarchies and candidate mutations to verify engine safety invariants.
_Avoid_: Mock environment, testbed, test sandbox

### Planning & Execution

**Revalidation**:
The pre-flight verification performed immediately before taking action to ensure candidate metadata and identity have not drifted.
_Avoid_: Pre-check, sanity check, drift test

**Revalidation Fingerprint**:
The set of filesystem metadata (device, inode, size, timestamps) captured during planning to verify candidate identity immediately before execution.
_Avoid_: File hash, checksum, metadata signature

**External Cleanup**:
A delegated operation invoking an official subsystem command whose actions are irreversible and cannot utilize native Trash.
_Avoid_: Third-party purge, shell cleanup, command cleaner

**Allocated Estimate**:
The physical block allocation calculated for a candidate, representing an upper-bound reclamation estimate subject to filesystem extent sharing and snapshot retention.
_Avoid_: Exact size, actual savings, freed space

**Run Journal**:
An append-only structured record persisted during execution that tracks operation intents, actual outcomes, and destination paths.
_Avoid_: Transaction log, execution history

**Activity Criteria**:
The composite evaluation of timestamps, project activity, and repository status used to determine recommendation eligibility without guaranteeing safety.
_Avoid_: Staleness score, heuristic rating, dirty check

**Trash Backend**:
An execution adapter responsible for transferring candidates to the operating system's trash facility and returning location receipts.
_Avoid_: Trash driver, file mover, delete adapter

**Trash Receipt**:
A record returned upon successful relocation to Trash, capturing the original path, destination identifier, and timestamp.
_Avoid_: Trash token, move result, delete ticket
