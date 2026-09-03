use model_manager::{parse_and_normalize_model_pack_manifest, Sha256Digest};
use serde::Serialize;
use std::env;
use std::fs;

#[derive(Serialize)]
struct ManifestDigests {
    identity: model_manager::PackRevision,
    raw_document_sha256: Sha256Digest,
    canonical_document_sha256: Sha256Digest,
    normalized_manifest_sha256: Sha256Digest,
}

#[allow(clippy::print_stdout)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::args()
        .nth(1)
        .ok_or("usage: inspect_model_pack_manifest <strict-v2-manifest.json>")?;
    let bytes = fs::read(path)?;
    let normalized = parse_and_normalize_model_pack_manifest(&bytes)?;
    let output = ManifestDigests {
        identity: normalized.core_manifest.identity(),
        raw_document_sha256: Sha256Digest::of_bytes(&bytes),
        canonical_document_sha256: normalized.canonical_document_sha256,
        normalized_manifest_sha256: normalized.core_manifest.digest()?,
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
