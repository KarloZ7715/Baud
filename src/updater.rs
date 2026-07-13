//! Actualizador automatico verificado para instalaciones oficiales de Baud.
//!
//! Descubre la ultima release, verifica un manifiesto firmado y el digest del
//! asset, y reemplaza el binario y los recursos del launcher de forma atomica
//! solo cuando todo ha sido validado.

use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io::{self, Cursor, IsTerminal, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use ed25519_dalek::VerifyingKey;
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tar::Archive;
use tempfile::{Builder, TempDir};

use crate::base64;
use crate::installation::Installation;

const GITHUB_REPO: &str = "KarloZ7715/Baud";
const UPDATE_KEY_ID: &str = "baud-update-v1";
const PLATFORM: &str = "Linux_x86_64";
const PROFILE: &str = "desktop-bundle";
const ASSET_NAME: &str = "baud_Linux_x86_64.tar.gz";

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_CHECKSUM_BYTES: u64 = 1024 * 1024;
const MAX_ARCHIVE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_DECOMPRESSED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_BINARY_BYTES: u64 = 192 * 1024 * 1024;
const MAX_DESKTOP_BYTES: u64 = 64 * 1024;
const MAX_ICON_BYTES: u64 = 8 * 1024 * 1024;

/// Clave publica embebida para verificar el manifiesto de actualizacion.
const UPDATE_PUBLIC_KEY_BYTES: [u8; 32] = [
    0x6e, 0x7a, 0x14, 0x24, 0x0d, 0xaf, 0x33, 0x8d, 0x64, 0x2b, 0xab, 0x84, 0x86, 0x24, 0x61, 0x27,
    0x86, 0x9a, 0x95, 0x92, 0xf6, 0x66, 0x86, 0xbb, 0x66, 0x63, 0xb8, 0x38, 0xdf, 0x85, 0x0e, 0xa7,
];

/// Manifiesto firmado que vincula una release con su asset y digest.
#[derive(Debug, Serialize, Deserialize)]
struct UpdateManifest {
    version: u32,
    key_id: String,
    tag: String,
    platform: String,
    profile: String,
    asset: String,
    sha256: String,
}

/// Cliente HTTP minimo para descubrir releases y descargar assets.
pub trait HttpClient: Send + Sync {
    /// Descarga un cuerpo JSON con limite de tamano.
    fn get_json(
        &self,
        url: &str,
        max_bytes: u64,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>>;

    /// Descarga bytes brutos con limite de tamano.
    fn get_bytes(
        &self,
        url: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>;
}

/// Implementacion real basada en ureq.
struct UreqClient {
    agent: ureq::Agent,
}

impl UreqClient {
    fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .https_only(true)
            .max_redirects(3)
            .timeout_global(Some(Duration::from_secs(120)))
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_body(Some(Duration::from_secs(60)))
            .build();
        Self {
            agent: config.into(),
        }
    }
}

impl HttpClient for UreqClient {
    fn get_json(
        &self,
        url: &str,
        max_bytes: u64,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut response = self.agent.get(url).call()?;
        let body = response
            .body_mut()
            .with_config()
            .limit(max_bytes)
            .read_to_string()?;
        Ok(serde_json::from_str(&body)?)
    }

    fn get_bytes(
        &self,
        url: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let mut response = self.agent.get(url).call()?;
        let bytes = response
            .body_mut()
            .with_config()
            .limit(max_bytes)
            .read_to_vec()?;
        Ok(bytes)
    }
}

/// Actualizador para una instalacion oficial reconocida.
pub struct Updater {
    installation: Installation,
    client: Box<dyn HttpClient>,
    verifying_key: VerifyingKey,
    installed_version: Version,
}

impl Updater {
    pub fn new(installation: Installation) -> Self {
        let key = load_embedded_key();
        let installed_version =
            Version::parse_package(env!("CARGO_PKG_VERSION")).expect("version valida");
        Self {
            installation,
            client: Box::new(UreqClient::new()),
            verifying_key: key,
            installed_version,
        }
    }

    /// Construye un updater con un cliente HTTP y clave publica arbitrarios.
    /// Exclusivo para tests.
    #[cfg(test)]
    fn with_client_and_key(
        installation: Installation,
        client: Box<dyn HttpClient>,
        key: VerifyingKey,
        installed_version: Version,
    ) -> Self {
        Self {
            installation,
            client,
            verifying_key: key,
            installed_version,
        }
    }

    pub fn run(&self) -> Result<(), UpdateError> {
        self.ensure_supported_platform()?;

        let installed = self.installed_version.clone();

        self.println_phase("Checking for updates...");
        let release_tag = fetch_latest_release(self.client.as_ref())?;
        let release = Version::parse_tag(&release_tag)?;

        match installed.cmp(&release) {
            std::cmp::Ordering::Equal => {
                println!("Baud is already up to date (v{}).", installed);
                return Ok(());
            }
            std::cmp::Ordering::Greater => {
                return Err(UpdateError::StaleRelease {
                    installed: installed.to_string(),
                    release: release.to_string(),
                });
            }
            std::cmp::Ordering::Less => {}
        }

        self.println_phase(&format!("Latest release is v{release}. Downloading..."));

        let manifest = self.fetch_and_verify_manifest(&release_tag)?;
        let archive = self.download_asset(&manifest)?;

        self.println_phase("Verifying update archive...");
        let staging = self.stage_archive(&archive, &manifest)?;

        self.println_phase(&format!("Installing Baud v{installed} -> v{release}..."));
        commit_update(&self.installation, staging.path())?;

        self.println_phase(&format!("Updated Baud v{installed} -> v{release}."));
        Ok(())
    }

    fn ensure_supported_platform(&self) -> Result<(), UpdateError> {
        if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
            Ok(())
        } else {
            Err(UpdateError::UnsupportedPlatform)
        }
    }

    fn fetch_and_verify_manifest(&self, release_tag: &str) -> Result<UpdateManifest, UpdateError> {
        if self.verifying_key.to_bytes() == [0u8; 32] {
            return Err(UpdateError::KeyNotProvisioned);
        }

        let manifest_url = format!(
            "https://github.com/{GITHUB_REPO}/releases/download/{release_tag}/update-manifest.json"
        );
        let sig_url = format!(
            "https://github.com/{GITHUB_REPO}/releases/download/{release_tag}/update-manifest.sig"
        );

        // Descargamos el manifiesto como bytes brutos para verificar la firma
        // sobre el payload exacto publicado; despues lo parseamos como JSON.
        let manifest_bytes = self
            .client
            .get_bytes(&manifest_url, MAX_MANIFEST_BYTES)
            .map_err(|e| UpdateError::ManifestFetch(e.to_string()))?;

        let sig_b64 = self
            .client
            .get_bytes(&sig_url, MAX_MANIFEST_BYTES)
            .map_err(|e| UpdateError::ManifestFetch(e.to_string()))?;
        let sig_bytes = base64::decode(&sig_b64).ok_or(UpdateError::SignatureInvalid)?;
        let signature = ed25519_dalek::Signature::from_slice(&sig_bytes)
            .map_err(|_| UpdateError::SignatureInvalid)?;

        self.verifying_key
            .verify_strict(&manifest_bytes, &signature)
            .map_err(|_| UpdateError::SignatureInvalid)?;

        let manifest: UpdateManifest = serde_json::from_slice(&manifest_bytes)
            .map_err(|e| UpdateError::ManifestInvalid(e.to_string()))?;

        if manifest.version != 1
            || manifest.key_id != UPDATE_KEY_ID
            || manifest.platform != PLATFORM
            || manifest.profile != PROFILE
            || manifest.asset != ASSET_NAME
            || manifest.tag != release_tag
            || manifest.sha256.len() != 64
            || hex_decode(&manifest.sha256).is_none()
        {
            return Err(UpdateError::ManifestInvalid(
                "manifest fields do not match the update contract".into(),
            ));
        }

        Ok(manifest)
    }

    fn download_asset(&self, manifest: &UpdateManifest) -> Result<Vec<u8>, UpdateError> {
        let asset_url = format!(
            "https://github.com/{GITHUB_REPO}/releases/download/{}/{}",
            manifest.tag, manifest.asset
        );
        let checksum_url = format!(
            "https://github.com/{GITHUB_REPO}/releases/download/{}/SHA256SUMS",
            manifest.tag
        );

        let archive = self
            .client
            .get_bytes(&asset_url, MAX_ARCHIVE_BYTES)
            .map_err(|e| UpdateError::AssetFetch(e.to_string()))?;
        let checksums = self
            .client
            .get_bytes(&checksum_url, MAX_CHECKSUM_BYTES)
            .map_err(|e| UpdateError::AssetFetch(e.to_string()))?;

        let archive_digest = hex_encode(&sha256(&archive));
        if archive_digest != manifest.sha256 {
            return Err(UpdateError::ChecksumMismatch);
        }

        let entries = parse_checksums(&checksums, &manifest.asset)?;
        if entries.len() != 1 {
            return Err(UpdateError::Checksum(
                "exactly one checksum entry required".into(),
            ));
        }
        if entries[0] != manifest.sha256 {
            return Err(UpdateError::ChecksumMismatch);
        }

        Ok(archive)
    }

    fn stage_archive(
        &self,
        archive: &[u8],
        _manifest: &UpdateManifest,
    ) -> Result<TempDir, UpdateError> {
        let binary_parent = self
            .installation
            .binary_path
            .parent()
            .ok_or_else(|| UpdateError::InstallPathInvalid("missing binary parent".into()))?;

        // Los permisos restrictivos solo aplican en sistemas Unix; en Windows
        // el directorio temporal hereda la ACL del directorio padre.
        #[cfg(unix)]
        let staging = Builder::new()
            .prefix(".baud-update-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in(binary_parent)
            .map_err(UpdateError::Io)?;
        #[cfg(not(unix))]
        let staging = Builder::new()
            .prefix(".baud-update-")
            .tempdir_in(binary_parent)
            .map_err(UpdateError::Io)?;

        validate_and_extract(archive, staging.path())?;
        Ok(staging)
    }

    fn println_phase(&self, message: &str) {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        if handle.is_terminal() {
            let _ = writeln!(handle, "\x1b[1;34m>> {}\x1b[0m", message);
        } else {
            let _ = writeln!(handle, ">> {}", message);
        }
    }
}

fn load_embedded_key() -> VerifyingKey {
    if UPDATE_PUBLIC_KEY_BYTES == [0u8; 32] {
        // Clave no aprovisionada: devolvemos una clave debil que fallara en
        // verificacion para que el updater no confie en un marcador vacio.
        VerifyingKey::from_bytes(&UPDATE_PUBLIC_KEY_BYTES)
            .expect("clave de 32 bytes valida como material crudo")
    } else {
        VerifyingKey::from_bytes(&UPDATE_PUBLIC_KEY_BYTES)
            .expect("UPDATE_PUBLIC_KEY_BYTES debe ser una clave publica Ed25519 valida")
    }
}

fn fetch_latest_release(client: &dyn HttpClient) -> Result<String, UpdateError> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let json = client
        .get_json(&url, MAX_MANIFEST_BYTES)
        .map_err(|e| UpdateError::ReleaseFetch(e.to_string()))?;
    let tag = json["tag_name"]
        .as_str()
        .ok_or_else(|| UpdateError::ReleaseFetch("missing tag_name".into()))?;
    if !tag.starts_with('v') {
        return Err(UpdateError::ReleaseFetch("tag must start with 'v'".into()));
    }
    Version::parse_tag(tag)?;
    Ok(tag.to_string())
}

fn validate_and_extract(archive: &[u8], staging: &Path) -> Result<(), UpdateError> {
    let decoder = GzDecoder::new(Cursor::new(archive));
    let mut archive_reader = Archive::new(decoder.take(MAX_DECOMPRESSED_BYTES));

    let mut seen = HashSet::new();
    let mut decompressed_total: u64 = 0;

    for entry in archive_reader
        .entries()
        .map_err(|e| UpdateError::ArchiveInvalid(e.to_string()))?
    {
        let mut entry = entry.map_err(|e| UpdateError::ArchiveInvalid(e.to_string()))?;
        let path = entry
            .path()
            .map_err(|e| UpdateError::ArchiveInvalid(e.to_string()))?
            .into_owned();

        if !is_safe_archive_path(&path) {
            return Err(UpdateError::ArchiveInvalid(format!(
                "forbidden archive path: {}",
                path.display()
            )));
        }

        let header = entry.header();
        let entry_type = header.entry_type();

        if entry_type.is_file() {
            let size = header.size().unwrap_or(0);
            validate_file_size(&path, size)?;
            decompressed_total += size;
            if decompressed_total > MAX_DECOMPRESSED_BYTES {
                return Err(UpdateError::ArchiveInvalid(
                    "decompressed size exceeds limit".into(),
                ));
            }
            entry
                .unpack_in(staging)
                .map_err(|e| UpdateError::ArchiveInvalid(format!("extraction failed: {e}")))?;
        } else if entry_type.is_dir() {
            if !allowed_dir(&path) {
                return Err(UpdateError::ArchiveInvalid(format!(
                    "unexpected directory: {}",
                    path.display()
                )));
            }
            entry
                .unpack_in(staging)
                .map_err(|e| UpdateError::ArchiveInvalid(format!("extraction failed: {e}")))?;
        } else {
            return Err(UpdateError::ArchiveInvalid(format!(
                "unexpected entry type in archive: {}",
                path.display()
            )));
        }

        if !seen.insert(path.clone()) {
            return Err(UpdateError::ArchiveInvalid(format!(
                "duplicate archive entry: {}",
                path.display()
            )));
        }
    }

    verify_staging_contents(staging)?;
    Ok(())
}

fn is_safe_archive_path(path: &Path) -> bool {
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            _ => return false,
        }
    }
    true
}

