```yaml
titulo: "ADR-0005: Patron de Event Loop e Integracion I/O"
tipo: decision
autor: "Carlos Canabal Cordero"
fecha_creacion: "2026-06-14"
fecha_modificacion: "2026-06-14"
version: "0.1.0"
estado: aceptado
tags: [decision, event-loop, pty, hilos, polling, async]
```

# ADR-0005: Patron de Event Loop e Integracion I/O

## Contexto

El proyecto necesita definir como se integra el PTY I/O
(bloqueante por naturaleza) con el event loop de la GUI
(no bloqueante, manejado por winit). Hay tres opciones
principales:

1. **winit puro + hilo PTY separado.** Patron de Alacritty.
   El hilo GUI corre el event loop de winit; el hilo PTY
   es un loop sincronico que lee bytes.
2. **winit + runtime async (tokio/smol).** Patron de
   WezTerm (parcialmente). tokio maneja tanto GUI events
   como PTY I/O en el mismo runtime.
3. **Hilo único con polling::Poller.** Variante minimalista
   donde todo se ejecuta en un solo hilo.

La pregunta es cual patron minimiza complejidad sin
sacrificar performance.

## Decision

Se adopta el patron de **dos hilos por ventana**:

- **Hilo GUI:** ejecuta el event loop de winit, captura
  WindowEvent (teclado, mouse, resize, close), mantiene
  el contexto wgpu, y lee snapshots del estado del
  terminal bajo lock.
- **Hilo PTY:** loop sincronico simple que lee bytes del
  PTY master, los alimenta al parser vte, y muta el
  estado del terminal bajo lock.

La sincronizacion usa `Arc<parking_lot::FairMutex<Term<T>>>`.
La comunicacion unidireccional usa:

- GUI -> PTY: `mpsc::Sender<Msg>` con variantes para
  bytes, resize, shutdown.
- PTY -> GUI: `winit::event_loop::EventLoopProxy` para
  solicitar wakeup.

La integracion del file descriptor del PTY con el event
loop de winit usa `polling::Poller` (la misma libreria
que usa Alacritty, versión 2.x).

**No se usa tokio ni ningun runtime async.** El hilo PTY
es sincronico, lo que elimina la complejidad de
async/await en la lógica del terminal.

## Justificacion

1. **Alacritty lo valida en produccion desde 2017.**
   El patron de 2 hilos + polling::Poller + mpsc +
   FairMutex es el mismo que usa Alacritty con ~40k
   lineas de código. El proyecto hereda esta decision
   probada.

2. **WezTerm confirma que tokio es opcional.** WezTerm
   usa tokio para multiplexar sesiones (Mux), pero
   para una sola ventana, el patron de 2 hilos sin
   async es suficiente. El proyecto no requiere
   multiplexacion en MVP.

3. **Elimina complejidad del modelo async.** Sin
   async/await, no hay que pensar en lifetimes de
   futures, Pin, ni box::pin. El código del parser
   y del grid permanece sincronico y facil de
   testear con unit tests.

4. **El PTY I/O es naturalmente bloqueante.** Llamar
   `read()` en el master es bloqueante por POSIX.
   Forzarlo en un runtime async requiere spawn_blocking
   o equivalente, que no aporta beneficio.

5. **winit esta disenado para event loop en hilo
   principal.** winit no expone su event loop como
   futurable; requiere un hilo dedicado. Adaptar winit
   a tokio agregaria una capa de adaptacion que no
   aporta valor.

6. **Polling::Poller integra I/O externo limpiamente.**
   Se registra el fd del PTY con Poller; cuando hay
   datos, se llama `EventLoopProxy::send_event()` para
   que el hilo GUI procese el redraw. El código es
   ~50 lineas y se entiende en una lectura.

## Alternativas Consideradas

| Alternativa                       | Pros                                             | Contras                                                                         | Veredicto                                      |
| :-------------------------------- | :----------------------------------------------- | :------------------------------------------------------------------------------ | :--------------------------------------------- |
| tokio en hilo GUI                 | Stack unificado, muchos crates async             | winit no es async-native, requiere adaptacion; el modelo mental es mas complejo | Rechazada                                      |
| tokio en hilo PTY solamente       | Aprovecha async solo donde aporta                | La interfaz sync-async agrega complejidad; no hay beneficio claro               | Rechazada                                      |
| smol (runtime async ligero)       | Mas pequeno que tokio                            | Mismo problema fundamental que tokio                                            | Rechazada                                      |
| Hilo único con polling::Poller    | Sin locks                                        | Limita la capacidad de paralelizar; CPU-bound tasks bloquean I/O                | Considerado, descartado por paralelismo futuro |
| 3+ hilos (GUI + PTY + Mux)        | Multiplexacion nativa                            | WezTerm lo necesita, el MVP no                                                  | Descartado para MVP                            |
| **2 hilos (GUI + PTY) sin async** | Validado en Alacritty, simple, sin runtime async | Requiere Poller para integrar I/O externo                                       | **Seleccionada**                               |

## Consecuencias

### Positivas

- Modelo mental simple: el hilo PTY es un loop
  sincronico; el hilo GUI es el event loop de winit.
  Sin futures, sin async/await.
- Performance: el patron es lo suficientemente rápido
  para el MVP (60fps con 200x50 es alcanzable).
- Testabilidad: el hilo PTY se puede testear en
  isolation con un mock que escribe bytes al canal.
- Dependencias: solo `polling` y `parking_lot` son
  necesarias para sincronizacion. No tokio.

### Negativas

- **El lock se comparte entre dos hilos.** Hay que
  ser disciplinado para no mantener el lock durante
  operaciones lentas (I/O, allocation).
- **El bus de mensajes (mpsc) tiene latencia.** En
  teoria, crossbeam::channel es mas rápido; el
  proyecto lo evalua en Fase 5.
- **No soporta multi-tenant.** Cada ventana tiene su
  propio par de hilos. Para multiplexacion (WezTerm
  Mux), se necesitara un rediseño.

### Mitigacion

- Las operaciones bajo lock se limitan a mutaciones
  del estado. El render (draw call a wgpu) se hace
  fuera del lock, leyendo una snapshot.
- Se mide la latencia de mpsc en Fase 5 con un
  benchmark; si es problema, se migra a
  crossbeam::channel.
- El patron permite extender a multi-tenant en Fase
  5+ lanzando multiples pares de hilos, sin cambiar
  la arquitectura.

## Referencias

- docs/prompts/iter-06-investigacion-B.md
  (investigacion completa, 467 lineas, 10 URLs
  verificadas HTTP 200).
- docs/research/03-input.md (input handling).
- docs/research/05-terminal-grid.md (estado del
  terminal).
- Alacritty event loop: alacritty/src/event.rs y
  alacritty_terminal/src/event_loop.rs.
- WezTerm main: wezterm-gui/src/main.rs.
- https://crates.io/crates/polling
- https://crates.io/crates/parking_lot

## Cambios

| Version | Fecha      | Cambios                                                |
| :------ | :--------- | :----------------------------------------------------- |
| 0.1.0   | 2026-06-14 | Primer borrador. Decision adoptada. 2 hilos sin async. |
