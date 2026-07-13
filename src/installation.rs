//! Deteccion de instalaciones gestionadas por el instalador oficial de Baud.
//!
//! El updater solo puede actuar cuando el binario en ejecucion tiene un
//! recibo oficial co-ubicado y canonicalizado. Cualquier otro caso se
//! rechaza antes de realizar peticiones de red o mutaciones.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Nombre del recibo escrito por el instalador oficial junto al binario.
const RECEIPT_NAME: &str = ".baud-install.toml";

/// Valor que identifica al instalador oficial en el recibo.
const MANAGER: &str = "baud-installer";

/// Version de esquema soportada para el recibo.
const SCHEMA_VERSION: u32 = 1;

/// Instalacion oficial reconocida por recibo.
#[derive(Debug, Clone)]
pub struct Installation {
    /// Ruta canonicalizada del binario en ejecucion.
    pub binary_path: PathBuf,
    /// Directorio de datos del launcher para esta instalacion.
    pub data_dir: PathBuf,
}

/// Alcance de una instalacion oficial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Instalacion en el home del usuario.
    User,
    /// Instalacion en un prefijo de sistema.
    Root,
}

/// Errores de propiedad/alcance que impiden una actualizacion.
#[derive(Debug)]
pub enum OwnershipError {
    /// Instalacion no oficial: instruccion generica.
    NotOwned,
    /// Instalacion oficial anterior sin recibo: reinstalar una vez.
    LegacyLocation,
    /// Instalacion root sin privilegios: instruccion con sudo.
    RootNeedsSudo { path: PathBuf },
}

impl OwnershipError {
    pub fn write_to(&self, writer: &mut dyn Write) -> io::Result<()> {
        match self {
            OwnershipError::NotOwned => writeln!(
                writer,
                "Error: this Baud installation is not managed by the official installer. Update it using the method that installed it."
            ),
            OwnershipError::LegacyLocation => writeln!(
                writer,
                "Error: this installation predates the ownership receipt. Run the official installer once to enable `baud update`."
            ),
            OwnershipError::RootNeedsSudo { path } => writeln!(
                writer,
                "Error: this installation is owned by root. Run: sudo {} update",
                path.display()
            ),
        }
    }
}

impl fmt::Display for OwnershipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut buf = Vec::new();
        self.write_to(&mut buf).map_err(|_| fmt::Error)?;
        write!(f, "{}", String::from_utf8_lossy(&buf).trim_end())
    }
}

impl std::error::Error for OwnershipError {}

/// Contenido del recibo oficial de instalacion.
#[derive(Debug, Deserialize)]
struct Receipt {
    schema_version: u32,
    managed_by: String,
    binary_path: String,
    data_dir: String,
}

/// Resuelve la instalacion a partir del ejecutable en curso.
pub fn resolve() -> Result<Installation, OwnershipError> {
    let exe = current_exe_canonical().map_err(|_| OwnershipError::NotOwned)?;
    resolve_with_exe(&exe)
}

fn resolve_with_exe(exe: &Path) -> Result<Installation, OwnershipError> {
    let bin_dir = exe.parent().ok_or(OwnershipError::NotOwned)?;
    let receipt_path = bin_dir.join(RECEIPT_NAME);

    if !receipt_path.is_file() {
        return Err(legacy_or_not_owned(exe));
    }

    let receipt: Receipt = {
        let contents = fs::read_to_string(&receipt_path).map_err(|_| OwnershipError::NotOwned)?;
        toml::from_str(&contents).map_err(|_| OwnershipError::NotOwned)?
    };

    if receipt.schema_version != SCHEMA_VERSION || receipt.managed_by != MANAGER {
        return Err(OwnershipError::NotOwned);
    }

    let receipt_binary =
        canonical_no_symlink(&receipt.binary_path).map_err(|_| OwnershipError::NotOwned)?;
    if receipt_binary != exe {
        return Err(OwnershipError::NotOwned);
    }

    let data_dir = PathBuf::from(&receipt.data_dir);
    let data_dir_canon = canonicalize_existing(&data_dir).unwrap_or(data_dir);

    let scope = classify_scope(exe)?;
    validate_installation(exe, &receipt_path, &data_dir_canon, scope)?;

    if scope == Scope::Root && !running_as_root() {
        return Err(OwnershipError::RootNeedsSudo {
            path: exe.to_path_buf(),
        });
    }

    Ok(Installation {
        binary_path: exe.to_path_buf(),
        data_dir: data_dir_canon,
    })
}

/// Devuelve la ruta canonicalizada del ejecutable en curso.
fn current_exe_canonical() -> Result<PathBuf, io::Error> {
    std::env::current_exe().and_then(|p| p.canonicalize())
}

/// Canonicaliza una ruta, devolviendo la original si no existe todavia.
fn canonicalize_existing(path: &Path) -> Option<PathBuf> {
    path.canonicalize().ok()
}