fn validate_file_size(path: &Path, size: u64) -> Result<(), UpdateError> {
    let limit = match path.as_os_str().to_str() {
        Some("baud") => MAX_BINARY_BYTES,
        Some("share/applications/baud.desktop") => MAX_DESKTOP_BYTES,
        Some("share/icons/hicolor/48x48/apps/baud.png") => MAX_ICON_BYTES,
        Some("share/icons/hicolor/256x256/apps/baud.png") => MAX_ICON_BYTES,
        _ => {
            return Err(UpdateError::ArchiveInvalid(format!(
                "unexpected file: {}",
                path.display()
            )))
        }
    };
    if size > limit {
        return Err(UpdateError::ArchiveInvalid(format!(
            "{} exceeds size limit",
            path.display()
        )));
    }
    Ok(())
}

fn allowed_dir(path: &Path) -> bool {
    let s = path.to_string_lossy().trim_end_matches('/').to_string();
    matches!(
        s.as_str(),
        "share"
            | "share/applications"
            | "share/icons"
            | "share/icons/hicolor"
            | "share/icons/hicolor/48x48"
            | "share/icons/hicolor/48x48/apps"
            | "share/icons/hicolor/256x256"
            | "share/icons/hicolor/256x256/apps"
    )
}

fn verify_staging_contents(staging: &Path) -> Result<(), UpdateError> {
    let expected = [
        staging.join("baud"),
        staging.join("share/applications/baud.desktop"),
        staging.join("share/icons/hicolor/48x48/apps/baud.png"),
        staging.join("share/icons/hicolor/256x256/apps/baud.png"),
    ];

    for path in &expected {
        if !path.is_file() {
            return Err(UpdateError::ArchiveInvalid(format!(
                "missing extracted file: {}",
                path.display()
            )));
        }
    }

    // Rechaza archivos extra en el staging.
    let mut found = 0;
    walk_files(staging, &mut |path| {
        found += 1;
        if !expected.iter().any(|e| e == path) {
            return Err(UpdateError::ArchiveInvalid(format!(
                "extra file in archive: {}",
                path.display()
            )));
        }
        Ok(())
    })?;

    if found != expected.len() {
        return Err(UpdateError::ArchiveInvalid(
            "archive contents do not match expected bundle".into(),
        ));
    }

    Ok(())
}

