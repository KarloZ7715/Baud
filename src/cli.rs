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
    "Usage: baud [OPTIONS] [COMMAND]\n\nCommands:\n  update    Update Baud to the latest release\n  version   Print the installed Baud version\n  help      Show this help message\n\nOptions:\n  -e <command> [args...]            Execute command and its arguments in the PTY\n      --working-directory <dir>      Set the initial working directory for the child process\n      --title <text>                 Set the initial window title\n      --app-id <id>                  Set the Wayland app_id / X11 WM_CLASS instance\n      --hold                         Keep the window open after the command exits\n\nAliases:\n  -v, --version    Print the installed Baud version\n  -h, --help       Show this help message\n";

/// Mensaje de error ante un subcomando o flag no reconocido.
pub const UNKNOWN_COMMAND: &str = "Error: unknown command. Run `baud help` for usage.\n";

/// Resultado de evaluar la CLI: salir inmediatamente o lanzar la GUI.
#[derive(Debug, PartialEq, Eq)]
pub enum CliOutcome {
    /// Salir del proceso con el codigo indicado.
    Exit(i32),
    /// Lanzar la aplicacion grafica con las opciones de arranque dadas.
    LaunchGui(LaunchOptions),
}

/// Opciones de lanzamiento de la GUI obtenidas desde los argumentos.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LaunchOptions {
    /// Comando y argumentos para ejecutar en el PTY (`-e`).
    pub command: Option<Vec<String>>,
    /// Directorio de trabajo inicial del proceso hijo.
    pub working_directory: Option<String>,
    /// Titulo inicial de la ventana.
    pub title: Option<String>,
    /// app_id de Wayland / instancia de WM_CLASS en X11.
    pub app_id: Option<String>,
    /// Mantener la ventana abierta tras salir el proceso hijo.
    pub hold: bool,
}

/// Comando interpretado a partir de los argumentos del proceso.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    /// Lanzar la aplicacion grafica con las opciones de arranque dadas.
    LaunchGui(LaunchOptions),
    /// Actualizar a la ultima release oficial verificada.
    Update,
    /// Mostrar la version instalada.
    Version,
    /// Mostrar la ayuda.
    Help,
    /// Subcomando o flag no reconocido.
    Unknown,
}

/// Parsea los argumentos del proceso en un `Command`.
///
/// El primer argumento (el nombre del ejecutable) se ignora. Si no hay mas
/// argumentos, el resultado es `LaunchGui` con las opciones por defecto. Los
/// subcomandos se reconocen solo en la primera posicion. Los flags de lanzamiento
/// pueden aparecer en cualquier orden y `-e` consume el resto de la linea de
/// comandos como el comando a ejecutar.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Command {
    let mut iter = args.into_iter();
    let _exe = iter.next();

    let Some(first) = iter.next() else {
        return Command::LaunchGui(LaunchOptions::default());
    };
    let Some(first_str) = first.to_str() else {
        return Command::Unknown;
    };

    match first_str {
        "update" => Command::Update,
        "version" | "-v" | "--version" => Command::Version,
        "help" | "-h" | "--help" => Command::Help,
        _ => parse_flags(std::iter::once(first).chain(iter)),
    }
}

fn parse_flags(mut iter: impl Iterator<Item = OsString>) -> Command {
    let mut opts = LaunchOptions::default();

    while let Some(arg) = iter.next() {
        let Some(flag) = arg.to_str() else {
            return Command::Unknown;
        };

        match flag {
            "--working-directory" => {
                let Some(value) = iter.next().and_then(|s| s.into_string().ok()) else {
                    return Command::Unknown;
                };
                opts.working_directory = Some(value);
            }
            "--title" => {
                let Some(value) = iter.next().and_then(|s| s.into_string().ok()) else {
                    return Command::Unknown;
                };
                opts.title = Some(value);
            }
            "--app-id" => {
                let Some(value) = iter.next().and_then(|s| s.into_string().ok()) else {
                    return Command::Unknown;
                };
                opts.app_id = Some(value);
            }
            "--hold" => opts.hold = true,
            "-e" => {
                let tail: Vec<String> = iter.map(|s| s.into_string().unwrap_or_default()).collect();
                if tail.is_empty() {
                    return Command::Unknown;
                }
                opts.command = Some(tail);
                return Command::LaunchGui(opts);
            }
            _ => {
                if let Some(value) = flag.strip_prefix("--working-directory=") {
                    opts.working_directory = Some(value.to_string());
                } else if let Some(value) = flag.strip_prefix("--title=") {
                    opts.title = Some(value.to_string());
                } else if let Some(value) = flag.strip_prefix("--app-id=") {
                    opts.app_id = Some(value.to_string());
                } else {
                    return Command::Unknown;
                }
            }
        }
    }

    Command::LaunchGui(opts)
}

