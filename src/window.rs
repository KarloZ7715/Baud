//! Ventana principal de Baud.
//!
//! App implementa ApplicationHandler<UserEvent> de winit 0.30.
//! El Renderer se inicializa en resumed() y se invoca en redraw_requested().
//! El Term se comparte con el hilo drain via Arc<Mutex<Term>>.

use std::future::Future;
use std::pin::Pin;
use std::sync::{mpsc, Arc, Mutex};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use crate::ansi::Term;
use crate::event_loop::PtyCommand;
use crate::grid::{COLS, ROWS};
use crate::renderer::Renderer;
use winit::application::ApplicationHandler;
use winit::event::ElementState;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

/// Eventos enviados desde el hilo drain al hilo GUI.
#[derive(Debug)]
pub enum UserEvent {
    /// El drain termino de procesar bytes del PTY; la GUI debe redibujar.
    RedrawNeeded,
    /// El child termino (EOF en master fd).
    PtyExited(i32),
    /// Error de I/O del PTY.
    PtyError(String),
}

/// Estado de la aplicación GUI.
pub struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    term: Arc<Mutex<Term>>,
    pty_tx: Arc<Mutex<Option<mpsc::Sender<PtyCommand>>>>,
    /// Estado de teclas modificadoras (Ctrl, Shift, Alt, etc.).
    modifiers: winit::event::Modifiers,
}

impl App {
    /// Crea una nueva instancia de App con el term compartido.
    pub fn new(
        term: Arc<Mutex<Term>>,
        pty_tx: Arc<Mutex<Option<mpsc::Sender<PtyCommand>>>>,
    ) -> Self {
        Self {
            window: None,
            renderer: None,
            term,
            pty_tx,
            modifiers: winit::event::Modifiers::default(),
        }
    }

