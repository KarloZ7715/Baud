//! Interfaz de linea de comandos no interactiva de Baud.
//!
//! `baud` sin argumentos es un cliente corto del daemon de sesion. Los comandos
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
    "Usage: baud [OPTIONS] [COMMAND]\n\nCommands:\n  update    Update Baud to the latest release\n  version   Print the installed Baud version\n  mcp       Speak MCP over stdio to a running Baud instance\n  help      Show this help message\n\nOptions:\n  -e <command> [args...]            Execute command and its arguments in the PTY\n      --working-directory <dir>      Set the initial working directory for the child process\n      --title <text>                 Set the initial window title\n      --app-id <id>                  Set the Wayland app_id / X11 WM_CLASS instance\n      --hold                         Keep the window open after the command exits\n      --config <path>                Load config from this file instead of the default search path\n  -o <key=value>                     Override a config key (repeatable); invalid keys are skipped\n      --window-size <COLSxROWS>      Set the initial window size in terminal cells\n      --maximized                    Start the window maximized\n      --fullscreen                   Start the window in borderless fullscreen\n      --server                       Run the session daemon in the foreground\n      --new-instance                 Open a GUI that does not talk to the daemon\n\n  mcp options:\n      --socket <path>                Control socket (default: newest instance in the runtime dir)\n      --list-tools                   Print the MCP tool catalog as JSON and exit\n\nAliases:\n  -v, --version    Print the installed Baud version\n  -h, --help       Show this help message\n";

/// Mensaje de error ante un subcomando o flag no reconocido.
pub const UNKNOWN_COMMAND: &str = "Error: unknown command. Run `baud help` for usage.\n";

/// Resultado de evaluar la CLI: salir, hablar con el daemon, o lanzar GUI.
#[derive(Debug, PartialEq, Eq)]
pub enum CliOutcome {
    /// Salir del proceso con el codigo indicado.
    Exit(i32),
    /// Cliente corto: pide una tab al daemon y sale.
    SpawnClient(LaunchOptions),
    /// Daemon de sesion en primer plano (`--server`).
    RunServer {
        config_path: Option<String>,
        overrides: Vec<String>,
    },
    /// Lanzar la aplicacion grafica sin hablar con el daemon (`--new-instance`).
    LaunchGui(LaunchOptions),
    /// Hablar MCP por stdio contra una instancia con remote_control.
    RunMcp {
        socket: Option<String>,
        list_tools: bool,
    },
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
    /// Ruta explícita de config (`--config`). None = búsqueda por defecto.
    pub config_path: Option<String>,
    /// Pares `clave=valor` de `-o`, en orden de aparición.
    pub overrides: Vec<String>,
    /// Tamaño inicial en celdas (`COLSxROWS`).
    pub window_size: Option<(u16, u16)>,
    /// Arrancar maximizado.
    pub maximized: bool,
    /// Arrancar en pantalla completa sin bordes.
    pub fullscreen: bool,
    /// No hablar con el daemon: proceso GUI clasico.
    pub new_instance: bool,
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
    /// Adaptador MCP por stdio (`baud mcp`).
    Mcp {
        socket: Option<String>,
        list_tools: bool,
    },
    /// Daemon de sesion en primer plano.
    Server {
        config_path: Option<String>,
        overrides: Vec<String>,
    },
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
        "mcp" => parse_mcp(iter),
        _ => parse_flags(std::iter::once(first).chain(iter)),
    }
}

fn parse_flags(mut iter: impl Iterator<Item = OsString>) -> Command {
    let mut opts = LaunchOptions::default();
    let mut server = false;

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
            "--config" => {
                let Some(value) = iter.next().and_then(|s| s.into_string().ok()) else {
                    return Command::Unknown;
                };
                opts.config_path = Some(value);
            }
            "-o" => {
                let Some(value) = iter.next().and_then(|s| s.into_string().ok()) else {
                    return Command::Unknown;
                };
                opts.overrides.push(value);
            }
            "--window-size" => {
                let Some(value) = iter.next().and_then(|s| s.into_string().ok()) else {
                    return Command::Unknown;
                };
                let Some(size) = parse_window_size(&value) else {
                    return Command::Unknown;
                };
                opts.window_size = Some(size);
            }
            "--maximized" => opts.maximized = true,
            "--fullscreen" => opts.fullscreen = true,
            "--server" => server = true,
            "--new-instance" => opts.new_instance = true,
            "-e" => {
                let tail: Vec<String> = iter.map(|s| s.into_string().unwrap_or_default()).collect();
                if tail.is_empty() {
                    return Command::Unknown;
                }
                opts.command = Some(tail);
                break;
            }
            _ => {
                if let Some(value) = flag.strip_prefix("--working-directory=") {
                    opts.working_directory = Some(value.to_string());
                } else if let Some(value) = flag.strip_prefix("--title=") {
                    opts.title = Some(value.to_string());
                } else if let Some(value) = flag.strip_prefix("--app-id=") {
                    opts.app_id = Some(value.to_string());
                } else if let Some(value) = flag.strip_prefix("--config=") {
                    opts.config_path = Some(value.to_string());
                } else if let Some(value) = flag.strip_prefix("-o=") {
                    opts.overrides.push(value.to_string());
                } else if let Some(value) = flag.strip_prefix("--window-size=") {
                    let Some(size) = parse_window_size(value) else {
                        return Command::Unknown;
                    };
                    opts.window_size = Some(size);
                } else {
                    return Command::Unknown;
                }
            }
        }
    }

    if server {
        return Command::Server {
            config_path: opts.config_path,
            overrides: opts.overrides,
        };
    }
    Command::LaunchGui(opts)
}

