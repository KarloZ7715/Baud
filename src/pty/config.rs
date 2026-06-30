/// Configuración del proceso hijo que se lanza en el PTY.
#[derive(Debug, Clone)]
pub struct ProcessConfig {
    pub shell: String,
    pub args: Vec<String>,
    pub working_directory: Option<String>,
    /// Variables extra (se añaden a las heredadas). TERM lo fija spawn.
    pub env: Vec<(String, String)>,
    /// Comando a escribir al PTY tras arrancar (con newline). None = nada.
    pub startup_command: Option<String>,
    /// Si true, arranca como login shell (argv[0] con '-' inicial).
    pub login_shell: bool,
}

impl Default for ProcessConfig {
    fn default() -> Self {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        Self {
            shell,
            args: Vec::new(),
            working_directory: None,
            env: Vec::new(),
            startup_command: None,
            login_shell: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_config_default_resuelve_shell() {
        // sin $SHELL definido debe caer a /bin/bash; con $SHELL lo usa.
        unsafe {
            std::env::remove_var("SHELL");
        }
        let cfg = ProcessConfig::default();
        assert_eq!(cfg.shell, "/bin/bash");
        assert!(cfg.args.is_empty());
        assert!(cfg.working_directory.is_none());
        assert!(!cfg.login_shell);
    }
}
