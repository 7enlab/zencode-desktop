//! R15: signed releases — offline Ed25519 (minisign) verification.
//!
//! Every ZenCode Desktop release artifact ships with a `.minisig` signature.
//! This module verifies a file against the release public key without any
//! network access. The Tauri auto-updater performs the same check before
//! installing an update; this code backs tests and any non-Tauri install path.
//!
//! Key management (generation, rotation) is documented in
//! `docs/signed-releases.md`. The secret key is NEVER in this repo.

use std::fmt;
use std::fs;
use std::path::Path;

/// Errors from release signature verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    /// Public key is not valid minisign base64.
    BadPublicKey(String),
    /// Signature file is missing or malformed.
    BadSignature(String),
    /// File could not be read.
    Io(String),
    /// Signature does not match the file (tampered or wrong key).
    InvalidSignature,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifyError::BadPublicKey(e) => write!(f, "bad release public key: {e}"),
            VerifyError::BadSignature(e) => write!(f, "bad signature file: {e}"),
            VerifyError::Io(e) => write!(f, "io error: {e}"),
            VerifyError::InvalidSignature => {
                write!(f, "INVALID signature — do not install this artifact")
            }
        }
    }
}

impl std::error::Error for VerifyError {}

/// Minimal standard-base64 decoder (RFC 4648, padded).
/// Used to unwrap the Tauri updater `pubkey`, which is the base64 encoding of
/// the whole minisign `.pub` file (comment line + key line).
fn decode_base64(s: &str) -> Result<Vec<u8>, String> {
    fn val(c: u8) -> Result<u8, String> {
        match c {
            b'A'..=b'Z' => Ok(c - b'A'),
            b'a'..=b'z' => Ok(c - b'a' + 26),
            b'0'..=b'9' => Ok(c - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err(format!("invalid base64 char: {}", c as char)),
        }
    }
    let bytes: Vec<u8> = s.bytes().filter(|&b| b != b'\n' && b != b'\r').collect();
    if !bytes.len().is_multiple_of(4) {
        return Err("base64 length not a multiple of 4".into());
    }
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut n: u32 = 0;
        let mut pad = 0;
        for (i, &c) in chunk.iter().enumerate() {
            if c == b'=' {
                pad += 1;
            } else {
                if pad > 0 {
                    return Err("padding in the middle of base64".into());
                }
                n |= (val(c)? as u32) << (18 - 6 * i);
            }
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Ok(out)
}

/// Verify `file` against a minisign signature.
///
/// `public_key_b64` is the base64 public key (the `RW...` line of the `.pub`
/// file). `signature_content` is the full text of the `.minisig` file.
///
/// Returns `Ok(())` iff the signature is valid for the exact file bytes, was
/// made by this key, and the trusted comment is covered by the global
/// signature. Legacy (non-prehashed) signatures are rejected.
pub fn verify_bytes(
    public_key_b64: &str,
    file_bytes: &[u8],
    signature_content: &str,
) -> Result<(), VerifyError> {
    verify_bytes_with_comment(public_key_b64, file_bytes, signature_content).map(|_| ())
}

/// Like [`verify_bytes`], but returns the *verified* trusted comment so the
/// caller can bind the signature to an expected version / file name (for
/// example `zencode-desktop v0.4.0 - ZenCode.AppImage`, as written by
/// `tools/sign-release.sh`) and refuse a validly signed but different or older
/// artifact. The comment is only returned after the signature checked out.
pub fn verify_bytes_with_comment(
    public_key_b64: &str,
    file_bytes: &[u8],
    signature_content: &str,
) -> Result<String, VerifyError> {
    let pk = minisign_verify::PublicKey::from_base64(public_key_b64.trim())
        .map_err(|e| VerifyError::BadPublicKey(e.to_string()))?;
    let sig = minisign_verify::Signature::decode(signature_content)
        .map_err(|e| VerifyError::BadSignature(e.to_string()))?;
    // allow_legacy = false: only prehashed (Ed25519 + BLAKE2b) signatures.
    pk.verify(file_bytes, &sig, false)
        .map_err(|_| VerifyError::InvalidSignature)?;
    Ok(sig.trusted_comment().to_string())
}

/// Verify the file at `path` using the `.minisig` file next to it
/// (`<path>.minisig`, same convention as tools/verify-release.sh).
pub fn verify_file(public_key_b64: &str, path: &Path) -> Result<(), VerifyError> {
    let sig_path = format!("{}.minisig", path.display());
    let file_bytes = fs::read(path).map_err(|e| VerifyError::Io(e.to_string()))?;
    let sig_content = fs::read_to_string(&sig_path).map_err(|e| VerifyError::Io(e.to_string()))?;
    verify_bytes(public_key_b64, &file_bytes, &sig_content)
}

/// Extract the minisign public key (`RW...` line) from the Tauri updater
/// config. Tauri stores `plugins.updater.pubkey` as base64 of the whole `.pub`
/// file. Returns `None` if the key is missing, empty, a placeholder, or does
/// not parse as a minisign public key.
pub fn updater_public_key(tauri_conf_json: &str) -> Option<String> {
    let conf: serde_json::Value = serde_json::from_str(tauri_conf_json).ok()?;
    let wrapped = conf
        .pointer("/plugins/updater/pubkey")?
        .as_str()?
        .trim()
        .to_string();
    if wrapped.is_empty() || wrapped.contains("PLACEHOLDER") {
        return None;
    }
    let pub_file = String::from_utf8(decode_base64(&wrapped).ok()?).ok()?;
    let key_line = pub_file
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("untrusted comment:"))?
        .to_string();
    minisign_verify::PublicKey::from_base64(&key_line).ok()?;
    Some(key_line)
}

/// Returns true if the Tauri updater config does NOT carry a usable public key
/// (missing, empty, placeholder or malformed) — i.e. releases CANNOT be
/// verified yet. The `release_key_is_wired` test fails the build in that case.
pub fn updater_pubkey_is_placeholder(tauri_conf_json: &str) -> bool {
    updater_public_key(tauri_conf_json).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // Test keypair (generated 2026-10-18, NEVER the release key):
    //   minisign -G -W -p r15-test.pub -s r15-test.key
    const TEST_PUBKEY: &str = "RWQdtwrmteKxjKJr7y54fi1dyz9HDRpXUOghMxrKTduBtg3oOgrttbwi";
    const TEST_FILE_BYTES: &[u8] = b"zencode test artifact\n";
    // Signature of TEST_FILE_BYTES made with the test secret key:
    //   minisign -S -W -s r15-test.key -m r15-test.txt -t "test artifact"
    const TEST_MINISIG: &str = "untrusted comment: signature from minisign secret key\nRUQdtwrmteKxjP9ZDdQqgLwUFtfreh/grgQpZ2GYWa02vXxwUAUvqgYN2Q2ibdIaxnXzScZ3Hu0ybLl6Lk0yspaZ1lxMeJ9EAA0=\ntrusted comment: test artifact\nmlfo4rsTacWkJlPODc4zGLY0JJxMUyhPHXbYaKQw5QRvMnUzEF7AafTOYbBhx22sedmMuVp0VmaGYS8YXYKaDA==\n";

    #[test]
    fn valid_signature_verifies() {
        assert!(verify_bytes(TEST_PUBKEY, TEST_FILE_BYTES, TEST_MINISIG).is_ok());
    }

    #[test]
    fn tampered_file_fails() {
        let mut tampered = TEST_FILE_BYTES.to_vec();
        tampered[0] = b'X';
        assert_eq!(
            verify_bytes(TEST_PUBKEY, &tampered, TEST_MINISIG),
            Err(VerifyError::InvalidSignature)
        );
    }

    #[test]
    fn wrong_key_fails() {
        // A different, valid-format public key must not verify the signature.
        // (Random key: "Ed" + 40 random bytes, base64 — valid structure, wrong key.)
        let other = "RWTGCiWTINb+iEMIQnVXTJ64+XVBCJj4UUGls0PrzSC0dqttm2vxwZ/0";
        assert_eq!(
            verify_bytes(other, TEST_FILE_BYTES, TEST_MINISIG),
            Err(VerifyError::InvalidSignature)
        );
    }

    #[test]
    fn bad_public_key_rejected() {
        assert!(matches!(
            verify_bytes("not-base64!!!", TEST_FILE_BYTES, TEST_MINISIG),
            Err(VerifyError::BadPublicKey(_))
        ));
    }

    #[test]
    fn malformed_signature_rejected() {
        assert!(matches!(
            verify_bytes(TEST_PUBKEY, TEST_FILE_BYTES, "garbage"),
            Err(VerifyError::BadSignature(_))
        ));
        assert!(matches!(
            verify_bytes(TEST_PUBKEY, TEST_FILE_BYTES, "untrusted comment: x\n!!!"),
            Err(VerifyError::BadSignature(_))
        ));
    }

    #[test]
    fn verify_file_roundtrip() {
        let dir = std::env::temp_dir().join(format!("zencode-r15-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("artifact.bin");
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(TEST_FILE_BYTES).unwrap();
        // verify_file looks for "<path>.minisig"
        let sig_path = format!("{}.minisig", path.display());
        fs::write(&sig_path, TEST_MINISIG).unwrap();
        assert!(verify_file(TEST_PUBKEY, &path).is_ok());
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&sig_path);
    }

    #[test]
    fn verify_file_missing_signature_errors() {
        let dir =
            std::env::temp_dir().join(format!("zencode-r15-test-nosig-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("artifact.bin");
        fs::write(&path, TEST_FILE_BYTES).unwrap();
        let err = verify_file(TEST_PUBKEY, &path).unwrap_err();
        assert!(matches!(err, VerifyError::Io(_)));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn placeholder_detection() {
        let with_placeholder =
            r#"{"plugins":{"updater":{"pubkey":"PLACEHOLDER-D3-CHECKLIST: generate"}}}"#;
        assert!(updater_pubkey_is_placeholder(with_placeholder));
        assert!(updater_pubkey_is_placeholder(
            r#"{"plugins":{"updater":{"pubkey":""}}}"#
        ));
        assert!(updater_pubkey_is_placeholder(r#"{"plugins":{}}"#));
        assert!(updater_pubkey_is_placeholder("not json"));
        // A bare RW... key is NOT what Tauri expects (it wants the base64 of
        // the whole .pub file), so it must not count as wired.
        let bare = r#"{"plugins":{"updater":{"pubkey":"RWQdtwrmteKxjKJr7y54fi1dyz9HDRpXUOghMxrKTduBtg3oOgrttbwi"}}}"#;
        assert!(updater_pubkey_is_placeholder(bare));
        // base64("untrusted comment: minisign public key 8CB1E2B5E60AB71D\nRWQdtw...\n")
        let wrapped = r#"{"plugins":{"updater":{"pubkey":"dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXkgOENCMUUyQjVFNjBBQjcxRApSV1FkdHdybXRlS3hqS0pyN3k1NGZpMWR5ejlIRFJwWFVPZ2hNeHJLVGR1QnRnM29PZ3J0dGJ3aQo="}}}"#;
        assert!(!updater_pubkey_is_placeholder(wrapped));
        assert_eq!(updater_public_key(wrapped).as_deref(), Some(TEST_PUBKEY));
    }

    #[test]
    fn trusted_comment_is_returned_only_after_verification() {
        assert_eq!(
            verify_bytes_with_comment(TEST_PUBKEY, TEST_FILE_BYTES, TEST_MINISIG).unwrap(),
            "test artifact"
        );
        // Editing the trusted comment breaks the global signature.
        let forged =
            TEST_MINISIG.replace("trusted comment: test artifact", "trusted comment: v9.9.9");
        assert_eq!(
            verify_bytes_with_comment(TEST_PUBKEY, TEST_FILE_BYTES, &forged),
            Err(VerifyError::InvalidSignature)
        );
    }

    /// Release gate: the real updater key in tauri.conf.json must be wired and
    /// must match the published `docs/keys/zencode-release.pub`.
    #[test]
    fn release_key_is_wired() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let conf = fs::read_to_string(root.join("desktop/src-tauri/tauri.conf.json")).unwrap();
        let pub_file = fs::read_to_string(root.join("docs/keys/zencode-release.pub")).unwrap();
        let expected = pub_file.lines().nth(1).unwrap().trim();
        assert_eq!(updater_public_key(&conf).as_deref(), Some(expected));
    }

    #[test]
    fn base64_decoder_vectors() {
        assert_eq!(decode_base64("").unwrap(), b"");
        assert_eq!(decode_base64("Zg==").unwrap(), b"f");
        assert_eq!(decode_base64("Zm8=").unwrap(), b"fo");
        assert_eq!(decode_base64("Zm9v").unwrap(), b"foo");
        assert!(decode_base64("!").is_err());
        assert!(decode_base64("abc").is_err()); // not multiple of 4
    }
}
