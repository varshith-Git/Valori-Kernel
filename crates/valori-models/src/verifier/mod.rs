// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! On-demand SHA-256 re-verification for installed local models — M6.

use serde::Serialize;

use crate::downloader::sha256_hex;
use crate::manifest::ModelManifest;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum VerifyStatus {
    /// Model is remote-service — no local file to verify.
    Remote,
    /// SHA-256 matches the stored value.
    Ok,
    /// File not found on disk.
    Missing,
    /// File present but no expected SHA-256 stored (can't verify).
    Unverified,
    /// File present; SHA-256 does not match stored value.
    Corrupted,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerifyResult {
    pub id: String,
    pub status: VerifyStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_sha: Option<String>,
}

/// Re-verify an installed manifest's on-disk SHA-256. Pure — no network, no async.
pub fn verify_manifest(manifest: &ModelManifest) -> VerifyStatus {
    verify_manifest_full(manifest).status
}

/// Full verification result (id + status + hashes).
pub fn verify_manifest_full(manifest: &ModelManifest) -> VerifyResult {
    let path = match &manifest.path {
        None => {
            return VerifyResult {
                id: manifest.id.clone(),
                status: VerifyStatus::Remote,
                expected_sha: None,
                actual_sha: None,
            }
        }
        Some(p) => std::path::PathBuf::from(p),
    };

    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => {
            return VerifyResult {
                id: manifest.id.clone(),
                status: VerifyStatus::Missing,
                expected_sha: manifest.sha256.clone(),
                actual_sha: None,
            }
        }
    };

    let actual = sha256_hex(&bytes);
    let status = match &manifest.sha256 {
        None => VerifyStatus::Unverified,
        Some(expected) => {
            if *expected == actual { VerifyStatus::Ok } else { VerifyStatus::Corrupted }
        }
    };

    VerifyResult {
        id: manifest.id.clone(),
        status,
        expected_sha: manifest.sha256.clone(),
        actual_sha: Some(actual),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ManifestStatus, ModelFormat, ModelTask, ProviderKind};

    fn make_manifest(path: Option<String>, sha256: Option<String>) -> ModelManifest {
        ModelManifest {
            id: "test/model".into(),
            name: "test".into(),
            version: None,
            provider: ProviderKind::Local,
            family: None,
            task: ModelTask::Embedding,
            dimensions: 384,
            quantization: None,
            format: if path.is_some() { ModelFormat::Onnx } else { ModelFormat::Remote },
            sha256,
            size_bytes: 0,
            installed_at: Some(0),
            path,
            status: ManifestStatus::Installed,
            min_ram_mb: 0,
            license: None,
            homepage: None,
            download_url: None,
        }
    }

    #[test]
    fn remote_manifest_is_remote() {
        let m = make_manifest(None, None);
        assert_eq!(verify_manifest(&m), VerifyStatus::Remote);
    }

    #[test]
    fn missing_file_is_missing() {
        let m = make_manifest(Some("/nonexistent/path/model.bin".into()), None);
        assert_eq!(verify_manifest(&m), VerifyStatus::Missing);
    }

    #[test]
    fn valid_file_ok() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("model.bin");
        let data = b"hello valori";
        std::fs::write(&file, data).unwrap();
        let hash = sha256_hex(data);
        let m = make_manifest(Some(file.display().to_string()), Some(hash));
        assert_eq!(verify_manifest(&m), VerifyStatus::Ok);
    }

    #[test]
    fn wrong_hash_is_corrupted() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("model.bin");
        std::fs::write(&file, b"hello valori").unwrap();
        let m = make_manifest(
            Some(file.display().to_string()),
            Some("deadbeef".repeat(8)),
        );
        assert_eq!(verify_manifest(&m), VerifyStatus::Corrupted);
    }

    #[test]
    fn no_sha_stored_is_unverified() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("model.bin");
        std::fs::write(&file, b"data").unwrap();
        let m = make_manifest(Some(file.display().to_string()), None);
        assert_eq!(verify_manifest(&m), VerifyStatus::Unverified);
    }
}
