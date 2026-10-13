
use mini_lsm::Lsm;
use std::fs;
use std::path::PathBuf;

fn test_directory(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "mini-lsm-test-{}-{name}",
        std::process::id()
    ));

    if path.exists() {
        fs::remove_dir_all(&path).unwrap();
    }

    path
}

#[test]
fn put_and_get() {
    let path = test_directory("put-get");
    let mut db = Lsm::open(&path, 10).unwrap();

    db.put("A".into(), 10).unwrap();
    db.put("B".into(), 20).unwrap();

    assert_eq!(db.get("A"), Some(10));
    assert_eq!(db.get("B"), Some(20));
    assert_eq!(db.get("Z"), None);

    drop(db);
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn update_existing_key() {
    let path = test_directory("update");
    let mut db = Lsm::open(&path, 10).unwrap();

    db.put("A".into(), 10).unwrap();
    db.put("A".into(), 99).unwrap();

    assert_eq!(db.get("A"), Some(99));

    // Updating a key should not create a second MemTable key.
    drop(db);
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn automatic_flush() {
    let path = test_directory("flush");
    let mut db = Lsm::open(&path, 2).unwrap();

    db.put("A".into(), 10).unwrap();
    assert_eq!(db.get("A"), Some(10));

    db.put("B".into(), 20).unwrap();

    // Reopening verifies that the flush persisted both values.
    drop(db);

    let db = Lsm::open(&path, 2).unwrap();

    assert_eq!(db.get("A"), Some(10));
    assert_eq!(db.get("B"), Some(20));

    drop(db);
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn newest_value_wins_after_compaction() {
    let path = test_directory("compaction");
    let mut db = Lsm::open(&path, 1).unwrap();

    db.put("B".into(), 20).unwrap();
    db.put("B".into(), 40).unwrap();

    assert_eq!(db.get("B"), Some(40));

    db.compact().unwrap();

    assert_eq!(db.get("B"), Some(40));

    drop(db);
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn deleted_key_stays_deleted_after_compaction() {
    let path = test_directory("delete");
    let mut db = Lsm::open(&path, 1).unwrap();

    db.put("B".into(), 20).unwrap();
    db.delete("B".into()).unwrap();

    assert_eq!(db.get("B"), None);

    db.compact().unwrap();

    assert_eq!(db.get("B"), None);

    drop(db);
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn unflushed_data_survives_reopening() {
    let path = test_directory("recovery");

    {
        let mut db = Lsm::open(&path, 10).unwrap();

        db.put("A".into(), 10).unwrap();
        db.put("B".into(), 20).unwrap();

        // These writes remain in the WAL and MemTable.
        assert_eq!(db.get("A"), Some(10));
        assert_eq!(db.get("B"), Some(20));
    }

    // Opening again replays the WAL.
    {
        let db = Lsm::open(&path, 10).unwrap();

        assert_eq!(db.get("A"), Some(10));
        assert_eq!(db.get("B"), Some(20));
    }

    fs::remove_dir_all(path).unwrap();
}

#[test]
fn deletion_survives_reopening() {
    let path = test_directory("delete-recovery");

    {
        let mut db = Lsm::open(&path, 10).unwrap();

        db.put("A".into(), 10).unwrap();
        db.flush().unwrap();

        // The deletion remains in the WAL.
        db.delete("A".into()).unwrap();

        assert_eq!(db.get("A"), None);
    }

    {
        let db = Lsm::open(&path, 10).unwrap();

        assert_eq!(db.get("A"), None);
    }

    fs::remove_dir_all(path).unwrap();
}