fn walk_files(
    dir: &Path,
    cb: &mut dyn FnMut(&Path) -> Result<(), UpdateError>,
) -> Result<(), UpdateError> {
    for entry in fs::read_dir(dir).map_err(UpdateError::Io)? {
        let entry = entry.map_err(UpdateError::Io)?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).map_err(UpdateError::Io)?;
        if meta.file_type().is_symlink() {
            return Err(UpdateError::ArchiveInvalid(format!(
                "symlink not allowed: {}",
                path.display()
            )));
        }
        if meta.is_dir() {
            walk_files(&path, cb)?;
        } else if meta.is_file() {
            cb(&path)?;
        }
    }
    Ok(())
}

fn commit_update(installation: &Installation, staging: &Path) -> Result<(), UpdateError> {
    let resources = [
        (
            staging.join("share/applications/baud.desktop"),
            installation.data_dir.join("applications/baud.desktop"),
        ),
        (
            staging.join("share/icons/hicolor/48x48/apps/baud.png"),
            installation
                .data_dir
                .join("icons/hicolor/48x48/apps/baud.png"),
        ),
        (
            staging.join("share/icons/hicolor/256x256/apps/baud.png"),
            installation
                .data_dir
                .join("icons/hicolor/256x256/apps/baud.png"),
        ),
    ];

    let mut committed: Vec<PathBuf> = Vec::new();

    for (source, target) in &resources {
        if let Err(e) = install_file(source, target) {
            for target in committed.iter().rev() {
                let _ = restore_backup(target);
            }
            return Err(e);
        }
        committed.push(target.clone());
    }

    if let Err(e) = install_binary(&staging.join("baud"), &installation.binary_path) {
        for target in committed.iter().rev() {
            let _ = restore_backup(target);
        }
        return Err(e);
    }

    // Limpia los backups tras un commit exitoso.
    for (_, target) in &resources {
        let _ = fs::remove_file(backup_path(target));
    }
    let _ = fs::remove_file(backup_path(&installation.binary_path));

    Ok(())
}

