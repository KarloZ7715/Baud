use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

struct App {
    window: Option<Window>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // ponytail: solo ventana basica, el render real llega en Fase 2 (Sprint 3)
        let attrs = WindowAttributes::default()
            .with_title("baud")
            .with_inner_size(winit::dpi::LogicalSize::new(800.0, 600.0));
        self.window = Some(
            event_loop
                .create_window(attrs)
                .expect("no se pudo crear la ventana"),
        );
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let WindowEvent::CloseRequested = event {
            event_loop.exit();
        }
    }
}

/// Abre la ventana principal de baud.
///
/// Bloquea el hilo de llamada hasta que se cierra la ventana.
/// Retorna Ok(()) al cerrarse limpio.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    let mut app = App { window: None };
    event_loop.run_app(&mut app)?;
    Ok(())
}