/// Ejecuta el comando correspondiente a los argumentos del proceso.
///
/// Devuelve `Ok(CliOutcome::Exit(code))` cuando el comando termina el proceso
/// sin iniciar la GUI, y `Ok(CliOutcome::LaunchGui(opts))` cuando debe continuar
/// el lanzamiento grafico.
pub fn run() -> Result<CliOutcome, Box<dyn std::error::Error>> {
    match parse(env::args_os()) {
        Command::LaunchGui(opts) => Ok(CliOutcome::LaunchGui(opts)),
        Command::Help => {
            print!("{}", HELP_TEXT);
            Ok(CliOutcome::Exit(EXIT_OK))
        }
        Command::Version => {
            println!("baud {}", env!("CARGO_PKG_VERSION"));
            Ok(CliOutcome::Exit(EXIT_OK))
        }
        Command::Update => run_update(),
        Command::Unknown => {
            eprint!("{}", UNKNOWN_COMMAND);
            Ok(CliOutcome::Exit(EXIT_ERR))
        }
    }
}

fn run_update() -> Result<CliOutcome, Box<dyn std::error::Error>> {
    // En plataformas no soportadas fallamos antes de cualquier trabajo de red.
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        eprintln!("Error: self-update is only supported on Linux x86_64.");
        return Ok(CliOutcome::Exit(EXIT_ERR));
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        match crate::installation::resolve() {
            Ok(installation) => match crate::updater::Updater::new(installation).run() {
                Ok(()) => Ok(CliOutcome::Exit(EXIT_OK)),
                Err(e) => {
                    eprintln!("Error: {e}");
                    Ok(CliOutcome::Exit(EXIT_ERR))
                }
            },
            Err(err) => {
                err.write_to(&mut std::io::stderr())?;
                Ok(CliOutcome::Exit(EXIT_ERR))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch_opts(args: Vec<&str>) -> LaunchOptions {
        let parsed = parse(args.into_iter().map(OsString::from).collect::<Vec<_>>());
        match parsed {
            Command::LaunchGui(opts) => opts,
            _ => panic!("expected LaunchGui, got {parsed:?}"),
        }
    }

    fn parse_cmd(args: Vec<&str>) -> Command {
        parse(args.into_iter().map(OsString::from).collect::<Vec<_>>())
    }

    #[test]
    fn sin_argumentos_lanza_gui() {
        assert_eq!(
            parse_cmd(vec![]),
            Command::LaunchGui(LaunchOptions::default())
        );
        assert_eq!(
            parse_cmd(vec!["baud"]),
            Command::LaunchGui(LaunchOptions::default())
        );
    }

    #[test]
    fn alias_de_version() {
        for arg in ["version", "-v", "--version"] {
            let cmd = parse_cmd(vec!["baud", arg]);
            assert_eq!(cmd, Command::Version, "alias fallido: {arg}");
        }
    }

    #[test]
    fn alias_de_help() {
        for arg in ["help", "-h", "--help"] {
            let cmd = parse_cmd(vec!["baud", arg]);
            assert_eq!(cmd, Command::Help, "alias fallido: {arg}");
        }
    }

    #[test]
    fn subcomando_update_solo_en_primera_posicion() {
        // `update` como flag suelto es desconocido; el plan solo lo reconoce en primer lugar.
        assert_eq!(
            parse_cmd(vec!["baud", "--hold", "update"]),
            Command::Unknown
        );
    }

    #[test]
    fn comando_desconocido_es_unknown() {
        let cmd = parse_cmd(vec!["baud", "nope"]);
        assert_eq!(cmd, Command::Unknown);
    }

    #[test]
    fn flag_desconocido_es_unknown() {
        assert_eq!(parse_cmd(vec!["baud", "--bogus"]), Command::Unknown);
        assert_eq!(
            parse_cmd(vec!["baud", "--hold"]),
            Command::LaunchGui(LaunchOptions {
                hold: true,
                ..LaunchOptions::default()
            })
        );
    }

    #[test]
    fn help_text_contiene_comandos_alias_y_flags() {
        assert!(HELP_TEXT.contains("update"));
        assert!(HELP_TEXT.contains("version"));
        assert!(HELP_TEXT.contains("-v, --version"));
        assert!(HELP_TEXT.contains("-h, --help"));
        assert!(HELP_TEXT.contains("-e <command>"));
        assert!(HELP_TEXT.contains("--working-directory"));
        assert!(HELP_TEXT.contains("--title"));
        assert!(HELP_TEXT.contains("--app-id"));
        assert!(HELP_TEXT.contains("--hold"));
    }

    #[test]
    fn e_consuma_resto_como_comando_y_argumentos() {
        let opts = launch_opts(vec!["baud", "-e", "tmux", "-u"]);
        assert_eq!(opts.command, Some(vec!["tmux".into(), "-u".into()]));
    }

    #[test]
    fn e_sin_argumentos_es_unknown() {
        assert_eq!(parse_cmd(vec!["baud", "-e"]), Command::Unknown);
    }

    #[test]
    fn e_no_interpreta_tokens_posteriores_como_flags() {
        let opts = launch_opts(vec!["baud", "-e", "sh", "-c", "echo --hold"]);
        assert_eq!(
            opts.command,
            Some(vec!["sh".into(), "-c".into(), "echo --hold".into()])
        );
        assert!(!opts.hold);
    }

    #[test]
    fn working_directory_acepta_formas_larga_y_igual() {
        let opts = launch_opts(vec!["baud", "--working-directory", "/tmp"]);
        assert_eq!(opts.working_directory, Some("/tmp".into()));

        let opts = launch_opts(vec!["baud", "--working-directory=/tmp"]);
        assert_eq!(opts.working_directory, Some("/tmp".into()));
    }

    #[test]
    fn working_directory_sin_valor_es_unknown() {
        assert_eq!(
            parse_cmd(vec!["baud", "--working-directory"]),
            Command::Unknown
        );
    }

    #[test]
    fn title_acepta_formas_larga_y_igual() {
        let opts = launch_opts(vec!["baud", "--title", "Notes"]);
        assert_eq!(opts.title, Some("Notes".into()));

        let opts = launch_opts(vec!["baud", "--title=Notes"]);
        assert_eq!(opts.title, Some("Notes".into()));
    }

    #[test]
    fn title_sin_valor_es_unknown() {
        assert_eq!(parse_cmd(vec!["baud", "--title"]), Command::Unknown);
    }

    #[test]
    fn app_id_acepta_formas_larga_y_igual() {
        let opts = launch_opts(vec!["baud", "--app-id", "scratchpad"]);
        assert_eq!(opts.app_id, Some("scratchpad".into()));

        let opts = launch_opts(vec!["baud", "--app-id=scratchpad"]);
        assert_eq!(opts.app_id, Some("scratchpad".into()));
    }

    #[test]
    fn app_id_sin_valor_es_unknown() {
        assert_eq!(parse_cmd(vec!["baud", "--app-id"]), Command::Unknown);
    }

    #[test]
    fn hold_flag_parsea() {
        let opts = launch_opts(vec!["baud", "--hold"]);
        assert!(opts.hold);
    }

    #[test]
    fn flags_compuestas_parsean() {
        let opts = launch_opts(vec![
            "baud",
            "--working-directory=/tmp",
            "--title=t",
            "--hold",
            "-e",
            "sh",
            "-c",
            "pwd",
        ]);
        assert_eq!(opts.working_directory, Some("/tmp".into()));
        assert_eq!(opts.title, Some("t".into()));
        assert!(opts.hold);
        assert_eq!(
            opts.command,
            Some(vec!["sh".into(), "-c".into(), "pwd".into()])
        );
    }

    #[test]
    fn run_devuelve_outcome_correcto() {
        // Los subcomandos informativos se resuelen internamente y retornan Exit(0).
        let outcome = run_from(vec!["baud", "help"]);
        assert_eq!(outcome, CliOutcome::Exit(EXIT_OK));

        let outcome = run_from(vec!["baud", "--version"]);
        assert_eq!(outcome, CliOutcome::Exit(EXIT_OK));

        let outcome = run_from(vec!["baud", "--bogus"]);
        assert_eq!(outcome, CliOutcome::Exit(EXIT_ERR));
    }

    fn run_from(args: Vec<&str>) -> CliOutcome {
        match parse(args.into_iter().map(OsString::from).collect::<Vec<_>>()) {
            Command::LaunchGui(opts) => CliOutcome::LaunchGui(opts),
            Command::Help => CliOutcome::Exit(EXIT_OK),
            Command::Version => CliOutcome::Exit(EXIT_OK),
            Command::Unknown => CliOutcome::Exit(EXIT_ERR),
            Command::Update => CliOutcome::Exit(EXIT_OK),
        }
    }
}