/// `COLSxROWS` en celdas; ambos lados enteros > 0. Solo `x` minúscula.
fn parse_window_size(s: &str) -> Option<(u16, u16)> {
    let (cols, rows) = s.split_once('x')?;
    let cols: u16 = cols.parse().ok()?;
    let rows: u16 = rows.parse().ok()?;
    if cols == 0 || rows == 0 {
        return None;
    }
    Some((cols, rows))
}

fn parse_mcp(mut iter: impl Iterator<Item = OsString>) -> Command {
    let mut socket = None;
    let mut list_tools = false;
    while let Some(arg) = iter.next() {
        let Some(flag) = arg.to_str() else {
            return Command::Unknown;
        };
        match flag {
            "--socket" => {
                let Some(value) = iter.next().and_then(|s| s.into_string().ok()) else {
                    return Command::Unknown;
                };
                socket = Some(value);
            }
            "--list-tools" => list_tools = true,
            other => {
                if let Some(value) = other.strip_prefix("--socket=") {
                    socket = Some(value.to_string());
                } else {
                    return Command::Unknown;
                }
            }
        }
    }
    Command::Mcp { socket, list_tools }
}

/// Ejecuta el comando correspondiente a los argumentos del proceso.
///
/// Devuelve `Exit` cuando el comando termina el proceso, `SpawnClient` para
/// `baud` sin `--new-instance`, `RunServer` para `--server`, y `LaunchGui`
/// para el escape `--new-instance`.
pub fn run() -> Result<CliOutcome, Box<dyn std::error::Error>> {
    outcome_from(parse(env::args_os()))
}

