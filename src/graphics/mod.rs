//! Protocolo de gráficos por APC y almacén de imágenes/placements.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

pub mod protocol;
pub mod store;

pub use protocol::{parse, Action, ChunkAssembler, GraphicsCommand, GraphicsError, Keys};
pub use store::{DecodedImage, ExecContext, GraphicsStore, Viewport, VisiblePlacement};

/// Identificador de imagen (0 = sin id / asignar al transmitir).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ImageId(pub u32);

/// Respuesta que se escribe de vuelta al PTY.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphicsResponse {
    pub image_id: ImageId,
    pub image_number: u32,
    pub placement_id: u32,
    pub error: Option<String>,
}

/// Placement anclado a una fila lógica del grid.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub image_id: ImageId,
    pub placement_id: u32,
    pub logical_row: usize,
    pub col: usize,
    pub rows: u32,
    pub cols: u32,
    pub src_x: u32,
    pub src_y: u32,
    pub src_w: u32,
    pub src_h: u32,
    pub z: i32,
}
