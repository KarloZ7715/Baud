//! Interfaz de linea de comandos no interactiva de Baud.
//!
//! `baud` sin argumentos continua lanzando la interfaz grafica. Los comandos
//! `update`, `version` y `help` (y sus alias) se resuelen antes de inicializar
//! winit, tracing o el reportador de panics, para que funcionen en sesiones
//! graficas rotas y nunca abran una ventana.

use std::env;
use std::ffi::OsString;

/// Codigo de exito para comandos CLI exitosos.
pub const EXIT_OK: i32 = 0;
/// Codigo de error generico para comandos CLI fallidos.
pub const EXIT_ERR: i32 = 1;

/// Texto de ayuda mostrado por `baud help` y ante un comando desconocido.
pub const HELP_TEXT: &str =
    "Usage: baud [COMMAND]\n\nCommands:\n  update    Update Baud to the latest release\n  version   Print the installed Baud version\n  help      Show this help message\n\nAliases:\n  -v, --version    Print the installed Baud version\n  -h, --help       Show this help message\n";

/// Mensaje de error ante un subcomando no reconocido.
pub const UNKNOWN_COMMAND: &str = "Error: unknown command. Run `baud help` for usage.\n";

/// Comando interpretado a partir de los argumentos del proceso.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    /// Lanzar la aplicacion grafica (comportamiento historico).
    LaunchGui,
    /// Actualizar a la ultima release oficial verificada.
    Update,
    /// Mostrar la version instalada.
    Version,
    /// Mostrar la ayuda.
    Help,
    /// Subcomando no reconocido.
    Unknown,
}

/// Parsea los argumentos del proceso en un `Command`.
///
/// El primer argumento (el nombre del ejecutable) se ignora. Si no hay mas
/// argumentos, el resultado es `LaunchGui`.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Command {
    let mut iter = args.into_iter();
    let _exe = iter.next();

    match iter.next().as_deref().and_then(|s| s.to_str()) {
        None => Command::LaunchGui,
        Some("update") => Command::Update,
        Some("version") | Some("-v") | Some("--version") => Command::Version,
        Some("help") | Some("-h") | Some("--help") => Command::Help,
        Some(_) => Command::Unknown,
    }
}

/// Ejecuta el comando correspondiente a los argumentos del proceso.
///
/// Devuelve `Ok(Some(exit_code))` cuando el comando termina el proceso sin
/// iniciar la GUI, y `Ok(None)` cuando debe continuar el lanzamiento grafico.
pub fn run() -> Result<Option<i32>, Box<dyn std::error::Error>> {
    match parse(env::args_os()) {
        Command::LaunchGui => Ok(None),
        Command::Help => {
            print!("{}", HELP_TEXT);
            Ok(Some(EXIT_OK))
        }
        Command::Version => {
            println!("baud {}", env!("CARGO_PKG_VERSION"));
            Ok(Some(EXIT_OK))
        }
        Command::Update => run_update(),
        Command::Unknown => {
            eprint!("{}", UNKNOWN_COMMAND);
            Ok(Some(EXIT_ERR))
        }
    }
}

fn run_update() -> Result<Option<i32>, Box<dyn std::error::Error>> {
    // En plataformas no soportadas fallamos antes de cualquier trabajo de red.
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        eprintln!("Error: self-update is only supported on Linux x86_64.");
        return Ok(Some(EXIT_ERR));
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        match crate::installation::resolve() {
            Ok(installation) => match crate::updater::Updater::new(installation).run() {
                Ok(()) => Ok(Some(EXIT_OK)),
                Err(e) => {
                    eprintln!("Error: {e}");
                    Ok(Some(EXIT_ERR))
                }
            },
            Err(err) => {
                err.write_to(&mut std::io::stderr())?;
                Ok(Some(EXIT_ERR))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_argumentos_lanza_gui() {
        assert_eq!(parse(vec![]), Command::LaunchGui);
        assert_eq!(parse(vec![OsString::from("baud")]), Command::LaunchGui);
    }

    #[test]
    fn alias_de_version() {
        for arg in ["version", "-v", "--version"] {
            let cmd = parse(vec![OsString::from("baud"), OsString::from(arg)]);
            assert_eq!(cmd, Command::Version, "alias fallido: {arg}");
        }
    }

    #[test]
    fn alias_de_help() {
        for arg in ["help", "-h", "--help"] {
            let cmd = parse(vec![OsString::from("baud"), OsString::from(arg)]);
            assert_eq!(cmd, Command::Help, "alias fallido: {arg}");
        }
    }

    #[test]
    fn comando_desconocido_es_unknown() {
        let cmd = parse(vec![OsString::from("baud"), OsString::from("nope")]);
        assert_eq!(cmd, Command::Unknown);
    }

    #[test]
    fn help_text_contiene_comandos_y_alias() {
        assert!(HELP_TEXT.contains("update"));
        assert!(HELP_TEXT.contains("version"));
        assert!(HELP_TEXT.contains("-v, --version"));
        assert!(HELP_TEXT.contains("-h, --help"));
    }
}
