//! Servidor IPC del control remoto (UDS en Unix, named pipe en Windows).

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]