fn install_file(source: &Path, target: &Path) -> Result<(), UpdateError> {
    ensure_parent_dir(target)?;
    let tmp = target.with_extension("baud-update.tmp");
    let bak = backup_path(target);

    fs::copy(source, &tmp).map_err(UpdateError::Io)?;
    fs::rename(target, &bak)
        .or_else(|e| {
            // El target puede no existir en una instalacion previa sin recursos.
            if e.kind() == io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(e)
            }
        })
        .map_err(UpdateError::Io)?;
    fs::rename(&tmp, target).map_err(|e| {
        let _ = restore_backup(target);
        UpdateError::Io(e)
    })?;

    sync_path(target)?;
    sync_path(target.parent().unwrap_or(target))?;
    Ok(())
}

fn install_binary(source: &Path, target: &Path) -> Result<(), UpdateError> {
    let tmp = target.with_extension("baud-update.tmp");
    let bak = backup_path(target);

    fs::copy(source, &tmp).map_err(UpdateError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        fs::set_permissions(&tmp, perms).map_err(UpdateError::Io)?;
    }
    fs::rename(target, &bak).map_err(UpdateError::Io)?;
    fs::rename(&tmp, target).map_err(|e| {
        let _ = fs::rename(&bak, target);
        UpdateError::Io(e)
    })?;

    sync_path(target)?;
    if let Some(parent) = target.parent() {
        sync_path(parent)?;
    }
    Ok(())
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("baud-update.bak")
}

