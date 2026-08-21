use std::path::PathBuf;
use tempfile::tempdir;

use mac_cleaner_core::config::{ConfigProfile, WorkspaceRoot};
use mac_cleaner_core::journal::{JournalReader, JournalWriter};
use mac_cleaner_core::model::candidate::{
    AllocatedSize, Candidate, ExecutionStrategy, RevalidationFingerprint, RiskTier,
};
use mac_cleaner_core::model::diagnostic::CoverageStatus;
use mac_cleaner_core::model::plan::Plan;
use mac_cleaner_core::platform::trash::{FakeTrashBackend, TrashBackend};
use mac_cleaner_core::restore::RestoreEngine;
use mac_cleaner_core::scanners::{ScanContext, ScannerRegistry};

#[test]
fn test_cli_scanner_suite_execution() {
    let temp = tempdir().unwrap();

    // Create mock Rust project
    let rust_dir = temp.path().join("rust_proj");
    let target_dir = rust_dir.join("target").join("release");
    std::fs::create_dir_all(&target_dir).unwrap();
    std::fs::write(
        rust_dir.join("Cargo.toml"),
        "[package]\nname = \"rust_proj\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(target_dir.join("app"), vec![0xFA; 8192]).unwrap();

    // Create mock Node project
    let node_dir = temp.path().join("node_proj");
    let nm_dir = node_dir.join("node_modules").join("lodash");
    std::fs::create_dir_all(&nm_dir).unwrap();
    std::fs::write(
        node_dir.join("package.json"),
        r#"{"name": "node_proj", "version": "1.0.0"}"#,
    )
    .unwrap();
    std::fs::write(nm_dir.join("index.js"), b"module.exports = {};").unwrap();

    let config = ConfigProfile {
        workspace_roots: vec![WorkspaceRoot::new(temp.path())],
        age_threshold_days: Some(0),
        exclusions: vec![],
        enabled_scanners: vec![],
        disabled_scanners: vec![],
    };

    let ctx = ScanContext::from_config(&config);
    let registry = ScannerRegistry::default_registry();

    let results = registry.execute_all(&ctx);
    let mut candidates = Vec::new();
    for res in results {
        let r = res.unwrap();
        candidates.extend(r.candidates);
    }

    assert!(
        candidates.iter().any(|c| c.scanner_name == "rust_target"),
        "Should discover Rust target directory"
    );
    assert!(
        candidates.iter().any(|c| c.scanner_name == "node_modules"),
        "Should discover Node.js node_modules directory"
    );
}

#[test]
fn test_cli_filtered_category_scan() {
    let temp = tempdir().unwrap();

    let rust_dir = temp.path().join("rust_proj");
    let target_dir = rust_dir.join("target");
    std::fs::create_dir_all(&target_dir).unwrap();
    std::fs::write(
        rust_dir.join("Cargo.toml"),
        "[package]\nname = \"rust_proj\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    let config = ConfigProfile {
        workspace_roots: vec![WorkspaceRoot::new(temp.path())],
        age_threshold_days: Some(0),
        exclusions: vec![],
        enabled_scanners: vec![],
        disabled_scanners: vec![],
    };

    let ctx = ScanContext::from_config(&config);
    let registry = ScannerRegistry::default_registry();

    // Filter only rust
    let results = registry.execute_filtered(&ctx, |s| s.name() == "rust_target");
    assert_eq!(results.len(), 1);
    let r = results.into_iter().next().unwrap().unwrap();
    assert_eq!(r.scanner_name, "rust_target");
}

#[test]
fn test_plan_generation_and_tamper_proofing() {
    let temp = tempdir().unwrap();
    let sample_file = temp.path().join("sample.bin");
    std::fs::write(&sample_file, vec![0x11; 4096]).unwrap();

    let fp = RevalidationFingerprint::from_path(&sample_file).unwrap();
    let candidate = Candidate::new(
        sample_file.clone(),
        "rust_target",
        RiskTier::Tier2Rebuildable,
        ExecutionStrategy::NativeTrash,
        AllocatedSize::new(4096, 4096),
        fp,
        "Sample artifact",
    );

    let plan = Plan::from_candidates(
        vec![candidate],
        CoverageStatus::Complete,
        vec![],
        true,
    );

    assert!(plan.verify_hash());
    let json = plan.to_json().unwrap();
    let loaded = Plan::from_json(&json).unwrap();
    assert!(loaded.verify_hash());

    // Tampering test: modify path
    let mut tampered = loaded.clone();
    tampered.actions[0].path = PathBuf::from("/etc/passwd");
    assert!(!tampered.verify_hash(), "Tampered plan must fail hash check");
}

#[test]
fn test_restore_workflow_with_journal() {
    let work_dir = tempdir().unwrap();
    let trash_dir = tempdir().unwrap();
    let journal_dir = tempdir().unwrap();

    let backend = std::sync::Arc::new(FakeTrashBackend::new(trash_dir.path()));
    let file_to_clean = work_dir.path().join("temp_artifact.o");
    std::fs::write(&file_to_clean, b"important build data").unwrap();

    // Move to fake trash
    let receipt = backend.move_to_trash(&file_to_clean).unwrap();
    assert!(!file_to_clean.exists());

    // Write receipt to journal
    let mut journal_writer = JournalWriter::create_default_in(journal_dir.path()).unwrap();
    let plan_id = uuid::Uuid::new_v4();
    let action_id = uuid::Uuid::new_v4();
    journal_writer
        .append(&mac_cleaner_core::model::manifest::RunJournalEntry::trash_success(
            plan_id,
            action_id,
            file_to_clean.clone(),
            receipt,
        ))
        .unwrap();

    // Read back and restore
    let restore_engine = RestoreEngine::new(backend);
    let receipts = JournalReader::read_trash_receipts(journal_writer.path()).unwrap();
    let summary = restore_engine.restore_receipts(&receipts);

    assert_eq!(summary.total_attempted, 1);
    assert_eq!(summary.successfully_restored, 1);
    assert!(file_to_clean.exists());
    assert_eq!(std::fs::read(&file_to_clean).unwrap(), b"important build data");
}