    /// Copia todo el grid activo (24x80) al clipboard del sistema.
    /// Usa wl-copy (Wayland nativo) porque arboard requiere XWayland.
    fn handle_copy(&mut self) {
        // 1. Serializar el grid activo.
        let serialized = {
            let term_guard = match self.term.lock() {
                Ok(g) => g,
                Err(poisoned) => {
                    tracing::warn!("term mutex poisoned: {poisoned}");
                    return;
                }
            };
            let grid = term_guard.active_grid();
            let mut s = String::new();
            for row in &grid.rows {
                for cell in row {
                    s.push(cell.ch);
                }
                s.push('\n');
            }
            s.pop();
            s
        };

        // 2. Copiar via wl-copy (Wayland nativo).
        // ponytail: wl-copy debe estar instalado (parte de wl-clipboard).
        let ok = std::process::Command::new("wl-copy")
            .arg("--trim-newline")
            .arg(&serialized)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        // 3. Mostrar feedback visual.
        if ok {
            if let Some(renderer) = &mut self.renderer {
                renderer.set_status("[Copiado al clipboard]");
            }
        } else {
            if let Some(renderer) = &mut self.renderer {
                renderer.set_status("[Clipboard no disponible]");
            }
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    /// Obtiene texto del clipboard del sistema, lo filtra y lo envia al PTY.
    /// Usa wl-paste (Wayland nativo).
    /// Si bracketed paste mode (DEC 2004) esta activo, envuelve el texto en
    /// \x1b[200~...\x1b[201~ para que readline no ejecute comandos al pegar.
    fn handle_paste(&mut self) {
        tracing::info!("handle_paste: iniciando");
        // Obtener texto via wl-paste (Wayland nativo).
        // ponytail: wl-paste debe estar instalado (parte de wl-clipboard).
        let output = match std::process::Command::new("wl-paste").output() {
            Ok(o) if o.status.success() => o,
            _ => {
                tracing::warn!("wl-paste fallo o no disponible");
                return;
            }
        };
        let text = String::from_utf8_lossy(&output.stdout).to_string();
        // Eliminar newline final que wl-paste suele incluir.
        // ponytail: trim_end_matches('\n') es mas compatible que --trim-newline.
        let text = text.trim_end_matches('\n').to_string();

        // Verificar si bracketed paste mode esta activo para evitar que
        // el texto pegado se ejecute como comandos al contener newlines.
        let bracketed = self
            .term
            .lock()
            .ok()
            .map(|t| t.bracketed_paste)
            .unwrap_or(false);

        // Filtrar y (si aplica) envolver en marcadores DEC 2004.
        let filtered = if bracketed {
            tracing::info!("handle_paste: bracketed paste activo, envolviendo texto");
            crate::input::paste_with_bracketing(&text, true)
        } else {
            crate::input::paste_text(&text)
        };
        tracing::info!("handle_paste: {} bytes filtrados", filtered.len());
        if let Some(tx) = self.pty_tx.lock().expect("pty_tx mutex poisoned").as_ref() {
            let _ = tx.send(PtyCommand::Input(filtered));
        }
    }

    /// Envia bytes de input al hilo PTY para escribirlos en el master fd.
    fn send_input(&self, bytes: Vec<u8>) {
        tracing::info!("send_input: {} bytes: {:02x?}", bytes.len(), bytes);
        if let Some(tx) = self.pty_tx.lock().expect("pty_tx mutex poisoned").as_ref() {
            let _ = tx.send(PtyCommand::Input(bytes));
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // ponytail: solo inicializar una vez.
        if self.window.is_some() {
            return;
        }

        // 1. Crear ventana.
        let attrs = Window::default_attributes()
            .with_title("baud")
            .with_inner_size(winit::dpi::LogicalSize::new(800.0, 600.0));
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("no se pudo crear la ventana"),
        );
        self.window = Some(window.clone());

        // 2. Obtener display handle para wgpu (evita el lifetime de ActiveEventLoop).
        let display_handle = event_loop.owned_display_handle();

        // 3. Inicializar wgpu: instance, adapter, device, queue, surface, config.
        //    wgpu 29 tiene API async (request_adapter, request_device retornan Future).
        //    Usamos block_on() local (sin pollster) para bloquear en nativo.
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(display_handle)),
        );

        let surface = instance
            .create_surface(window.clone())
            .expect("no se pudo crear la surface wgpu");

        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("no se encontro adaptador GPU compatible");

        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            required_limits:
                wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
        }))
        .expect("no se pudo crear el device GPU");

        let size = window.inner_size();
        let config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .expect("no se encontro formato de surface compatible");
        surface.configure(&device, &config);

        // 4. Crear Renderer (Ronda 1).
        self.renderer = Some(Renderer::new(
            window.clone(),
            device,
            queue,
            surface,
            config,
        ));
        tracing::info!("renderer inicializado");

        // 5. Forzar el primer redraw para que winit dispare RedrawRequested.
        // Sin esto, la ventana queda vacia hasta que el drain envie bytes
        // (lo cual activa el user_event RedrawNeeded -> request_redraw).
        // Con esto, pintamos el estado inicial del term inmediatamente,
        // evitando que el compositor (Hyprland) marque la ventana como
        // "no responde" mientras espera output.
        window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                // Enviar Shutdown al hilo PTY para que envie SIGHUP al child.
                if let Some(tx) = self.pty_tx.lock().expect("pty_tx mutex poisoned").as_ref() {
                    let _ = tx.send(PtyCommand::Shutdown);
                }
                // Salir del event loop. El hilo PTY recibira el Shutdown, hara SIGHUP,
                // esperara 100ms, y morira. El Pty se dropea con SIGKILL safety net.
                event_loop.exit();
            }
            WindowEvent::Resized(new_size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(new_size.width, new_size.height);
                }
                // Enviar resize al hilo PTY para que haga ioctl(TIOCSWINSZ).
                // ponytail: el grid logico sigue siendo 24x80 hardcoded; el child
                // recibe SIGWINCH para ajustar su output. Reflow en Sprint 6.
                if let Some(tx) = self.pty_tx.lock().expect("pty_tx mutex poisoned").as_ref() {
                    let _ = tx.send(PtyCommand::Resize {
                        rows: ROWS as u16,
                        cols: COLS as u16,
                    });
                }
            }
            WindowEvent::RedrawRequested => {
                let Some(renderer) = &mut self.renderer else {
                    return;
                };
                tracing::info!("RedrawRequested: renderizando frame");
                // Lockear el term directamente; el campo es solo lectura aqui.
                let term_guard = match self.term.lock() {
                    Ok(g) => g,
                    Err(poisoned) => {
                        tracing::warn!("term mutex poisoned: {poisoned}");
                        return;
                    }
                };
                if let Err(e) = renderer.render(&term_guard) {
                    tracing::warn!("error al renderizar: {e}");
                }
            }
            // Track modifier state (Ctrl, Shift, Alt, etc.) for keyboard shortcuts.
            // winit 0.30 envia ModifiersChanged separado de KeyboardInput.
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers;
            }
            // Input de teclado completo: letras, Enter, Backspace, Tab, Ctrl+letter, etc.
            // ponytail: input basico sin manejo de flechas ni F-keys; Sprint 6 las agrega.
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let ctrl = self.modifiers.state().control_key();
                let shift = self.modifiers.state().shift_key();

                // 1. Ctrl+Shift+C/V (copy/paste, ya implementado en Sprint 5b).
                if ctrl && shift {
                    match &event.logical_key {
                        Key::Character(c) if c.eq_ignore_ascii_case("c") => {
                            self.handle_copy();
                            return;
                        }
                        Key::Character(c) if c.eq_ignore_ascii_case("v") => {
                            self.handle_paste();
                            return;
                        }
                        _ => {}
                    }
                }

                // 2. Ctrl+letter: enviar byte de control (Ctrl+A=0x01 .. Ctrl+Z=0x1A).
                if ctrl {
                    if let Key::Character(c) = &event.logical_key {
                        if let Some(&first_byte) = c.as_bytes().first() {
                            self.send_input(vec![first_byte & 0x1F]);
                            return;
                        }
                    }
                }

                // 3. Teclas con texto generado (letras, numeros, simbolos con Shift).
                //    Fallback a logical_key si text es None (bug conocido en winit/Wayland).
                if let Some(text) = event.text {
                    self.send_input(text.as_bytes().to_vec());
                    return;
                }
                if let Key::Character(c) = &event.logical_key {
                    if !c.is_empty() {
                        tracing::info!("keyboard: text=None, logical_key=Character({c}), fallback");
                        self.send_input(c.as_bytes().to_vec());
                        return;
                    }
                }

                // 4. Teclas especiales sin texto asociado.
                match &event.logical_key {
                    Key::Named(NamedKey::Enter) => self.send_input(b"\r".to_vec()),
                    Key::Named(NamedKey::Backspace) => self.send_input(b"\x7f".to_vec()),
                    Key::Named(NamedKey::Tab) => self.send_input(b"\t".to_vec()),
                    Key::Named(NamedKey::Escape) => self.send_input(b"\x1b".to_vec()),
                    Key::Named(NamedKey::ArrowUp) => self.send_input(b"\x1b[A".to_vec()),
                    Key::Named(NamedKey::ArrowDown) => self.send_input(b"\x1b[B".to_vec()),
                    Key::Named(NamedKey::ArrowLeft) => self.send_input(b"\x1b[D".to_vec()),
                    Key::Named(NamedKey::ArrowRight) => self.send_input(b"\x1b[C".to_vec()),
                    Key::Named(NamedKey::Home) => self.send_input(b"\x1b[H".to_vec()),
                    Key::Named(NamedKey::End) => self.send_input(b"\x1b[F".to_vec()),
                    Key::Named(NamedKey::PageUp) => self.send_input(b"\x1b[5~".to_vec()),
                    Key::Named(NamedKey::PageDown) => self.send_input(b"\x1b[6~".to_vec()),
                    Key::Named(NamedKey::Delete) => self.send_input(b"\x1b[3~".to_vec()),
                    Key::Named(NamedKey::Insert) => self.send_input(b"\x1b[2~".to_vec()),
                    Key::Named(NamedKey::F1) => self.send_input(b"\x1bOP".to_vec()),
                    Key::Named(NamedKey::F2) => self.send_input(b"\x1bOQ".to_vec()),
                    Key::Named(NamedKey::F3) => self.send_input(b"\x1bOR".to_vec()),
                    Key::Named(NamedKey::F4) => self.send_input(b"\x1bOS".to_vec()),
                    Key::Named(NamedKey::F5) => self.send_input(b"\x1b[15~".to_vec()),
                    Key::Named(NamedKey::F6) => self.send_input(b"\x1b[17~".to_vec()),
                    Key::Named(NamedKey::F7) => self.send_input(b"\x1b[18~".to_vec()),
                    Key::Named(NamedKey::F8) => self.send_input(b"\x1b[19~".to_vec()),
                    Key::Named(NamedKey::F9) => self.send_input(b"\x1b[20~".to_vec()),
                    Key::Named(NamedKey::F10) => self.send_input(b"\x1b[21~".to_vec()),
                    Key::Named(NamedKey::F11) => self.send_input(b"\x1b[23~".to_vec()),
                    Key::Named(NamedKey::F12) => self.send_input(b"\x1b[24~".to_vec()),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::RedrawNeeded => {
                // Solicitar un redraw para actualizar la pantalla.
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            UserEvent::PtyExited(code) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.set_status(&format!("[Proceso terminado: codigo {}]", code));
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            UserEvent::PtyError(msg) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.set_status(&format!("[Error PTY: {}]", msg));
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
        }
    }
}

/// Ejecuta un Future de forma sincrona bloqueando el hilo actual.
///
/// Implementacion minimalista usando solo std. En nativo, los futures de
/// wgpu (request_adapter, request_device) se resuelven en la primera poll,
/// asi que el overhead del spin-loop es despreciable.
// ponytail: si en algun momento wgpu requiere waker real, migrar a pollster.
fn block_on<F: Future>(mut future: F) -> F::Output {
    let mut future = unsafe { Pin::new_unchecked(&mut future) };
    let raw_waker = RawWaker::new(std::ptr::null(), &VTABLE);
    let waker = unsafe { Waker::from_raw(raw_waker) };
    let mut cx = Context::from_waker(&waker);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(val) => return val,
            Poll::Pending => {}
        }
    }
}

static VTABLE: RawWakerVTable = RawWakerVTable::new(
    |_: *const ()| RawWaker::new(std::ptr::null(), &VTABLE),
    |_: *const ()| {},
    |_: *const ()| {},
    |_: *const ()| {},
);
