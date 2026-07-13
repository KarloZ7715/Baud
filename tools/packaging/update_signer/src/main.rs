//! Helper para firmar el manifiesto de actualizacion de Baud.
//!
//! Uso:
//!   BAUD_UPDATE_SIGNING_KEY=<hex-seed-32-bytes> baud-update-signer <dist_dir> <tag>
//!
//! Escribe `update-manifest.json` y `update-manifest.sig` en `<dist_dir>`.
//! El JSON se emite en el orden exacto que espera el updater.

use std::fs;
use std::process::ExitCode;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signer, SigningKey};

const ASSET_NAME: &str = "baud_Linux_x86_64.tar.gz";
const KEY_ID: &str = "baud-update-v1";
const PLATFORM: &str = "Linux_x86_64";
const PROFILE: &str = "desktop-bundle";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <dist_dir> <tag>", args[0]);
        return ExitCode::FAILURE;
    }

    let dist_dir = &args[1];
    let tag = &args[2];

    if let Err(e) = run(dist_dir, tag) {
        eprintln!("Error: {e}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

fn run(dist_dir: &str, tag: &str) -> Result<(), String> {
    let key_hex = std::env::var("BAUD_UPDATE_SIGNING_KEY")
        .map_err(|_| "BAUD_UPDATE_SIGNING_KEY is not set")?;
    if key_hex.is_empty() {
        return Err("BAUD_UPDATE_SIGNING_KEY is empty".into());
    }

    let seed = decode_hex(&key_hex)?;
    let signing_key = SigningKey::from_bytes(&seed);

    let checksums_path = format!("{dist_dir}/SHA256SUMS");
    let checksums = fs::read_to_string(&checksums_path)
        .map_err(|e| format!("failed to read {checksums_path}: {e}"))?;
    let digest = find_digest(&checksums, ASSET_NAME)?;

    let manifest = format!(
        "{{\"version\":1,\"key_id\":\"{KEY_ID}\",\"tag\":\"{tag}\",\"platform\":\"{PLATFORM}\",\"profile\":\"{PROFILE}\",\"asset\":\"{ASSET_NAME}\",\"sha256\":\"{digest}\"}}"
    );

    let signature = signing_key.sign(manifest.as_bytes());
    let sig_b64 = STANDARD.encode(signature.to_bytes());

    let manifest_path = format!("{dist_dir}/update-manifest.json");
    let sig_path = format!("{dist_dir}/update-manifest.sig");

    fs::write(&manifest_path, manifest)
        .map_err(|e| format!("failed to write {manifest_path}: {e}"))?;
    fs::write(&sig_path, sig_b64)
        .map_err(|e| format!("failed to write {sig_path}: {e}"))?;

    println!("Signed update manifest written to {manifest_path}");
    Ok(())
}

fn decode_hex(s: &str) -> Result<[u8; 32], String> {
    if s.len() != 64 {
        return Err("BAUD_UPDATE_SIGNING_KEY must be 64 hex characters".into());
    }
    let mut out = [0u8; 32];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        let high = hex_digit(chunk[0])?;
        let low = hex_digit(chunk[1])?;
        out[i] = (high << 4) | low;
    }
    Ok(out)
}

fn hex_digit(c: u8) -> Result<u8, String> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(format!("invalid hex digit: {c}")),
    }
}

fn find_checksums(checksums: &str, asset: &str) -> Vec<(String, String)> {
    let mut matches = Vec::new();
    for line in checksums.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        if let (Some(digest), Some(filename)) = (parts.next(), parts.next()) {
            if filename == asset {
                matches.push((digest.to_lowercase(), filename.to_string()));
            }
        }
    }
    matches
}

fn find_digest(checksums: &str, asset: &str) -> Result<String, String> {
    let matches = find_checksums(checksums, asset);
    if matches.len() != 1 {
        return Err(format!(
            "SHA256SUMS must contain exactly one entry for {asset}"
        ));
    }
    Ok(matches[0].0.clone())
}
