//! Lives outside `src` so the crate itself keeps no reference to the removed
//! swap module. A separate test binary also keeps `init_db`'s global
//! connections away from the unit tests.

use bitkitcore::init_db;

/// Older releases kept swap data in `boltz.db`. With the wallet seed it is what
/// a manual refund of a locked swap needs, so `init_db` must neither create
/// nor touch it.
#[test]
fn init_db_leaves_boltz_db_alone() {
    let fresh = tempfile::tempdir().unwrap();
    init_db(fresh.path().to_string_lossy().into_owned()).unwrap();
    assert!(fresh.path().join("activity.db").exists());
    assert!(fresh.path().join("blocktank.db").exists());
    assert!(!fresh.path().join("boltz.db").exists());

    let existing = tempfile::tempdir().unwrap();
    let legacy_db = existing.path().join("boltz.db");
    let legacy_bytes = b"legacy boltz.db contents".to_vec();
    std::fs::write(&legacy_db, &legacy_bytes).unwrap();
    init_db(existing.path().to_string_lossy().into_owned()).unwrap();
    assert_eq!(std::fs::read(&legacy_db).unwrap(), legacy_bytes);
}
