use std::path::{Path, PathBuf};

/// Tipo de sesión soportada por el backend de PTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionKind {
    /// Sesión nativa de la plataforma (shell por defecto).
    #[default]
    Native,
    /// Sesión WSL bajo ConPTY (solo Windows).
    Wsl,
}

/// Configuración del proceso hijo que se lanza en el PTY.
#[derive(Debug, Clone)]
pub struct ProcessConfig {
    pub shell: String,
    pub args: Vec<String>,
    pub working_directory: Option<String>,
    /// Variables extra (se añaden a las heredadas). Spawn fija
    /// `TERM=xterm-256color` y `COLORTERM=truecolor` despues de este env.
    pub env: Vec<(String, String)>,
    /// Comando a escribir al PTY tras arrancar (con newline). None = nada.
    pub startup_command: Option<String>,
    /// Si true, arranca como login shell (argv[0] con '-' inicial).
    pub login_shell: bool,
    /// Perfil de sesión. En Windows `Wsl` activa `wsl.exe` bajo ConPTY.
    pub kind: SessionKind,
    /// Distro WSL objetivo (opcional). Se traduce en `-d <distro>`.
    pub distro: Option<String>,
    /// Directorio inicial para WSL vía `--cd` (opcional).
    pub wsl_cwd: Option<String>,
    /// Inyección de marcas OSC 133 al lanzar el shell.
    pub shell_integration: ShellIntegration,
}

/// Modo de inyección de shell integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ShellIntegration {
    /// Detecta zsh/bash/pwsh e inyecta lo que corresponda.
    #[default]
    Auto,
    /// No toca env ni args del hijo.
    Off,
}

/// Env y args extra que hay que añadir al spawn.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InjectionPlan {
    pub env: Vec<(String, String)>,
    pub extra_args: Vec<String>,
}

impl InjectionPlan {
    pub fn is_empty(&self) -> bool {
        self.env.is_empty() && self.extra_args.is_empty()
    }
}

const ZSH_SCRIPT: &str = include_str!("../../assets/shell/baud.zsh");
const BASH_SCRIPT: &str = include_str!("../../assets/shell/baud.bash");
const PWSH_SCRIPT: &str = include_str!("../../assets/shell/baud.ps1");