fn restore_backup(target: &Path) -> Result<(), UpdateError> {
    let bak = backup_path(target);
    if bak.exists() {
        fs::rename(&bak, target).map_err(UpdateError::Io)?;
    }
    Ok(())
}

fn ensure_parent_dir(path: &Path) -> Result<(), UpdateError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(UpdateError::Io)?;
    }
    Ok(())
}

#[cfg(unix)]
fn sync_path(path: &Path) -> Result<(), UpdateError> {
    let file = fs::File::open(path).map_err(UpdateError::Io)?;
    file.sync_all().map_err(UpdateError::Io)?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_path(_path: &Path) -> Result<(), UpdateError> {
    Ok(())
}

fn parse_checksums(bytes: &[u8], asset: &str) -> Result<Vec<String>, UpdateError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| UpdateError::Checksum("SHA256SUMS is not valid UTF-8".into()))?;
    let mut matches = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let digest = parts
            .next()
            .ok_or_else(|| UpdateError::Checksum("missing digest".into()))?;
        let filename = parts
            .next()
            .ok_or_else(|| UpdateError::Checksum("missing filename".into()))?;
        if filename != asset {
            continue;
        }
        if digest.len() != 64 || hex_decode(digest).is_none() {
            return Err(UpdateError::Checksum("invalid digest".into()));
        }
        matches.push(digest.to_lowercase());
    }
    Ok(matches)
}

fn sha256(bytes: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().to_vec()
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for chunk in s.as_bytes().chunks(2) {
        let high = hex_digit(chunk[0])?;
        let low = hex_digit(chunk[1])?;
        out.push((high << 4) | low);
    }
    Some(out)
}

fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Version semantica de tres componentes numericos.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Version {
    major: u32,
    minor: u32,
    patch: u32,
}

impl Version {
    fn parse_package(s: &str) -> Result<Self, UpdateError> {
        Self::parse_components(s)
    }

    fn parse_tag(s: &str) -> Result<Self, UpdateError> {
        let s = s
            .strip_prefix('v')
            .ok_or_else(|| UpdateError::VersionParse(s.into()))?;
        Self::parse_components(s)
    }