fn outcome_from(cmd: Command) -> Result<CliOutcome, Box<dyn std::error::Error>> {
    match cmd {
        Command::LaunchGui(opts) if opts.new_instance => Ok(CliOutcome::LaunchGui(opts)),
        Command::LaunchGui(opts) => Ok(CliOutcome::SpawnClient(opts)),
        Command::Server {
            config_path,
            overrides,
        } => Ok(CliOutcome::RunServer {
            config_path,
            overrides,
        }),
        Command::Help => {
            print!("{}", HELP_TEXT);
            Ok(CliOutcome::Exit(EXIT_OK))
        }
        Command::Version => {
            println!("baud {}", env!("CARGO_PKG_VERSION"));
            Ok(CliOutcome::Exit(EXIT_OK))
        }
        Command::Update => run_update(),
        Command::Mcp { socket, list_tools } => Ok(CliOutcome::RunMcp { socket, list_tools }),
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
        Ok(CliOutcome::Exit(EXIT_ERR))
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
        assert!(HELP_TEXT.contains("--config"));
        assert!(HELP_TEXT.contains("-o <key=value>"));
        assert!(HELP_TEXT.contains("--window-size"));
        assert!(HELP_TEXT.contains("--maximized"));
        assert!(HELP_TEXT.contains("--fullscreen"));
        assert!(HELP_TEXT.contains("--server"));
        assert!(HELP_TEXT.contains("--new-instance"));
        assert!(HELP_TEXT.contains("mcp"));
        assert!(HELP_TEXT.contains("--socket"));
    }

    #[test]
    fn server_flag_should_select_server_command() {
        assert!(matches!(
            parse_cmd(vec!["baud", "--server"]),
            Command::Server { .. }
        ));
    }

    #[test]
    fn new_instance_flag_should_set_launch_option() {
        let opts = launch_opts(vec!["baud", "--new-instance"]);
        assert!(opts.new_instance);
    }

    #[test]
    fn run_without_flags_should_be_spawn_client() {
        assert!(matches!(run_from(vec!["baud"]), CliOutcome::SpawnClient(_)));
    }

    #[test]
    fn server_flag_wins_over_new_instance() {
        assert!(matches!(
            parse_cmd(vec!["baud", "--server", "--new-instance"]),
            Command::Server { .. }
        ));
    }

    #[test]
    fn subcomando_mcp_sin_args() {
        assert_eq!(
            parse_cmd(vec!["baud", "mcp"]),
            Command::Mcp {
                socket: None,
                list_tools: false,
            }
        );
    }

    #[test]
    fn subcomando_mcp_con_socket() {
        assert_eq!(
            parse_cmd(vec!["baud", "mcp", "--socket", "/tmp/baud.sock"]),
            Command::Mcp {
                socket: Some("/tmp/baud.sock".into()),
                list_tools: false,
            }
        );
        assert_eq!(
            parse_cmd(vec!["baud", "mcp", "--socket=/tmp/b.sock"]),
            Command::Mcp {
                socket: Some("/tmp/b.sock".into()),
                list_tools: false,
            }
        );
    }

    #[test]
    fn mcp_list_tools_se_parsea() {
        assert_eq!(
            parse_cmd(vec!["baud", "mcp", "--list-tools"]),
            Command::Mcp {
                socket: None,
                list_tools: true
            }
        );
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
    fn config_flag_en_ambas_formas() {
        assert_eq!(
            launch_opts(vec!["baud", "--config", "/tmp/a.toml"])
                .config_path
                .as_deref(),
            Some("/tmp/a.toml")
        );
        assert_eq!(
            launch_opts(vec!["baud", "--config=/tmp/a.toml"])
                .config_path
                .as_deref(),
            Some("/tmp/a.toml")
        );
    }

    #[test]
    fn config_sin_valor_es_unknown() {
        assert_eq!(parse_cmd(vec!["baud", "--config"]), Command::Unknown);
    }

    #[test]
    fn overrides_repetibles_en_orden() {
        let opts = launch_opts(vec![
            "baud",
            "-o",
            "window.opacity=1.0",
            "-o",
            "font.size=13",
        ]);
        assert_eq!(opts.overrides, vec!["window.opacity=1.0", "font.size=13"]);
        let opts = launch_opts(vec!["baud", "-o=window.opacity=0.5"]);
        assert_eq!(opts.overrides, vec!["window.opacity=0.5"]);
    }

    #[test]
    fn o_sin_valor_es_unknown() {
        assert_eq!(parse_cmd(vec!["baud", "-o"]), Command::Unknown);
    }

    #[test]
    fn window_size_en_ambas_formas() {
        assert_eq!(
            launch_opts(vec!["baud", "--window-size", "120x40"]).window_size,
            Some((120, 40))
        );
        assert_eq!(
            launch_opts(vec!["baud", "--window-size=80x24"]).window_size,
            Some((80, 24))
        );
    }

    #[test]
    fn window_size_invalido_es_unknown() {
        assert_eq!(
            parse_cmd(vec!["baud", "--window-size", "120"]),
            Command::Unknown
        );
        assert_eq!(
            parse_cmd(vec!["baud", "--window-size", "120X40"]),
            Command::Unknown
        );
        assert_eq!(
            parse_cmd(vec!["baud", "--window-size", "0x40"]),
            Command::Unknown
        );
        assert_eq!(parse_cmd(vec!["baud", "--window-size"]), Command::Unknown);
    }

    #[test]
    fn maximized_y_fullscreen_parsean() {
        let opts = launch_opts(vec!["baud", "--maximized"]);
        assert!(opts.maximized);
        assert!(!opts.fullscreen);
        let opts = launch_opts(vec!["baud", "--fullscreen"]);
        assert!(opts.fullscreen);
        assert!(!opts.maximized);
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
    fn completions_cubren_todos_los_flags() {
        let flags = [
            "--working-directory",
            "--title",
            "--app-id",
            "--hold",
            "--config",
            "--window-size",
            "--maximized",
            "--fullscreen",
            "--server",
            "--new-instance",
            "-o",
            "-e",
        ];
        let root = env!("CARGO_MANIFEST_DIR");
        for f in [
            "packaging/completions/baud.bash",
            "packaging/completions/baud.zsh",
            "packaging/completions/baud.fish",
            "packaging/man/baud.1",
        ] {
            let path = format!("{root}/{f}");
            let cuerpo = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("no se pudo leer {path}: {e}"));
            let haystack = if f.ends_with("baud.1") {
                cuerpo.replace('\\', "")
            } else {
                cuerpo
            };
            for flag in flags {
                assert!(haystack.contains(flag), "{f} no menciona {flag}");
            }
        }
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
        outcome_from(parse(
            args.into_iter().map(OsString::from).collect::<Vec<_>>(),
        ))
        .expect("outcome")
    }
}
