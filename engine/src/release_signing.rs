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
/// Test-only helper (the minisign crate handles real parsing).
#[cfg(test)]
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
/// Returns `Ok(())` iff the signature is valid for the exact file bytes.
pub fn verify_bytes(
    public_key_b64: &str,
    file_bytes: &[u8],
    signature_content: &str,
) -> Result<(), VerifyError> {
    let pk = minisign::PublicKey::from_base64(public_key_b64.trim())
        .map_err(|e| VerifyError::BadPublicKey(e.to_string()))?;
    let sigbox = minisign::SignatureBox::from_string(signature_content)
        .map_err(|e| VerifyError::BadSignature(e.to_string()))?;
    let mut reader = std::io::Cursor::new(file_bytes);
    minisign::verify(&pk, &sigbox, &mut reader, true, false, false)
        .map_err(|_| VerifyError::InvalidSignature)
}

/// Verify the file at `path` using the `.minisig` file next to it
/// (`<path>.minisig`, same convention as tools/verify-release.sh).
pub fn verify_file(public_key_b64: &str, path: &Path) -> Result<(), VerifyError> {
    let sig_path = format!("{}.minisig", path.display());
    let file_bytes = fs::read(path).map_err(|e| VerifyError::Io(e.to_string()))?;
    let sig_content = fs::read_to_string(&sig_path).map_err(|e| VerifyError::Io(e.to_string()))?;
    verify_bytes(public_key_b64, &file_bytes, &sig_content)
}

/// Returns true if the Tauri updater config still carries the placeholder
/// public key — i.e. releases CANNOT be verified yet. Used by CI to fail
/// the build before the first public release.
pub fn updater_pubkey_is_placeholder(tauri_conf_json: &str) -> bool {
    tauri_conf_json.contains("PLACEHOLDER")
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
        let dir = std::env::temp_dir().join("zencode-r15-test");
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
        let dir = std::env::temp_dir().join("zencode-r15-test-nosig");
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
        let with_key = r#"{"plugins":{"updater":{"pubkey":"RWQdtwrmteKxjKJr7y54fi1dyz9HDRpXUOghMxrKTduBtg3oOgrttbwi"}}}"#;
        assert!(!updater_pubkey_is_placeholder(with_key));
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