    fn parse_components(s: &str) -> Result<Self, UpdateError> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return Err(UpdateError::VersionParse(s.into()));
        }
        let major = parts[0]
            .parse()
            .map_err(|_| UpdateError::VersionParse(s.into()))?;
        let minor = parts[1]
            .parse()
            .map_err(|_| UpdateError::VersionParse(s.into()))?;
        let patch = parts[2]
            .parse()
            .map_err(|_| UpdateError::VersionParse(s.into()))?;
        Ok(Self {
            major,
            minor,
            patch,
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Errores que pueden ocurrir durante una actualizacion.
#[derive(Debug)]
pub enum UpdateError {
    KeyNotProvisioned,
    UnsupportedPlatform,
    ReleaseFetch(String),
    ManifestFetch(String),
    AssetFetch(String),
    ManifestInvalid(String),
    SignatureInvalid,
    Checksum(String),
    ChecksumMismatch,
    ArchiveInvalid(String),
    InstallPathInvalid(String),
    Io(io::Error),
    VersionParse(String),
    StaleRelease { installed: String, release: String },
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpdateError::KeyNotProvisioned => {
                write!(f, "update signing key is not provisioned in this build")
            }
            UpdateError::UnsupportedPlatform => {
                write!(f, "self-update is only supported on Linux x86_64")
            }
            UpdateError::ReleaseFetch(msg) => write!(f, "failed to fetch release info: {msg}"),
            UpdateError::ManifestFetch(msg) => write!(f, "failed to fetch manifest: {msg}"),
            UpdateError::AssetFetch(msg) => write!(f, "failed to download update: {msg}"),
            UpdateError::ManifestInvalid(msg) => write!(f, "invalid update manifest: {msg}"),
            UpdateError::SignatureInvalid => write!(f, "manifest signature verification failed"),
            UpdateError::Checksum(msg) => write!(f, "checksum file error: {msg}"),
            UpdateError::ChecksumMismatch => write!(f, "archive checksum does not match manifest"),
            UpdateError::ArchiveInvalid(msg) => write!(f, "archive validation failed: {msg}"),
            UpdateError::InstallPathInvalid(msg) => write!(f, "invalid install path: {msg}"),
            UpdateError::Io(e) => write!(f, "filesystem error: {e}"),
            UpdateError::VersionParse(s) => write!(f, "invalid version tag: {s}"),
            UpdateError::StaleRelease { installed, release } => write!(
                f,
                "installed version (v{installed}) is newer than the latest release (v{release})"
            ),
        }
    }
}

