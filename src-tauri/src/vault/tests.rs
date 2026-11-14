use super::*;
use std::fs;
use tempfile::tempdir;

/// Minimal parameters so the tests run fast.
const FAST: KdfParams = KdfParams { memory_kib: 8, iterations: 1, lanes: 1 };

fn new_vault(dir: &Path, pw: &str) -> Vault {
    Vault::create_with(&dir.join("t.vault"), pw, FAST).unwrap()
}

#[test]
fn create_save_unlock_roundtrip() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("t.vault");
    let mut v = new_vault(dir.path(), "correct horse");
    v.data.known_hosts.insert("10.0.0.5:22".into(), "SHA256:abc".into());
    v.save().unwrap();

    let v2 = Vault::unlock(&path, "correct horse").unwrap();
    assert_eq!(v2.data, v.data);
    assert_eq!(v2.data.known_hosts["10.0.0.5:22"], "SHA256:abc");
}

#[test]
fn wrong_password_fails() {
    let dir = tempdir().unwrap();
    new_vault(dir.path(), "right");
    let err = Vault::unlock(&dir.path().join("t.vault"), "wrong").unwrap_err();
    assert!(matches!(err, VaultError::WrongPasswordOrCorrupt));
}

#[test]
fn corrupted_ciphertext_fails() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("t.vault");
    new_vault(dir.path(), "pw");
    let mut bytes = fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    fs::write(&path, bytes).unwrap();
    let err = Vault::unlock(&path, "pw").unwrap_err();
    assert!(matches!(err, VaultError::WrongPasswordOrCorrupt));
}

#[test]
fn tampered_header_fails() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("t.vault");
    new_vault(dir.path(), "pw");
    let original = fs::read(&path).unwrap();

    // altered salt
    let mut b = original.clone();
    b[17] ^= 0x01;
    fs::write(&path, &b).unwrap();
    assert!(matches!(Vault::unlock(&path, "pw").unwrap_err(), VaultError::WrongPasswordOrCorrupt));

    // altered iterations (1 -> 2, still a valid value)
    let mut b = original.clone();
    b[9] = 2;
    fs::write(&path, &b).unwrap();
    assert!(matches!(Vault::unlock(&path, "pw").unwrap_err(), VaultError::WrongPasswordOrCorrupt));

    // unknown version
    let mut b = original.clone();
    b[4] = 9;
    fs::write(&path, &b).unwrap();
    assert!(matches!(Vault::unlock(&path, "pw").unwrap_err(), VaultError::UnsupportedVersion(9)));

    // the original still works
    fs::write(&path, &original).unwrap();
    assert!(Vault::unlock(&path, "pw").is_ok());
}

#[test]
fn absurd_kdf_params_are_rejected_before_deriving() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("t.vault");
    new_vault(dir.path(), "pw");
    let mut b = fs::read(&path).unwrap();
    b[5..9].copy_from_slice(&u32::MAX.to_le_bytes()); // huge memory request
    fs::write(&path, &b).unwrap();
    assert!(matches!(Vault::unlock(&path, "pw").unwrap_err(), VaultError::InvalidParams));
}

#[test]
fn not_a_vault_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("t.vault");
    fs::write(&path, b"hello, this is definitely not a vault file at all, sorry").unwrap();
    assert!(matches!(Vault::unlock(&path, "pw").unwrap_err(), VaultError::NotAVault));
}

#[test]
fn truncated_file_fails() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("t.vault");
    new_vault(dir.path(), "pw");
    let b = fs::read(&path).unwrap();
    fs::write(&path, &b[..40]).unwrap();
    assert!(matches!(Vault::unlock(&path, "pw").unwrap_err(), VaultError::WrongPasswordOrCorrupt));
}

#[test]
fn missing_file() {
    let dir = tempdir().unwrap();
    let err = Vault::unlock(&dir.path().join("nope.vault"), "pw").unwrap_err();
    assert!(matches!(err, VaultError::NotFound));
}

#[test]
fn create_refuses_to_overwrite() {
    let dir = tempdir().unwrap();
    new_vault(dir.path(), "pw");
    let err = Vault::create_with(&dir.path().join("t.vault"), "other", FAST).unwrap_err();
    assert!(matches!(err, VaultError::AlreadyExists));
}

#[test]
fn empty_password_is_rejected() {
    let dir = tempdir().unwrap();
    let err = Vault::create_with(&dir.path().join("t.vault"), "", FAST).unwrap_err();
    assert!(matches!(err, VaultError::EmptyPassword));
    assert!(!dir.path().join("t.vault").exists());
}

#[test]
fn change_password_reencrypts() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("t.vault");
    let mut v = new_vault(dir.path(), "old");
    v.data.known_hosts.insert("h:22".into(), "SHA256:x".into());
    v.save().unwrap();

    v.change_password("new").unwrap();

    assert!(matches!(Vault::unlock(&path, "old").unwrap_err(), VaultError::WrongPasswordOrCorrupt));
    let v2 = Vault::unlock(&path, "new").unwrap();
    assert_eq!(v2.data.known_hosts["h:22"], "SHA256:x");

    // the open vault is still usable with the new key
    v.data.known_hosts.insert("h2:22".into(), "SHA256:y".into());
    v.save().unwrap();
    assert_eq!(Vault::unlock(&path, "new").unwrap().data.known_hosts.len(), 2);
}

#[test]
fn change_to_empty_password_is_rejected_and_vault_unchanged() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("t.vault");
    let mut v = new_vault(dir.path(), "old");
    assert!(matches!(v.change_password("").unwrap_err(), VaultError::EmptyPassword));
    assert!(Vault::unlock(&path, "old").is_ok());
}

#[test]
fn backup_is_created_on_second_save() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("t.vault");
    let bak = dir.path().join("t.vault.bak");
    let v = new_vault(dir.path(), "pw");
    assert!(!bak.exists()); // first write: nothing to copy yet
    v.save().unwrap();
    assert!(bak.exists());
    assert!(Vault::unlock(&bak, "pw").is_ok());
    assert!(!dir.path().join("t.vault.tmp").exists()); // no leftover temp file
    assert!(Vault::unlock(&path, "pw").is_ok());
}

#[test]
fn every_save_uses_a_fresh_nonce() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("t.vault");
    let v = new_vault(dir.path(), "pw");
    let a = fs::read(&path).unwrap();
    v.save().unwrap();
    let b = fs::read(&path).unwrap();
    assert_eq!(a.len(), b.len());
    assert_ne!(&a[33..57], &b[33..57]); // fresh nonce
    assert_ne!(&a[57..], &b[57..]); // same content, different ciphertext
}

#[test]
fn file_does_not_contain_plaintext() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("t.vault");
    let mut v = new_vault(dir.path(), "pw");
    v.data.known_hosts.insert("secret-host-name.example:22".into(), "SHA256:topsecret".into());
    v.save().unwrap();
    let bytes = fs::read(&path).unwrap();
    let hay = String::from_utf8_lossy(&bytes);
    assert!(!hay.contains("secret-host-name"));
    assert!(!hay.contains("topsecret"));
}