/// Canonicaliza una ruta y rechaza symlinks en el ultimo componente.
///
/// `canonicalize` resuelve symlinks, asi que para detectar que el propio
/// path era un symlink comparamos la ruta original con su canonicalizada.
fn canonical_no_symlink(path: &str) -> Result<PathBuf, io::Error> {
    let p = PathBuf::from(path);
    let canon = p.canonicalize()?;

    // Si el path original no es absoluto, canonicalize lo convierte; eso no
    // indica symlink, solo normalizacion. Solo rechazamos si el canonico
    // difiere del absolutizado (symlink) o si el tipo de archivo es symlink.
    let meta = fs::symlink_metadata(&p)?;
    if meta.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "symlink not allowed",
        ));
    }
    Ok(canon)
}

/// Decide si la ausencia de recibo merece el hint legacy o el rechazo generico.
fn legacy_or_not_owned(exe: &Path) -> OwnershipError {
    if let Some(parent) = exe.parent() {
        if is_historical_official_dir(parent, dirs::home_dir().as_deref()) {
            return OwnershipError::LegacyLocation;
        }
    }
    OwnershipError::NotOwned
}

/// Ubicaciones oficiales historicas donde se aceptaba la instalacion sin recibo.
fn is_historical_official_dir(dir: &Path, home: Option<&Path>) -> bool {
    if let Some(home) = home {
        let user_loc: PathBuf = home.join(".local").join("bin");
        if dir == user_loc {
            return true;
        }
    }
    dir == Path::new("/usr/local/bin")
}

/// Clasifica el alcance de la instalacion a partir de la propiedad del binario.
#[cfg(unix)]
fn classify_scope(exe: &Path) -> Result<Scope, OwnershipError> {
    use std::os::unix::fs::MetadataExt;
    let meta = fs::metadata(exe).map_err(|_| OwnershipError::NotOwned)?;
    if meta.uid() == 0 {
        Ok(Scope::Root)
    } else {
        Ok(Scope::User)
    }
}

#[cfg(not(unix))]
fn classify_scope(_exe: &Path) -> Result<Scope, OwnershipError> {
    Err(OwnershipError::NotOwned)
}

/// Verifica que los paths de la instalacion sean seguros para el alcance dado.
#[cfg(unix)]
fn validate_installation(
    exe: &Path,
    receipt: &Path,
    data_dir: &Path,
    scope: Scope,
) -> Result<(), OwnershipError> {
    validate_file(exe, scope)?;
    validate_file(receipt, scope)?;

    if let Some(bin_dir) = exe.parent() {
        validate_dir(bin_dir, scope)?;
    }

    // El data_dir puede no existir si el bundle no incluyo recursos de
    // escritorio. Validamos el directorio si existe, o su padre inmediato
    // si lo vamos a crear. No seguimos hasta la raiz para no depender de
    // los permisos de directorios intermedios ajenos a la instalacion.
    if data_dir.is_dir() {
        validate_dir(data_dir, scope)?;
    } else if let Some(parent) = data_dir.parent() {
        validate_dir(parent, scope)?;
    }

    Ok(())
}

#[cfg(not(unix))]
fn validate_installation(
    _exe: &Path,
    _receipt: &Path,
    _data_dir: &Path,
    _scope: Scope,
) -> Result<(), OwnershipError> {
    Err(OwnershipError::NotOwned)
}

#[cfg(unix)]
fn validate_file(path: &Path, scope: Scope) -> Result<(), OwnershipError> {
    use std::os::unix::fs::MetadataExt;

    let meta = fs::symlink_metadata(path).map_err(|_| OwnershipError::NotOwned)?;
    if meta.file_type().is_symlink() {
        return Err(OwnershipError::NotOwned);
    }

    match scope {
        Scope::Root => {
            if meta.uid() != 0 || (meta.mode() & 0o022) != 0 {
                return Err(OwnershipError::NotOwned);
            }
        }
        Scope::User => {
            if meta.uid() != running_uid() {
                return Err(OwnershipError::NotOwned);
            }
        }
    }

    Ok(())
}

#[cfg(unix)]
fn validate_dir(path: &Path, scope: Scope) -> Result<(), OwnershipError> {
    use std::os::unix::fs::MetadataExt;

    let meta = fs::symlink_metadata(path).map_err(|_| OwnershipError::NotOwned)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(OwnershipError::NotOwned);
    }

    match scope {
        Scope::Root => {
            if meta.uid() != 0 || (meta.mode() & 0o022) != 0 {
                return Err(OwnershipError::NotOwned);
            }
        }
        Scope::User => {
            if meta.uid() != running_uid() {
                return Err(OwnershipError::NotOwned);
            }
        }
    }

    Ok(())
}

#[cfg(unix)]
fn running_as_root() -> bool {
    nix::unistd::geteuid().is_root()
}

#[cfg(unix)]
fn running_uid() -> u32 {
    nix::unistd::geteuid().as_raw()
}

#[cfg(not(unix))]
fn running_as_root() -> bool {
    false
}

