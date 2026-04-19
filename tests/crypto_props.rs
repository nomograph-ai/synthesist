//! Property-style tests for the crypto module.
//!
//! `proptest` is not currently in `[dev-dependencies]` for this crate
//! (see BUILDING.md §Wave 2). Per the implementor brief ("add proptest
//! if allowed; if not, flag and skip"), we flag the absence in the
//! module header and implement the same invariants as deterministic
//! sweeps over seeded `StdRng` inputs. This gives us the coverage
//! described in BUILDING-lever-principles.md (round-trip, wrong-key,
//! determinism) without adding a new dependency mid-sprint.
//!
//! To upgrade: add `proptest = "1"` to `[dev-dependencies]` and replace
//! each `for _ in 0..N` sweep below with a `proptest!` block.

use nomograph_claim::crypto::{decrypt, derive_key, encrypt};
use rand::rngs::StdRng;
use rand::{Rng, RngCore, SeedableRng};

const ITERS: usize = 128;

/// Argon2id at default params takes ~30 ms each (memory-hard by design),
/// so we sweep fewer derivation iterations than AEAD iterations.
const KDF_ITERS: usize = 16;

fn random_bytes(rng: &mut StdRng, max_len: usize) -> Vec<u8> {
    let len = rng.gen_range(0..=max_len);
    let mut buf = vec![0u8; len];
    rng.fill_bytes(&mut buf);
    buf
}

fn random_phrase(rng: &mut StdRng) -> String {
    let len = rng.gen_range(1..=40);
    (0..len)
        .map(|_| {
            // printable ASCII, no control chars
            let c: u8 = rng.gen_range(0x20..=0x7e);
            c as char
        })
        .collect()
}

#[test]
fn prop_encrypt_decrypt_round_trip() {
    // For any plaintext + AAD, decrypt(encrypt(x, aad), aad) == x.
    let mut rng = StdRng::seed_from_u64(0xC1A1_C0DE);
    let key = derive_key(b"round-trip-passphrase", "nomograph/prop").unwrap();

    for i in 0..ITERS {
        let plaintext = random_bytes(&mut rng, 512);
        let aad = random_bytes(&mut rng, 64);

        let (nonce, ct) = encrypt(&key, &plaintext, &aad).unwrap();
        let recovered = decrypt(&key, &nonce, &ct, &aad).expect("round-trip decrypt");
        assert_eq!(recovered, plaintext, "iter {i} failed");
    }
}

#[test]
fn prop_wrong_key_always_errors() {
    // Decryption with a key derived from a *different* passphrase must
    // never succeed on a ciphertext we produced.
    let mut rng = StdRng::seed_from_u64(0xDEAD_BEEF);
    let right = derive_key(b"right-passphrase", "nomograph/prop").unwrap();
    // Pre-derive one alternate key and flip random bytes in it for each
    // iteration. This gives us many "wrong key" samples without paying
    // the Argon2id memory cost per iteration.
    let alt_base = derive_key(b"alternate-passphrase", "nomograph/prop").unwrap();

    for i in 0..ITERS {
        let pt = random_bytes(&mut rng, 256);
        let aad = random_bytes(&mut rng, 32);
        let (nonce, ct) = encrypt(&right, &pt, &aad).unwrap();

        // Perturb one byte of the alternate key so every iteration gets
        // a distinct "wrong" key that is NOT equal to `right`.
        let mut wrong_bytes = *alt_base;
        let idx = rng.gen_range(0..32);
        wrong_bytes[idx] ^= 0x01;
        let wrong: nomograph_claim::crypto::Key = zeroize::Zeroizing::new(wrong_bytes);

        assert!(
            decrypt(&wrong, &nonce, &ct, &aad).is_err(),
            "iter {i}: wrong key unexpectedly decrypted"
        );
    }
}

#[test]
fn prop_wrong_aad_always_errors() {
    let mut rng = StdRng::seed_from_u64(0xA11_BAD);
    let key = derive_key(b"aad-passphrase", "nomograph/prop").unwrap();

    for i in 0..ITERS {
        let pt = random_bytes(&mut rng, 128);
        let aad_ok = random_bytes(&mut rng, 32);
        let (nonce, ct) = encrypt(&key, &pt, &aad_ok).unwrap();

        // Flip one byte of AAD (or append if empty).
        let mut aad_bad = aad_ok.clone();
        if aad_bad.is_empty() {
            aad_bad.push(0x01);
        } else {
            let idx = rng.gen_range(0..aad_bad.len());
            aad_bad[idx] ^= 0x80;
        }

        assert!(
            decrypt(&key, &nonce, &ct, &aad_bad).is_err(),
            "iter {i}: wrong AAD unexpectedly decrypted"
        );
    }
}

#[test]
fn prop_derive_key_is_deterministic() {
    // derive_key(same_passphrase, same_project) produces same key twice
    // across a swept range of passphrase and project strings.
    let mut rng = StdRng::seed_from_u64(0x0DE7);

    for i in 0..KDF_ITERS {
        let phrase = random_phrase(&mut rng);
        let project = format!("nomograph/prop-{}", i);

        let a = derive_key(phrase.as_bytes(), &project).unwrap();
        let b = derive_key(phrase.as_bytes(), &project).unwrap();
        assert_eq!(&*a, &*b, "iter {i}: derivation not deterministic");
    }
}

#[test]
fn prop_different_projects_yield_different_keys() {
    let mut rng = StdRng::seed_from_u64(0xF00D);
    for _ in 0..KDF_ITERS {
        let phrase = random_phrase(&mut rng);
        let k_a = derive_key(phrase.as_bytes(), "nomograph/project-a").unwrap();
        let k_b = derive_key(phrase.as_bytes(), "nomograph/project-b").unwrap();
        assert_ne!(
            &*k_a, &*k_b,
            "same passphrase must derive distinct keys per project"
        );
    }
}
