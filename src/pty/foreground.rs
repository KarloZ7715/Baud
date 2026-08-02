//! Proceso en primer plano de una sesion PTY, para el titulo de la tab.
//!
//! En Unix se lee el grupo de procesos en primer plano de la terminal
//! (`tcgetpgrp`) y se resuelve su nombre via `/proc/<pgid>/comm`. El fd usado
//! es un duplicado del master: el original lo posee el hilo del PTY, y
//! duplicarlo evita que el sondeo desde el hilo de la GUI dispute ese fd.
//!
//! En Windows no hay una señal equivalente sin recorrer el arbol de procesos
//! de ConPTY (`CreateToolhelp32Snapshot`); ver plan 011, riesgo "Windows sin
//! deteccion de proceso fiable". De momento se degrada a la cadena de
//! resolucion del titulo por cwd/OSC, que ya cubre los demas pasos.

#[cfg(unix)]
pub type Probe = std::os::fd::OwnedFd;

#[cfg(windows)]
pub type Probe = ();

/// Duplica el fd necesario para sondear el proceso en primer plano desde el
/// hilo de la GUI.
#[cfg(unix)]
pub fn make_probe(master: &super::Pty) -> std::io::Result<Probe> {
    nix::unistd::dup(master.fd()).map_err(std::io::Error::from)
}

#[cfg(windows)]
pub fn make_probe(_master: &super::Pty) -> std::io::Result<Probe> {
    Ok(())
}

/// Sondea el proceso en primer plano y actualiza `cache` si el grupo de
/// procesos cambio. Devuelve `true` si el nombre visible cambio (para
/// decidir si hace falta un redraw).
#[cfg(unix)]
pub fn poll(probe: &Probe, cache: &mut Option<(i32, String)>) -> bool {
    let Ok(pgid) = nix::unistd::tcgetpgrp(probe) else {
        return false;
    };
    let pgid = pgid.as_raw();
    if let Some((cached_pgid, _)) = cache {
        if *cached_pgid == pgid {
            return false;
        }
    }
    let Ok(comm) = std::fs::read_to_string(format!("/proc/{pgid}/comm")) else {
        return false;
    };
    let name = comm.trim();
    if name.is_empty() {
        return false;
    }
    let changed = cache.as_ref().map(|(_, n)| n.as_str()) != Some(name);
    *cache = Some((pgid, name.to_string()));
    changed
}

#[cfg(windows)]
pub fn poll(_probe: &Probe, _cache: &mut Option<(i32, String)>) -> bool {
    false
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn poll_resuelve_el_nombre_del_proceso_en_primer_plano() {
        let master = crate::pty::spawn("sleep", &["2"]).expect("spawn");
        let probe = make_probe(&master).expect("make_probe");
        let mut cache = None;

        let mut resolved = false;
        for _ in 0..50 {
            if poll(&probe, &mut cache) {
                resolved = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        assert!(resolved, "poll nunca reporto un cambio de nombre");
        assert_eq!(cache.as_ref().map(|(_, n)| n.as_str()), Some("sleep"));
    }

    #[test]
    fn poll_no_relee_proc_cuando_el_pgid_no_cambia() {
        let master = crate::pty::spawn("sleep", &["2"]).expect("spawn");
        let probe = make_probe(&master).expect("make_probe");
        let mut cache = None;

        while !poll(&probe, &mut cache) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let cached_after_first = cache.clone();

        assert!(
            !poll(&probe, &mut cache),
            "el pgid no cambio, no debe reportar cambio"
        );
        assert_eq!(cache, cached_after_first);
    }
}