#[cfg(not(unix))]
fn running_uid() -> u32 {
    u32::MAX
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_receipt_contents(binary: &Path, data_dir: &Path) -> String {
        format!(
            "# Baud official install receipt\nschema_version = 1\nmanaged_by = \"{}\"\nbinary_path = \"{}\"\ndata_dir = \"{}\"\n",
            MANAGER,
            binary.display(),
            data_dir.display()
        )
    }

    fn write_receipt(dir: &Path, binary: &Path, data_dir: &Path) -> PathBuf {
        let path = dir.join(RECEIPT_NAME);
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(make_receipt_contents(binary, data_dir).as_bytes())
            .unwrap();
        path
    }

    #[test]
    fn recibo_valido_resuelve_instalacion() {
        let tmp = tempfile::tempdir().unwrap();
        let bin_dir = tmp.path().join("bin");
        fs::create_dir(&bin_dir).unwrap();
        let data_dir = tmp.path().join("share");
        fs::create_dir_all(&data_dir).unwrap();

        let binary = bin_dir.join("baud");
        fs::write(&binary, b"binary").unwrap();

        write_receipt(&bin_dir, &binary, &data_dir);

        let inst = resolve_with_exe(&binary).unwrap();
        assert_eq!(inst.binary_path, binary.canonicalize().unwrap());
        assert_eq!(inst.data_dir, data_dir.canonicalize().unwrap());
    }

    #[test]
    fn recibo_con_path_distinto_es_rechazado() {
        let tmp = tempfile::tempdir().unwrap();
        let bin_dir = tmp.path().join("bin");
        fs::create_dir(&bin_dir).unwrap();
        let data_dir = tmp.path().join("share");
        fs::create_dir_all(&data_dir).unwrap();

        let binary = bin_dir.join("baud");
        fs::write(&binary, b"binary").unwrap();
        let other = bin_dir.join("other");
        fs::write(&other, b"other").unwrap();

        write_receipt(&bin_dir, &other, &data_dir);

        assert!(matches!(
            resolve_with_exe(&binary),
            Err(OwnershipError::NotOwned)
        ));
    }

    #[test]
    fn recibo_malformado_o_schema_incorrecto_es_rechazado() {
        let tmp = tempfile::tempdir().unwrap();
        let bin_dir = tmp.path().join("bin");
        fs::create_dir(&bin_dir).unwrap();
        let binary = bin_dir.join("baud");
        fs::write(&binary, b"binary").unwrap();

        let receipt = bin_dir.join(RECEIPT_NAME);
        fs::write(&receipt, b"not toml").unwrap();
        assert!(matches!(
            resolve_with_exe(&binary),
            Err(OwnershipError::NotOwned)
        ));

        fs::write(
            &receipt,
            "schema_version = 2\nmanaged_by = \"baud-installer\"\nbinary_path = \"x\"\ndata_dir = \"y\"\n",
        )
        .unwrap();
        assert!(matches!(
            resolve_with_exe(&binary),
            Err(OwnershipError::NotOwned)
        ));

        fs::write(
            &receipt,
            "schema_version = 1\nmanaged_by = \"other\"\nbinary_path = \"x\"\ndata_dir = \"y\"\n",
        )
        .unwrap();
        assert!(matches!(
            resolve_with_exe(&binary),
            Err(OwnershipError::NotOwned)
        ));
    }

    #[test]
    fn ubicacion_historica_sin_recibo_da_pista_legacy() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let bin_dir = home.join(".local").join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        // La deteccion legacy se basa en el directorio del binario, no en el
        // recibo. Usamos un helper inyectando el home para evitar depender de
        // la variable de entorno global durante tests paralelos.
        assert!(is_historical_official_dir(&bin_dir, Some(&home)));
        assert!(!is_historical_official_dir(
            &PathBuf::from("/opt/bin"),
            Some(&home)
        ));
    }

    #[test]
    fn ubicacion_no_oficial_sin_recibo_es_not_owned() {
        let tmp = tempfile::tempdir().unwrap();
        let bin_dir = tmp.path().join("some").join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let binary = bin_dir.join("baud");
        fs::write(&binary, b"binary").unwrap();

        assert!(matches!(
            resolve_with_exe(&binary),
            Err(OwnershipError::NotOwned)
        ));
    }

    #[test]
    fn symlink_al_recibo_es_rechazado() {
        let tmp = tempfile::tempdir().unwrap();
        let bin_dir = tmp.path().join("bin");
        fs::create_dir(&bin_dir).unwrap();
        let data_dir = tmp.path().join("share");
        fs::create_dir_all(&data_dir).unwrap();
        let binary = bin_dir.join("baud");
        fs::write(&binary, b"binary").unwrap();

        let real_receipt = tmp.path().join("real-receipt.toml");
        fs::write(&real_receipt, make_receipt_contents(&binary, &data_dir)).unwrap();
        let receipt_link = bin_dir.join(RECEIPT_NAME);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real_receipt, &receipt_link).unwrap();
        #[cfg(not(unix))]
        fs::copy(&real_receipt, &receipt_link).unwrap();

        #[cfg(unix)]
        assert!(matches!(
            resolve_with_exe(&binary),
            Err(OwnershipError::NotOwned)
        ));
    }
}
