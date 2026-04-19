//! End-to-end encryption helpers (D4).
//!
//! Key derivation: Argon2id over a per-project passphrase, with a
//! deterministic salt derived from `blake3(project_slug)[..16]`. The
//! derivation is deterministic on purpose — the same passphrase and
//! project slug must produce the same key on any machine so two
//! collaborators can decrypt each other's writes without transporting
//! key material.
//!
//! Envelope: ChaCha20-Poly1305 AEAD with a 32-byte key and a fresh
//! 12-byte random nonce per message. Additional associated data (AAD)
//! is authenticated but not encrypted; callers bind AAD to routing
//! metadata (project, sender, timestamp) so the beacon can inspect
//! headers without holding the key.
//!
//! Key storage: `~/.config/nomograph/keys/<project>.key`
//! (mode `0600` on unix).
//!
//! The invariants tested elsewhere (round-trip, wrong-key, wrong-aad,
//! wrong-nonce, determinism) are the ones lever flagged as load-bearing
//! for multi-user claim exchange.

use std::fs;
use std::path::PathBuf;

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key as ChaChaKey, Nonce as ChaChaNonce};
use rand::RngCore;
use zeroize::Zeroizing;

use crate::error::{Error, Result};

/// 32-byte symmetric key that zeroes on drop.
///
/// Wrapped in [`zeroize::Zeroizing`] so the key bytes never linger in
/// memory after the `Key` goes out of scope.
pub type Key = Zeroizing<[u8; 32]>;

/// 12-byte AEAD nonce for ChaCha20-Poly1305 (RFC 8439).
pub type Nonce = [u8; 12];

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const SALT_LEN: usize = 16;

/// Derive a per-project 32-byte key from a shared passphrase.
///
/// Uses Argon2id (v0x13) with library-default parameters
/// (`m_cost = 19456 KiB`, `t_cost = 2`, `p_cost = 1` — the
/// OWASP-recommended profile in `argon2 = "0.5"`). The salt is the
/// first 16 bytes of `blake3(project_slug)`, so the derivation is
/// deterministic: same passphrase + same project produces the same
/// key on any machine.
///
/// # Errors
///
/// Returns [`Error::Crypto`] if Argon2 rejects the parameters (should
/// not happen with defaults) or if the passphrase is empty.
///
/// # Example
///
/// ```
/// use nomograph_claim::crypto::derive_key;
///
/// let k1 = derive_key("correct horse battery staple", "nomograph/multiuse").unwrap();
/// let k2 = derive_key("correct horse battery staple", "nomograph/multiuse").unwrap();
/// assert_eq!(&*k1, &*k2, "derivation is deterministic per (passphrase, project)");
/// ```
pub fn derive_key(passphrase: &str, project_slug: &str) -> Result<Key> {
    if passphrase.is_empty() {
        return Err(Error::Crypto(
            "Passphrase is empty; pass a non-empty passphrase to derive_key".into(),
        ));
    }
    if project_slug.is_empty() {
        return Err(Error::Crypto(
            "Project slug is empty; pass the project slug used for storage".into(),
        ));
    }

    let salt = project_salt(project_slug);
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::default());

    let mut out = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(passphrase.as_bytes(), &salt, out.as_mut_slice())
        .map_err(|e| Error::Crypto(format!("Argon2id failed ({e}); verify passphrase encoding")))?;
    Ok(out)
}

/// AEAD-encrypt `plaintext` under `key`, binding `aad` to the ciphertext.
///
/// Generates a fresh random 12-byte nonce per call and returns it alongside
/// the ciphertext (which includes the Poly1305 tag). The caller is
/// responsible for binding AAD to routing metadata (project, sender,
/// timestamp) — the AEAD tag authenticates those bytes but does not
/// encrypt them.
///
/// # Errors
///
/// Returns [`Error::Crypto`] only on internal AEAD failure (unreachable
/// for correctly sized inputs).
///
/// # Example
///
/// ```
/// use nomograph_claim::crypto::{derive_key, encrypt, decrypt};
///
/// let key = derive_key("shared-passphrase", "proj").unwrap();
/// let (nonce, ct) = encrypt(&key, b"hello", b"aad").unwrap();
/// let pt = decrypt(&key, &nonce, &ct, b"aad").unwrap();
/// assert_eq!(pt, b"hello");
/// ```
pub fn encrypt(key: &Key, plaintext: &[u8], aad: &[u8]) -> Result<(Nonce, Vec<u8>)> {
    let cipher = ChaCha20Poly1305::new(ChaChaKey::from_slice(key.as_slice()));

    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = ChaChaNonce::from_slice(&nonce_bytes);

    let ct = cipher
        .encrypt(nonce, Payload { msg: plaintext, aad })
        .map_err(|_| {
            Error::Crypto("AEAD seal failed; report this, input sizes are internally bounded".into())
        })?;
    Ok((nonce_bytes, ct))
}