impl std::error::Error for UpdateError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installation::Installation;
    use ed25519_dalek::{Signer, SigningKey};
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct MockClient {
        responses: Mutex<HashMap<String, Vec<u8>>>,
        json_responses: Mutex<HashMap<String, serde_json::Value>>,
    }

    impl MockClient {
        fn new() -> Self {
            Self {
                responses: Mutex::new(HashMap::new()),
                json_responses: Mutex::new(HashMap::new()),
            }
        }

        fn set_bytes(&self, url: &str, bytes: Vec<u8>) {
            self.responses
                .lock()
                .unwrap()
                .insert(url.to_string(), bytes);
        }

        fn set_json(&self, url: &str, value: serde_json::Value) {
            self.json_responses
                .lock()
                .unwrap()
                .insert(url.to_string(), value);
        }
    }

    impl HttpClient for MockClient {
        fn get_json(
            &self,
            url: &str,
            _max_bytes: u64,
        ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
            self.json_responses
                .lock()
                .unwrap()
                .get(url)
                .cloned()
                .ok_or_else(|| format!("no mock json for {url}").into())
        }

        fn get_bytes(
            &self,
            url: &str,
            _max_bytes: u64,
        ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
            self.responses
                .lock()
                .unwrap()
                .get(url)
                .cloned()
                .ok_or_else(|| format!("no mock bytes for {url}").into())
        }
    }

    /// Semillas de vectores de prueba RFC 8032 conocidos.
    /// No son secretos reales; solo se usan para tests deterministicos.
    const TEST_SEED: [u8; 32] = [
        0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c,
        0xc4, 0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae,
        0x7f, 0x60,
    ];
    const TEST_BAD_SEED: [u8; 32] = [
        0x4c, 0xcd, 0x08, 0x9b, 0x28, 0xff, 0x96, 0xda, 0x9d, 0xb6, 0xc3, 0x46, 0xec, 0x11, 0x4e,
        0x0f, 0x5b, 0x8a, 0x31, 0x9f, 0x35, 0xab, 0xa6, 0x24, 0xda, 0x8c, 0xf6, 0xed, 0x4f, 0xb8,
        0xa6, 0xfb,
    ];

    fn generate_keypair() -> (SigningKey, VerifyingKey) {
        let signing = SigningKey::from_bytes(&TEST_SEED);
        let verifying = signing.verifying_key();
        (signing, verifying)
    }

    fn bad_verifying_key() -> VerifyingKey {
        SigningKey::from_bytes(&TEST_BAD_SEED).verifying_key()
    }

    fn installed_version() -> Version {
        Version::parse_package("0.0.6").unwrap()
    }

    fn make_archive(new_version: &str) -> Vec<u8> {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("staging");
        fs::create_dir_all(staging.join("share/applications")).unwrap();
        fs::create_dir_all(staging.join("share/icons/hicolor/48x48/apps")).unwrap();
        fs::create_dir_all(staging.join("share/icons/hicolor/256x256/apps")).unwrap();

        let binary = staging.join("baud");
        fs::write(&binary, format!("#!/bin/sh\necho 'baud {new_version}'\n")).unwrap();
        fs::write(
            staging.join("share/applications/baud.desktop"),
            "[Desktop Entry]\n",
        )
        .unwrap();
        fs::write(
            staging.join("share/icons/hicolor/48x48/apps/baud.png"),
            "icon48",
        )
        .unwrap();
        fs::write(
            staging.join("share/icons/hicolor/256x256/apps/baud.png"),
            "icon256",
        )
        .unwrap();

        let mut buf = Vec::new();
        {
            let encoder = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::default());
            let mut ar = tar::Builder::new(encoder);
            let mut binary_file = fs::File::open(&binary).unwrap();
            ar.append_file("baud", &mut binary_file).unwrap();
            ar.append_dir_all("share", staging.join("share")).unwrap();
            ar.into_inner().unwrap().finish().unwrap();
        }
        buf
    }

    fn build_release_fixture(
        signing: &SigningKey,
        new_version: &str,
    ) -> (MockClient, Vec<u8>, String) {
        let client = MockClient::new();
        let archive = make_archive(new_version);
        let digest = hex_encode(&sha256(&archive));

        let manifest = UpdateManifest {
            version: 1,
            key_id: UPDATE_KEY_ID.into(),
            tag: format!("v{new_version}"),
            platform: PLATFORM.into(),
            profile: PROFILE.into(),
            asset: ASSET_NAME.into(),
            sha256: digest.clone(),
        };
        // Firmamos el JSON exacto tal como se publica; el updater verifica la
        // firma sobre los bytes brutos antes de parsearlos.
        let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
        let signature = signing.sign(&manifest_bytes);
        let sig_b64 = base64::encode(&signature.to_bytes());

        client.set_json(
            &format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest"),
            serde_json::json!({"tag_name": format!("v{new_version}")}),
        );
        client.set_bytes(
            &format!(
                "https://github.com/{GITHUB_REPO}/releases/download/v{new_version}/update-manifest.json"
            ),
            manifest_bytes,
        );
        client.set_bytes(
            &format!(
                "https://github.com/{GITHUB_REPO}/releases/download/v{new_version}/update-manifest.sig"
            ),
            sig_b64.into_bytes(),
        );
        client.set_bytes(
            &format!(
                "https://github.com/{GITHUB_REPO}/releases/download/v{new_version}/{ASSET_NAME}"
            ),
            archive.clone(),
        );
        client.set_bytes(
            &format!(
                "https://github.com/{GITHUB_REPO}/releases/download/v{new_version}/SHA256SUMS"
            ),
            format!("{digest}  {ASSET_NAME}\n").into_bytes(),
        );

        (client, archive, digest)
    }

    fn make_installation(tmp: &tempfile::TempDir) -> Installation {
        let bin_dir = tmp.path().join("bin");
        let data_dir = tmp.path().join("share");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::create_dir_all(&data_dir).unwrap();
        let binary = bin_dir.join("baud");
        fs::write(&binary, "#!/bin/sh\necho 'baud 0.0.6'\n").unwrap();

        let receipt = bin_dir.join(".baud-install.toml");
        fs::write(
            &receipt,
            format!(
                "schema_version = 1\nmanaged_by = \"baud-installer\"\nbinary_path = \"{}\"\ndata_dir = \"{}\"\n",
                binary.canonicalize().unwrap().display(),
                data_dir.canonicalize().unwrap().display()
            ),
        )
        .unwrap();

        Installation {
            binary_path: binary.canonicalize().unwrap(),
            data_dir: data_dir.canonicalize().unwrap(),
        }
    }

    fn run_updater(
        installation: Installation,
        client: MockClient,
        key: VerifyingKey,
        installed: Version,
    ) -> Result<(), UpdateError> {
        Updater::with_client_and_key(installation, Box::new(client), key, installed).run()
    }

    #[test]
    fn actualiza_a_nueva_version_y_reemplaza_binario() {
        let tmp = tempfile::tempdir().unwrap();
        let installation = make_installation(&tmp);
        let (signing, key) = generate_keypair();
        let (client, _archive, _digest) = build_release_fixture(&signing, "0.0.7");

        run_updater(installation.clone(), client, key, installed_version()).unwrap();

        let new_output = std::process::Command::new(&installation.binary_path)
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&new_output.stdout).contains("baud 0.0.7"));
    }

    #[test]
    fn version_igual_no_realiza_cambios() {
        let tmp = tempfile::tempdir().unwrap();
        let installation = make_installation(&tmp);
        let (signing, key) = generate_keypair();
        let (client, _archive, _digest) = build_release_fixture(&signing, "0.0.6");

        let before = fs::metadata(&installation.binary_path)
            .unwrap()
            .modified()
            .unwrap();
        run_updater(installation.clone(), client, key, installed_version()).unwrap();
        let after = fs::metadata(&installation.binary_path)
            .unwrap()
            .modified()
            .unwrap();

        assert_eq!(before, after);
    }

    #[test]
    fn version_anterior_devuelve_stale_release() {
        let tmp = tempfile::tempdir().unwrap();
        let installation = make_installation(&tmp);
        let (signing, key) = generate_keypair();
        let (client, _archive, _digest) = build_release_fixture(&signing, "0.0.5");

        assert!(matches!(
            run_updater(installation.clone(), client, key, installed_version()),
            Err(UpdateError::StaleRelease { .. })
        ));
    }

    #[test]
    fn firma_incorrecta_rechaza_actualizacion() {
        let tmp = tempfile::tempdir().unwrap();
        let installation = make_installation(&tmp);
        let (signing, _key) = generate_keypair();
        let key_bad = bad_verifying_key();
        let (client, _archive, _digest) = build_release_fixture(&signing, "0.0.7");

        assert!(matches!(
            run_updater(installation.clone(), client, key_bad, installed_version()),
            Err(UpdateError::SignatureInvalid)
        ));
    }

    #[test]
    fn checksum_manifest_distinto_rechaza() {
        let tmp = tempfile::tempdir().unwrap();
        let installation = make_installation(&tmp);
        let (signing, key) = generate_keypair();
        let (client, mut archive, _digest) = build_release_fixture(&signing, "0.0.7");

        // Corrompemos el archive despues de generar el fixture.
        archive.push(0);
        client.set_bytes(
            &format!("https://github.com/{GITHUB_REPO}/releases/download/v0.0.7/{ASSET_NAME}"),
            archive,
        );

        assert!(matches!(
            run_updater(installation.clone(), client, key, installed_version()),
            Err(UpdateError::ChecksumMismatch)
        ));
    }

    #[test]
    fn checksums_duplicado_rechaza() {
        let tmp = tempfile::tempdir().unwrap();
        let installation = make_installation(&tmp);
        let (signing, key) = generate_keypair();
        let (client, _archive, digest) = build_release_fixture(&signing, "0.0.7");

        client.set_bytes(
            &format!("https://github.com/{GITHUB_REPO}/releases/download/v0.0.7/SHA256SUMS"),
            format!("{digest}  {ASSET_NAME}\n{digest}  {ASSET_NAME}\n").into_bytes(),
        );

        assert!(matches!(
            run_updater(installation.clone(), client, key, installed_version()),
            Err(UpdateError::Checksum(_))
        ));
    }

    #[test]
    fn archive_malformado_preserva_binario() {
        let tmp = tempfile::tempdir().unwrap();
        let installation = make_installation(&tmp);
        let (signing, key) = generate_keypair();
        let (client, _archive, digest) = build_release_fixture(&signing, "0.0.7");

        // Archive que no es gzip valido.
        client.set_bytes(
            &format!("https://github.com/{GITHUB_REPO}/releases/download/v0.0.7/{ASSET_NAME}"),
            b"not a tarball".to_vec(),
        );
        client.set_bytes(
            &format!("https://github.com/{GITHUB_REPO}/releases/download/v0.0.7/SHA256SUMS"),
            format!("{digest}  {ASSET_NAME}\n").into_bytes(),
        );

        let before = fs::read(&installation.binary_path).unwrap();
        assert!(run_updater(installation.clone(), client, key, installed_version()).is_err());
        let after = fs::read(&installation.binary_path).unwrap();
        assert_eq!(before, after);
    }
}
