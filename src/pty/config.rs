/// Tipo de sesión soportada por el backend de PTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn test_process_config_default_usa_shell_env() {
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
        unsafe {
            std::env::remove_var("SHELL");
        }
        let cfg = ProcessConfig::default();
        assert_eq!(cfg.shell, "/bin/bash");
        assert!(cfg.args.is_empty());
        assert!(cfg.working_directory.is_none());
        assert!(!cfg.login_shell);
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
