use std::sync::Arc;
use tempfile::tempdir;

use mac_cleaner_core::apply::{ApplyEngine, ApplySummary};
use mac_cleaner_core::journal::{JournalReader, JournalWriter};
use mac_cleaner_core::model::candidate::{
    Candidate, ExecutionStrategy, RevalidationFingerprint, RiskTier,
};
use mac_cleaner_core::model::diagnostic::CoverageStatus;
use mac_cleaner_core::model::plan::Plan;
use mac_cleaner_core::platform::process::ProcessExecutor;
use mac_cleaner_core::platform::trash::FakeTrashBackend;
use mac_cleaner_core::restore::RestoreEngine;
use mac_cleaner_core::scan::size::calculate_path_size;
use mac_cleaner_core::scan::traversal::{
    detect_project_markers, walk_directory_safely, ProjectMarkerType, TraversalConfig,
};

#[test]
fn test_end_to_end_sandbox_workflow() {
    // Setup sandbox environment
    let root_dir = tempdir().unwrap();
    let trash_dir = tempdir().unwrap();
    let journal_dir = tempdir().unwrap();

    // Create a mock Rust workspace with target folder
    let rust_project = root_dir.path().join("my_rust_project");
    let target_dir = rust_project.join("target");
    let target_debug_dir = target_dir.join("debug");
    std::fs::create_dir_all(&target_debug_dir).unwrap();
    std::fs::write(rust_project.join("Cargo.toml"), "[package]\nname = \"my_rust_project\"\nversion = \"0.1.0\"").unwrap();
    std::fs::write(target_debug_dir.join("app.o"), vec![0xAB; 8192]).unwrap();
    std::fs::write(target_debug_dir.join("app.d"), vec![0xCD; 4096]).unwrap();

    // 1. Verify project marker detection
    let markers = detect_project_markers(&rust_project);
    assert!(markers.iter().any(|m| m.marker_type == ProjectMarkerType::Rust));

    // 2. Traversal and sizing
    let config = TraversalConfig::default();
    let traversal = walk_directory_safely(&rust_project, &config, "rust_scanner");
    assert_eq!(traversal.coverage_status, CoverageStatus::Complete);
    assert!(traversal.paths.contains(&target_dir));

    let target_size = calculate_path_size(&target_dir).unwrap();
    assert_eq!(target_size.logical_bytes, 8192 + 4096);
    assert!(target_size.allocated_bytes > 0);

    // 3. Plan generation
    let fp = RevalidationFingerprint::from_path(&target_dir).unwrap();
    let candidate = Candidate::new(
        target_dir.clone(),
        "rust_target_scanner",
        RiskTier::Tier2Rebuildable,
        ExecutionStrategy::NativeTrash,
        target_size,
        fp,
        "Rust build artifacts",
    );

    let plan = Plan::from_candidates(
        vec![candidate],
        CoverageStatus::Complete,
        vec![],
        true, // auto-approve safe Tier 2
    );

    assert!(plan.verify_hash());
    assert_eq!(plan.approved_actions().count(), 1);

    // 4. Apply execution with FakeTrashBackend
    let fake_trash = Arc::new(FakeTrashBackend::new(trash_dir.path()));
    let process_executor = Arc::new(ProcessExecutor::new());
    let apply_engine = ApplyEngine::new(fake_trash.clone(), process_executor);

    let mut journal_writer = JournalWriter::create_default_in(journal_dir.path()).unwrap();
    let summary: ApplySummary = apply_engine.apply(&plan, &mut journal_writer).unwrap();

    assert_eq!(summary.total_processed, 1);
    assert_eq!(summary.successful_trash_moves, 1);
    assert_eq!(summary.skipped_drifts, 0);
    assert_eq!(summary.failures, 0);

    // Target directory should now be moved out of project
    assert!(!target_dir.exists());

    // 5. Journal auditing
    let journal_entries = JournalReader::read_entries(journal_writer.path()).unwrap();
    assert_eq!(journal_entries.len(), 2); // Intent + TrashSuccess

    let receipts = JournalReader::read_trash_receipts(journal_writer.path()).unwrap();
    assert_eq!(receipts.len(), 1);
    let receipt = &receipts[0];
    assert_eq!(receipt.original_path, target_dir);
    assert!(receipt.trash_path.exists());

    // 6. Restoration
    let restore_engine = RestoreEngine::new(fake_trash);
    let restore_summary = restore_engine.restore_from_journal(journal_writer.path()).unwrap();

    assert_eq!(restore_summary.total_attempted, 1);
    assert_eq!(restore_summary.successfully_restored, 1);
    assert_eq!(restore_summary.skipped_collisions, 0);
    assert_eq!(restore_summary.failed, 0);

    // Target directory should now be restored
    assert!(target_dir.exists());
    assert!(target_debug_dir.join("app.o").exists());
    assert_eq!(std::fs::read(target_debug_dir.join("app.o")).unwrap(), vec![0xAB; 8192]);
}