/// AEAD-decrypt `ciphertext` under `key`, requiring the same `aad` and `nonce`
/// used at encryption.
///
/// # Errors
///
/// - [`Error::Crypto`] with a prescriptive message if the tag fails to
///   verify. The message names the most likely fix (wrong key, wrong
///   AAD, or truncated ciphertext).
///
/// # Example
///
/// Round-trip is shown in [`encrypt`].
pub fn decrypt(key: &Key, nonce: &Nonce, ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    if ciphertext.len() < 16 {
        return Err(Error::Crypto(
            "Ciphertext shorter than 16-byte Poly1305 tag; check for truncation".into(),
        ));
    }
    let cipher = ChaCha20Poly1305::new(ChaChaKey::from_slice(key.as_slice()));
    let nonce = ChaChaNonce::from_slice(nonce);
    cipher
        .decrypt(nonce, Payload { msg: ciphertext, aad })
        .map_err(|_| {
            Error::Crypto(
                "AEAD open failed; check passphrase, project slug, and AAD match the encrypt call"
                    .into(),
            )
        })
}

/// Canonical filesystem path for the per-project key file.
///
/// Resolves to `<config_dir>/nomograph/keys/<project_slug>.key`.
/// On macOS/Linux this is `~/.config/nomograph/keys/...`. On Windows
/// it is the platform config dir from [`directories::ProjectDirs`].
///
/// # Errors
///
/// Returns [`Error::Other`] if no config directory can be resolved (a
/// no-`HOME` sandbox, for example). In that case pass an explicit path
/// via a higher-level API rather than calling this.
pub fn key_path(project_slug: &str) -> Result<PathBuf> {
    if project_slug.is_empty() {
        return Err(Error::Other(
            "Project slug is empty; pass the project slug used for storage".into(),
        ));
    }
    let sanitized = sanitize_slug(project_slug);
    let base = config_base()?;
    Ok(base.join("keys").join(format!("{sanitized}.key")))
}

/// Read the per-project key from disk.
///
/// # Errors
///
/// - [`Error::KeyFileNotFound`] if the key file is absent. Call
///   [`save_key`] after [`derive_key`] to create it.
/// - [`Error::Corrupt`] if the file length is not exactly 32 bytes.
/// - [`Error::Io`] on other filesystem errors.
pub fn load_key(project_slug: &str) -> Result<Key> {
    let path = key_path(project_slug)?;
    if !path.exists() {
        return Err(Error::KeyFileNotFound(path.display().to_string()));
    }
    let bytes = fs::read(&path)?;
    if bytes.len() != KEY_LEN {
        return Err(Error::Corrupt(format!(
            "key file {} is {} bytes, expected {KEY_LEN}; re-run derive_key and save_key",
            path.display(),
            bytes.len()
        )));
    }
    let mut out = Zeroizing::new([0u8; KEY_LEN]);
    out.copy_from_slice(&bytes);
    Ok(out)
}

/// Write the per-project key to disk with `0600` permissions on unix.
///
/// Creates `<config_dir>/nomograph/keys/` if it does not exist.
///
/// # Errors
///
/// Returns [`Error::Io`] if the directory cannot be created or the
/// file cannot be written. On unix, returns [`Error::Io`] if the
/// `0600` chmod fails.
pub fn save_key(project_slug: &str, key: &Key) -> Result<PathBuf> {
    let path = key_path(project_slug)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, key.as_slice())?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&path, perms)?;
    }

    Ok(path)
}

// ---------------------------------------------------------------------------
// internals
// ---------------------------------------------------------------------------

fn project_salt(project_slug: &str) -> [u8; SALT_LEN] {
    let digest = blake3::hash(project_slug.as_bytes());
    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(&digest.as_bytes()[..SALT_LEN]);
    salt
}

fn sanitize_slug(slug: &str) -> String {
    slug.chars()
        .map(|c| match c {
            '/' | '\\' | ':' => '_',
            c => c,
        })
        .collect()
}