const ZSH_WRAPPER: &str = "\
# Baud ZDOTDIR wrapper: user rc first, then marks.
if [[ -f \"${BAUD_ORIG_ZDOTDIR:-$HOME}/.zshrc\" ]]; then
  source \"${BAUD_ORIG_ZDOTDIR:-$HOME}/.zshrc\"
fi
source \"${ZDOTDIR}/baud.zsh\"
";

const BASH_WRAPPER: &str = "\
# Baud --rcfile wrapper: user rc first, then marks.
__baud_dir=$(dirname -- \"${BASH_SOURCE[0]}\")
if [[ -f \"${HOME}/.bashrc\" ]]; then
  source \"${HOME}/.bashrc\"
fi
source \"${__baud_dir}/baud.bash\"
";

/// Directorio de scripts: el mismo patrón que los logs (`state_dir` / local).
pub fn integration_scripts_dir() -> PathBuf {
    dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("baud")
        .join("shell-integration")
}

fn shell_basename(shell: &str) -> String {
    Path::new(shell)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(shell)
        .trim_end_matches(".exe")
        .to_ascii_lowercase()
}

/// Decide env/args de inyección sin tocar disco.
pub fn integration_plan(
    shell: &str,
    args: &[String],
    login_shell: bool,
    mode: ShellIntegration,
    orig_zdotdir: Option<&str>,
    scripts_dir: &Path,
) -> InjectionPlan {
    if mode == ShellIntegration::Off {
        return InjectionPlan::default();
    }
    match shell_basename(shell).as_str() {
        "zsh" => {
            let zdot = scripts_dir.join("zsh");
            let mut env = vec![("ZDOTDIR".into(), zdot.to_string_lossy().into_owned())];
            if let Some(orig) = orig_zdotdir {
                env.push(("BAUD_ORIG_ZDOTDIR".into(), orig.to_string()));
            }
            InjectionPlan {
                env,
                extra_args: Vec::new(),
            }
        }
        "bash" if args.is_empty() && !login_shell => {
            let rcfile = scripts_dir.join("bash").join("rcfile");
            InjectionPlan {
                env: Vec::new(),
                extra_args: vec!["--rcfile".into(), rcfile.to_string_lossy().into_owned()],
            }
        }
        "pwsh" | "powershell" => InjectionPlan {
            env: vec![
                ("BAUD_SHELL_INTEGRATION".into(), "1".into()),
                (
                    "BAUD_SHELL_INTEGRATION_SCRIPT".into(),
                    scripts_dir
                        .join("pwsh")
                        .join("baud.ps1")
                        .to_string_lossy()
                        .into_owned(),
                ),
            ],
            extra_args: Vec::new(),
        },
        _ => InjectionPlan::default(),
    }
}

pub fn write_integration_scripts(scripts_dir: &Path) -> std::io::Result<()> {
    let zsh_dir = scripts_dir.join("zsh");
    let bash_dir = scripts_dir.join("bash");
    let pwsh_dir = scripts_dir.join("pwsh");
    std::fs::create_dir_all(&zsh_dir)?;
    std::fs::create_dir_all(&bash_dir)?;
    std::fs::create_dir_all(&pwsh_dir)?;
    std::fs::write(zsh_dir.join("baud.zsh"), ZSH_SCRIPT)?;
    std::fs::write(zsh_dir.join(".zshrc"), ZSH_WRAPPER)?;
    std::fs::write(bash_dir.join("baud.bash"), BASH_SCRIPT)?;
    std::fs::write(bash_dir.join("rcfile"), BASH_WRAPPER)?;
    std::fs::write(pwsh_dir.join("baud.ps1"), PWSH_SCRIPT)?;
    Ok(())
}

impl ProcessConfig {
    /// Añade env/args de integración. Si no se pueden escribir los scripts,
    /// el spawn sigue sin inyección.
    pub fn apply_shell_integration(&mut self) {
        if self.shell_integration == ShellIntegration::Off {
            return;
        }
        let scripts_dir = integration_scripts_dir();
        let orig = std::env::var("ZDOTDIR").ok();
        let plan = integration_plan(
            &self.shell,
            &self.args,
            self.login_shell,
            self.shell_integration,
            orig.as_deref(),
            &scripts_dir,
        );
        if plan.is_empty() {
            return;
        }
        if let Err(e) = write_integration_scripts(&scripts_dir) {
            tracing::debug!("shell integration: no se escribieron scripts: {e}");
            return;
        }
        self.env.extend(plan.env);
        if !plan.extra_args.is_empty() {
            let mut args = plan.extra_args;
            args.append(&mut self.args);
            self.args = args;
        }
    }
}

impl Default for ProcessConfig {
    fn default() -> Self {
        #[cfg(unix)]
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        #[cfg(windows)]
        let shell = std::env::var("COMSPEC").unwrap_or_else(|_| "powershell.exe".into());
        #[cfg(not(any(unix, windows)))]
        let shell = "sh".into();

        Self {
            shell,
            args: Vec::new(),
            working_directory: None,
            env: Vec::new(),
            startup_command: None,
            login_shell: false,
            kind: SessionKind::Native,
            distro: None,
            wsl_cwd: None,
            shell_integration: ShellIntegration::Auto,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // SHELL es global al proceso: sin este lock, correr estos dos tests en
    // paralelo hace que uno observe el valor que el otro acaba de fijar.
    #[cfg(unix)]
    static SHELL_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[cfg(unix)]
    #[test]
    fn test_process_config_default_usa_shell_env() {
        let _guard = SHELL_ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("SHELL", "/usr/bin/zsh");
        }
        let cfg = ProcessConfig::default();
        assert_eq!(cfg.shell, "/usr/bin/zsh");
        unsafe {
            std::env::remove_var("SHELL");
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_process_config_default_resuelve_shell() {
        let _guard = SHELL_ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("SHELL");
        }
        let cfg = ProcessConfig::default();
        assert_eq!(cfg.shell, "/bin/bash");
        assert!(cfg.args.is_empty());
        assert!(cfg.working_directory.is_none());
        assert!(!cfg.login_shell);
    }

    #[test]
    fn zsh_auto_recibe_zdotdir() {
        let dir = Path::new("/tmp/baud-scripts");
        let plan = integration_plan(
            "/usr/bin/zsh",
            &[],
            false,
            ShellIntegration::Auto,
            Some("/old/zdot"),
            dir,
        );
        assert!(plan
            .env
            .iter()
            .any(|(k, v)| k == "ZDOTDIR" && v == "/tmp/baud-scripts/zsh"));
        assert!(plan
            .env
            .iter()
            .any(|(k, v)| k == "BAUD_ORIG_ZDOTDIR" && v == "/old/zdot"));
        assert!(plan.extra_args.is_empty());
    }

    #[test]
    fn bash_con_args_de_usuario_no_se_toca() {
        let plan = integration_plan(
            "bash",
            &["-c".into(), "true".into()],
            false,
            ShellIntegration::Auto,
            None,
            Path::new("/tmp/baud-scripts"),
        );
        assert!(plan.is_empty());
    }

    #[test]
    fn bash_login_no_se_toca() {
        let plan = integration_plan(
            "bash",
            &[],
            true,
            ShellIntegration::Auto,
            None,
            Path::new("/tmp/baud-scripts"),
        );
        assert!(plan.is_empty());
    }

    #[test]
    fn bash_auto_sin_args_recibe_rcfile() {
        let plan = integration_plan(
            "/bin/bash",
            &[],
            false,
            ShellIntegration::Auto,
            None,
            Path::new("/tmp/baud-scripts"),
        );
        assert_eq!(
            plan.extra_args,
            vec![
                "--rcfile".to_string(),
                "/tmp/baud-scripts/bash/rcfile".to_string()
            ]
        );
    }

    #[test]
    fn pwsh_auto_recibe_flag() {
        let plan = integration_plan(
            "pwsh.exe",
            &[],
            false,
            ShellIntegration::Auto,
            None,
            Path::new("/tmp/baud-scripts"),
        );
        assert!(plan
            .env
            .iter()
            .any(|(k, v)| k == "BAUD_SHELL_INTEGRATION" && v == "1"));
        assert!(plan
            .env
            .iter()
            .any(|(k, _)| k == "BAUD_SHELL_INTEGRATION_SCRIPT"));
    }

    #[test]
    fn off_nunca_inyecta() {
        let dir = Path::new("/tmp/baud-scripts");
        for shell in ["zsh", "bash", "pwsh", "fish"] {
            let plan = integration_plan(shell, &[], false, ShellIntegration::Off, None, dir);
            assert!(plan.is_empty(), "{shell}");
        }
    }

    #[test]
    fn shell_desconocido_no_se_toca() {
        let plan = integration_plan(
            "fish",
            &[],
            false,
            ShellIntegration::Auto,
            None,
            Path::new("/tmp/baud-scripts"),
        );
        assert!(plan.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn test_process_config_default_windows_shell() {
        let cfg = ProcessConfig::default();
        let lower = cfg.shell.to_lowercase();
        assert!(
            lower.contains("powershell") || lower.contains("pwsh") || lower.contains("cmd"),
            "shell inesperado: {}",
            cfg.shell
        );
    }
}
