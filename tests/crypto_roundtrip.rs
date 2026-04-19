//! Integration: end-to-end key derivation, save/load, encrypt/decrypt.
//!
//! Uses only the public crypto API and an isolated tempdir for the key file
//! store via `NOMOGRAPH_CONFIG_HOME`.

use nomograph_claim::Error;
use nomograph_claim::crypto::{decrypt, derive_key, encrypt, key_path, load_key, save_key};

// Env-mutation mutex. `std::env::set_var` is unsafe in edition 2024
// because it's not thread-safe; cargo runs integration tests in parallel
// within the same binary, so concurrent with_tempdir calls would race.
// The mutex serializes env mutation per-process for the whole suite.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn with_tempdir<F: FnOnce()>(f: F) {
    let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().expect("tempdir");
    // SAFETY: the mutex above serializes every env read/write in the
    // test binary, and tests in other binaries don't share this process.
    unsafe {
        std::env::set_var("NOMOGRAPH_CONFIG_HOME", tmp.path());
    }
    f();
    // SAFETY: same — mutex held for the duration of the test body.
    unsafe {
        std::env::remove_var("NOMOGRAPH_CONFIG_HOME");
    }
}

#[test]
fn save_then_load_round_trip() {
    with_tempdir(|| {
        let project = "nomograph/roundtrip";
        let key = derive_key(b"passphrase-alpha", project).unwrap();

        let path = save_key(project, &key).unwrap();
        assert!(path.exists(), "key file should exist at {}", path.display());

        let loaded = load_key(project).unwrap();
        assert_eq!(&*loaded, &*key);

        let (nonce, ct) = encrypt(&loaded, b"payload", b"aad").unwrap();
        let pt = decrypt(&loaded, &nonce, &ct, b"aad").unwrap();
        assert_eq!(pt, b"payload");
    });
}

#[test]
fn load_without_save_is_typed_not_found() {
    with_tempdir(|| {
        let err = load_key("nomograph/does-not-exist").unwrap_err();
        match err {
            Error::KeyFileNotFound(path) => {
                assert!(path.contains("nomograph_does-not-exist.key"), "{path}");
            }
            other => panic!("expected KeyFileNotFound, got {other:?}"),
        }
    });
}

#[test]
fn save_key_file_is_exactly_32_bytes() {
    with_tempdir(|| {
        let project = "nomograph/byte-length";
        let key = derive_key(b"p", project).unwrap();
        let path = save_key(project, &key).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len(), 32);
    });
}

#[cfg(unix)]
#[test]
fn save_key_sets_mode_0600_on_unix() {
    use std::os::unix::fs::PermissionsExt;
    with_tempdir(|| {
        let project = "nomograph/perms";
        let key = derive_key(b"p", project).unwrap();
        let path = save_key(project, &key).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "expected 0600, got {mode:o}");
    });
}

#[test]
fn key_path_contains_project_component() {
    with_tempdir(|| {
        let path = key_path("nomograph/multiuse").unwrap();
        let s = path.display().to_string();
        assert!(s.ends_with("keys/nomograph_multiuse.key"), "path was {s}");
    });
}

#[test]
fn two_peers_same_passphrase_decrypt_each_other() {
    // Both peers derive the key independently from the shared passphrase
    // + project slug. Ciphertext round-trips.
    let project = "nomograph/peers";
    let peer_a = derive_key(b"shared-secret", project).unwrap();
    let peer_b = derive_key(b"shared-secret", project).unwrap();

    let (nonce, ct) = encrypt(&peer_a, b"claim-bytes", b"project=nomograph/peers").unwrap();
    let pt = decrypt(&peer_b, &nonce, &ct, b"project=nomograph/peers").unwrap();
    assert_eq!(pt, b"claim-bytes");
}