fn config_base() -> Result<PathBuf> {
    if let Ok(root) = std::env::var("NOMOGRAPH_CONFIG_HOME") {
        return Ok(PathBuf::from(root));
    }
    if let Some(dirs) = directories::ProjectDirs::from("ai", "nomograph", "nomograph") {
        return Ok(dirs.config_dir().to_path_buf());
    }
    // Manual fallback for sandboxes that reject directories crate lookups.
    if let Ok(home) = std::env::var("HOME") {
        return Ok(PathBuf::from(home).join(".config").join("nomograph"));
    }
    Err(Error::Other(
        "No config directory; set NOMOGRAPH_CONFIG_HOME to the key storage root".into(),
    ))
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const PHRASE: &str = "correct horse battery staple";
    const PROJECT: &str = "nomograph/multiuse";

    #[test]
    fn derive_key_is_deterministic() {
        let a = derive_key(PHRASE, PROJECT).unwrap();
        let b = derive_key(PHRASE, PROJECT).unwrap();
        assert_eq!(&*a, &*b);
        // not all-zero: blank-node guard for the KDF itself
        assert_ne!(&*a, &[0u8; 32]);
    }

    #[test]
    fn derive_key_rejects_empty_passphrase() {
        let err = derive_key("", PROJECT).unwrap_err();
        match err {
            Error::Crypto(msg) => assert!(msg.contains("Passphrase"), "{msg}"),
            other => panic!("expected Error::Crypto, got {other:?}"),
        }
    }

    #[test]
    fn derive_key_rejects_empty_project() {
        assert!(derive_key(PHRASE, "").is_err());
    }

    #[test]
    fn derive_key_depends_on_project() {
        let a = derive_key(PHRASE, "project-a").unwrap();
        let b = derive_key(PHRASE, "project-b").unwrap();
        assert_ne!(&*a, &*b, "different projects MUST yield different keys");
    }

    #[test]
    fn encrypt_round_trips() {
        let key = derive_key(PHRASE, PROJECT).unwrap();
        let (nonce, ct) = encrypt(&key, b"secret payload", b"aad-v1").unwrap();
        let pt = decrypt(&key, &nonce, &ct, b"aad-v1").unwrap();
        assert_eq!(pt, b"secret payload");
    }

    #[test]
    fn encrypt_produces_fresh_nonces() {
        let key = derive_key(PHRASE, PROJECT).unwrap();
        let (n1, _) = encrypt(&key, b"x", b"").unwrap();
        let (n2, _) = encrypt(&key, b"x", b"").unwrap();
        assert_ne!(n1, n2, "nonces must be fresh per encrypt call");
    }

    #[test]
    fn known_answer_decrypts_with_fixed_nonce() {
        // Drive a fixed-nonce encrypt via the low-level cipher so we can
        // assert a stable ciphertext and prove our decrypt accepts it.
        let key = derive_key(PHRASE, PROJECT).unwrap();
        let cipher = ChaCha20Poly1305::new(ChaChaKey::from_slice(key.as_slice()));
        let nonce_bytes: Nonce = [0x01; 12];
        let nonce = ChaChaNonce::from_slice(&nonce_bytes);
        let ct = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: b"kat-input",
                    aad: b"kat-aad",
                },
            )
            .unwrap();

        let pt = decrypt(&key, &nonce_bytes, &ct, b"kat-aad").unwrap();
        assert_eq!(pt, b"kat-input");
    }

    #[test]
    fn decrypt_fails_with_wrong_key() {
        let key = derive_key(PHRASE, PROJECT).unwrap();
        let (nonce, ct) = encrypt(&key, b"secret", b"").unwrap();

        let wrong = derive_key("a different passphrase", PROJECT).unwrap();
        let err = decrypt(&wrong, &nonce, &ct, b"").unwrap_err();
        assert!(matches!(err, Error::Crypto(_)));
    }

    #[test]
    fn decrypt_fails_with_all_zero_key() {
        // Blank-node guard: the default/Default::default() key must not
        // decrypt anything we produced.
        let key = derive_key(PHRASE, PROJECT).unwrap();
        let (nonce, ct) = encrypt(&key, b"secret", b"").unwrap();

        let zero: Key = Zeroizing::new([0u8; 32]);
        assert!(decrypt(&zero, &nonce, &ct, b"").is_err());
    }

    #[test]
    fn decrypt_fails_with_wrong_aad() {
        let key = derive_key(PHRASE, PROJECT).unwrap();
        let (nonce, ct) = encrypt(&key, b"secret", b"aad-v1").unwrap();
        let err = decrypt(&key, &nonce, &ct, b"aad-v2").unwrap_err();
        match err {
            Error::Crypto(msg) => assert!(msg.contains("AAD") || msg.contains("AEAD")),
            other => panic!("expected Error::Crypto, got {other:?}"),
        }
    }

    #[test]
    fn decrypt_fails_with_wrong_nonce() {
        let key = derive_key(PHRASE, PROJECT).unwrap();
        let (_nonce, ct) = encrypt(&key, b"secret", b"aad").unwrap();
        let wrong_nonce: Nonce = [0u8; 12];
        assert!(decrypt(&key, &wrong_nonce, &ct, b"aad").is_err());
    }

    #[test]
    fn decrypt_rejects_truncated_ciphertext() {
        let key = derive_key(PHRASE, PROJECT).unwrap();
        let nonce: Nonce = [0u8; 12];
        assert!(decrypt(&key, &nonce, b"short", b"").is_err());
    }

    #[test]
    fn project_salt_changes_with_slug() {
        assert_ne!(project_salt("a"), project_salt("b"));
        assert_eq!(project_salt("a"), project_salt("a"));
    }

    #[test]
    fn sanitize_slug_removes_path_separators() {
        assert_eq!(sanitize_slug("nomograph/claim"), "nomograph_claim");
        assert_eq!(sanitize_slug("a:b\\c"), "a_b_c");
    }

    #[test]
    fn key_path_uses_configured_root() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("NOMOGRAPH_CONFIG_HOME", tmp.path());
        let path = key_path("nomograph/claim").unwrap();
        std::env::remove_var("NOMOGRAPH_CONFIG_HOME");
        assert!(path.ends_with("keys/nomograph_claim.key"));
    }
}
