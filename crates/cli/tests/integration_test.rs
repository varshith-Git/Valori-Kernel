
use tempfile::tempdir;
use valori_cli::commands::{diff, inspect, replay_query, timeline, verify};
use valori_persistence::fixtures;

#[test]
fn test_integration_workflow() {
    let dir = tempdir().unwrap();
    let paths = fixtures::generate_test_scenario(dir.path()).unwrap();

    // Test Inspect with --dir logic (directory auto-resolution)
    let result = inspect::run(
        Some(dir.path().to_path_buf()), 
        None, 
        None, 
        None
    );
    assert!(result.is_ok());

    // Test Verify (Should pass because fixtures.rs now computes real hash)
    let result = verify::run(paths.snapshot.to_str().unwrap());
    assert!(result.is_ok(), "Verification should succeed on valid fixtures");

    // Test Timeline
    let result = timeline::run(paths.idx.to_str().unwrap());
    assert!(result.is_ok());
}

#[test]
fn test_replay_logic() {
    let dir = tempdir().unwrap();
    
    // Use the REPLAY fixture (Snapshot 100, WAL 101-103)
    let paths = fixtures::generate_replay_scenario(dir.path()).unwrap();

    // Test 1: Replay to 102 (Should replay 2 events: 101, 102)
    let result = replay_query::run(
        paths.snapshot.to_str().unwrap(),
        paths.wal.to_str().unwrap(),
        102,
        None,
    );
    assert!(result.is_ok());

    // Test 2: Replay to 50 (Before snapshot) -> Should warn but not error
    let result = replay_query::run(
        paths.snapshot.to_str().unwrap(),
        paths.wal.to_str().unwrap(),
        50,
        None,
    );
    assert!(result.is_ok()); // Function returns Ok(()) even with warning

    // Test 3: Replay to 105 (Beyond WAL) -> Should warn "Reached end of WAL"
    let result = replay_query::run(
        paths.snapshot.to_str().unwrap(),
        paths.wal.to_str().unwrap(),
        105,
        None,
    );
    assert!(result.is_ok());
}

#[test]
fn test_diff_logic() {
    let dir = tempdir().unwrap();
    // Use the REPLAY fixture (Snapshot 100, WAL 101-103)
    let paths = fixtures::generate_replay_scenario(dir.path()).unwrap();

    // Test 1: Diff 100 vs 100 (Identical)
    let result = diff::run(
        paths.snapshot.to_str().unwrap(),
        paths.wal.to_str().unwrap(),
        100,
        100,
        None,
    );
    assert!(result.is_ok());

    // Test 2: Diff 100 vs 102 (Drifted: 101, 102)
    let result = diff::run(
        paths.snapshot.to_str().unwrap(),
        paths.wal.to_str().unwrap(),
        100,
        102,
        None,
    );
    assert!(result.is_ok());
    assert!(result.is_ok());
}

#[test]
fn test_golden_data_replay() -> anyhow::Result<()> {
    use valori_cli::engine::ForensicEngine;
    let dir = tempdir().unwrap();
    let paths = fixtures::generate_replay_scenario(dir.path()).unwrap();
    
    let mut engine = ForensicEngine::new(paths.snapshot.to_str().unwrap())?;
    
    // Initial State (From Golden Snapshot)
    // Snapshot has IDs: 1, 8, 3 (3 records)
    let initial_hash = engine.state.state_hash();
    assert_eq!(engine.state.record_count(), 3, "Snapshot should have 3 records");

    // Replay to 102 (Adds IDs 101, 102...) 
    // Wait, generate_replay_scenario in fixtures.rs:
    // Snapshot: 1, 8, 3.
    // WAL: 101, 102, 103 (IDs 101, 102, 103)
    // So replay to 102 implies adding events 101 and 102.
    
    engine.replay_to(paths.wal.to_str().unwrap(), 102)?;
    
    let final_hash = engine.state.state_hash();
    
    // Assertions
    assert_ne!(initial_hash, final_hash, "State hash MUST change after replay");
    // 3 initial + 2 replayed = 5
    assert_eq!(engine.state.record_count(), 5, "Should have 5 records after replay (3 snap + 2 wal)");
    
    Ok(())
}
