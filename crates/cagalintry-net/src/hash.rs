//! Hashing helpers.
//!
//! Every file this launcher writes is verified against a hash from a manifest
//! before it is used. A truncated download and a tampered jar look identical on
//! disk otherwise, and the failure surfaces much later as a confusing crash.

use std::path::Path;

use sha1::Digest as _;
use tokio::io::AsyncReadExt as _;

/// 64 KiB keeps the syscall count low without holding a large buffer per task
/// when many files hash concurrently.
const CHUNK: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum HashError {
    #[error("reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("{path} failed verification: expected {algorithm} {expected}, got {actual}")]
    Mismatch {
        path: String,
        algorithm: &'static str,
        expected: String,
        actual: String,
    },
}

pub fn sha1_hex(bytes: &[u8]) -> String {
    hex::encode(sha1::Sha1::digest(bytes))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(bytes))
}

pub fn sha512_hex(bytes: &[u8]) -> String {
    hex::encode(sha2::Sha512::digest(bytes))
}

/// Streams the file rather than reading it whole — instance content includes
/// multi-hundred-megabyte packs, and holding those in memory to hash them is
/// needless pressure.
pub async fn sha1_file(path: &Path) -> Result<String, HashError> {
    hash_file_with(path, sha1::Sha1::new()).await
}

pub async fn sha256_file(path: &Path) -> Result<String, HashError> {
    hash_file_with(path, sha2::Sha256::new()).await
}

pub async fn sha512_file(path: &Path) -> Result<String, HashError> {
    hash_file_with(path, sha2::Sha512::new()).await
}

async fn hash_file_with<D: sha1::Digest>(path: &Path, mut digest: D) -> Result<String, HashError> {
    let io_err = |source| HashError::Io { path: path.display().to_string(), source };

    let mut file = tokio::fs::File::open(path).await.map_err(io_err)?;
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let read = file.read(&mut buffer).await.map_err(io_err)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex::encode(digest.finalize()))
}

/// Compares hashes case-insensitively — manifests in the wild use both cases.
pub fn hashes_match(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.eq_ignore_ascii_case(b)
}

/// Verifies an on-disk file against an expected SHA-1, the hash Mojang's
/// manifests use for libraries and assets.
pub async fn verify_sha1(path: &Path, expected: &str) -> Result<(), HashError> {
    let actual = sha1_file(path).await?;
    if hashes_match(&actual, expected) {
        Ok(())
    } else {
        Err(HashError::Mismatch {
            path: path.display().to_string(),
            algorithm: "sha1",
            expected: expected.to_string(),
            actual,
        })
    }
}

/// Verifies against an expected SHA-512, which is what pack manifests carry for
/// mods and other content.
pub async fn verify_sha512(path: &Path, expected: &str) -> Result<(), HashError> {
    let actual = sha512_file(path).await?;
    if hashes_match(&actual, expected) {
        Ok(())
    } else {
        Err(HashError::Mismatch {
            path: path.display().to_string(),
            algorithm: "sha512",
            expected: expected.to_string(),
            actual,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[tokio::test]
    async fn file_hashing_matches_in_memory_hashing() {
        let dir = std::env::temp_dir().join("cagalintry-hash-test");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("sample.bin");
        // Larger than one chunk, so the streaming loop actually iterates.
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        tokio::fs::write(&path, &data).await.unwrap();

        assert_eq!(sha1_file(&path).await.unwrap(), sha1_hex(&data));
        assert_eq!(sha512_file(&path).await.unwrap(), sha512_hex(&data));

        verify_sha1(&path, &sha1_hex(&data)).await.unwrap();
        let wrong = verify_sha1(&path, &"0".repeat(40)).await;
        assert!(matches!(wrong, Err(HashError::Mismatch { .. })));

        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[test]
    fn comparison_ignores_case_but_not_length() {
        assert!(hashes_match("ABCD", "abcd"));
        assert!(!hashes_match("abcd", "abcde"));
    }
}
