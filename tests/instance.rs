use jellymax::instance::ServerInstance;
use std::{fs, io::ErrorKind, process::Command};
use tempfile::TempDir;

#[test]
fn independent_lock_handles_conflict_until_the_owner_drops() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("server.lock");
    fs::write(&path, b"existing lock file").unwrap();

    let first = ServerInstance::acquire(dir.path()).unwrap();
    let error = ServerInstance::acquire(dir.path()).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::WouldBlock);
    assert!(
        error
            .to_string()
            .contains("Another server is already using")
    );
    assert_eq!(fs::read(&path).unwrap(), b"existing lock file");

    drop(first);
    assert_eq!(fs::read(&path).unwrap(), b"existing lock file");
    let second = ServerInstance::acquire(dir.path()).unwrap();
    drop(second);
    assert!(path.exists());
}

#[test]
fn serving_process_refuses_a_locked_directory_before_opening_the_database() {
    let dir = TempDir::new().unwrap();
    let _instance = ServerInstance::acquire(dir.path()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_jellymax"))
        .arg("--data-dir")
        .arg(dir.path())
        .args(["serve", "--bind", "127.0.0.1:0"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("Another server is already using data directory")
    );
    assert!(!dir.path().join("jellyfin.db").exists());
}
