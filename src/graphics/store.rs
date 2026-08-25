//! Imágenes decodificadas, placements y cuotas.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use super::protocol::{Action, GraphicsCommand, GraphicsError, Keys};
use super::{GraphicsResponse, ImageId, Placement};

/// Tope de RAM de píxeles por sesión (bytes RGBA).
pub const MAX_PIXEL_BYTES: usize = 320 * 1024 * 1024;
/// Lado máximo de una imagen, en píxeles.
pub const MAX_DIM: u32 = 4096;
/// Tope de lectura de un archivo de transmisión.
const MAX_FILE_BYTES: u64 = 80 * 1024 * 1024;

/// Imagen RGBA8 lista para texturizar.
#[derive(Debug, Clone)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub generation: u64,
}

impl DecodedImage {
    pub fn byte_len(&self) -> usize {
        self.rgba.len()
    }
}

/// Recorte de viewport que el renderer usa para filtrar placements.
#[derive(Debug, Clone, Copy)]
pub struct Viewport {
    pub rows: usize,
    pub cols: usize,
    pub scrollback_len: usize,
    pub scrollback_offset: isize,
}

/// Placement ya proyectado a celdas visibles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VisiblePlacement {
    pub image_id: ImageId,
    pub src_rect_px: (u32, u32, u32, u32),
    pub dst_cells: (u16, u16, u16, u16),
    pub z: i32,
}

#[derive(Debug, Clone, Default)]
struct ScreenPlacements {
    items: Vec<Placement>,
}

/// Almacén de imágenes y placements (primario + alt).
#[derive(Debug, Clone)]
pub struct GraphicsStore {
    images: HashMap<ImageId, DecodedImage>,
    numbers: HashMap<u32, ImageId>,
    primary: ScreenPlacements,
    alt: ScreenPlacements,
    alt_screen: bool,
    next_id: u32,
    pixel_bytes: usize,
    generation: u64,
    last_trim: u64,
    evicted: Vec<ImageId>,
}

impl Default for GraphicsStore {
    fn default() -> Self {
        Self {
            images: HashMap::new(),
            numbers: HashMap::new(),
            primary: ScreenPlacements::default(),
            alt: ScreenPlacements::default(),
            alt_screen: false,
            next_id: 1,
            pixel_bytes: 0,
            generation: 1,
            last_trim: 0,
            evicted: Vec::new(),
        }
    }
}

impl GraphicsStore {
    pub fn is_empty(&self) -> bool {
        self.current().items.is_empty()
    }

    pub fn has_images(&self) -> bool {
        !self.images.is_empty()
    }

    pub fn image(&self, id: ImageId) -> Option<&DecodedImage> {
        self.images.get(&id)
    }

    pub fn last_placement_cells(&self) -> Option<(u32, u32)> {
        self.current().items.last().map(|p| (p.cols, p.rows))
    }

    pub fn take_evictions(&mut self) -> Vec<ImageId> {
        std::mem::take(&mut self.evicted)
    }

    pub fn enter_alt_screen(&mut self) {
        self.alt_screen = true;
        self.alt.items.clear();
    }

    pub fn exit_alt_screen(&mut self) {
        self.alt.items.clear();
        self.alt_screen = false;
        self.drop_unreferenced();
    }

    /// Reconciliar índices lógicos con el recorte de scrollback (misma regla
    /// que las marcas de prompt).
    pub fn reconcile_trim(&mut self, total_trim: u64) {
        if self.alt_screen {
            return;
        }
        let delta = total_trim.saturating_sub(self.last_trim) as usize;
        if delta == 0 {
            return;
        }
        self.primary.items.retain_mut(|p| {
            if p.logical_row < delta {
                false
            } else {
                p.logical_row -= delta;
                true
            }
        });
        self.last_trim = total_trim;
        self.drop_unreferenced();
    }

    pub fn visible_placements(&self, viewport: Viewport) -> Vec<VisiblePlacement> {
        let mut out = Vec::new();
        for p in &self.current().items {
            let Some(vis_row) = logical_to_visible(p.logical_row, viewport) else {
                continue;
            };
            if vis_row >= viewport.rows {
                continue;
            }
            if p.col >= viewport.cols {
                continue;
            }
            let rows = (p.rows as usize).min(viewport.rows.saturating_sub(vis_row));
            let cols = (p.cols as usize).min(viewport.cols.saturating_sub(p.col));
            if rows == 0 || cols == 0 {
                continue;
            }
            out.push(VisiblePlacement {
                image_id: p.image_id,
                src_rect_px: (p.src_x, p.src_y, p.src_w, p.src_h),
                dst_cells: (vis_row as u16, p.col as u16, rows as u16, cols as u16),
                z: p.z,
            });
        }
        out.sort_by(|a, b| a.z.cmp(&b.z).then_with(|| a.image_id.0.cmp(&b.image_id.0)));
        out
    }

    pub fn execute(&mut self, cmd: GraphicsCommand, ctx: &ExecContext) -> GraphicsResponse {
        if matches!(cmd.action, Action::Delete) {
            self.chunks_abort_hint();
            return self.delete(&cmd, ctx);
        }
        match cmd.action {
            Action::Query => self.query(&cmd),
            Action::Transmit => self.transmit(&cmd, false, ctx),
            Action::TransmitDisplay => self.transmit(&cmd, true, ctx),
            Action::Put => self.place(&cmd, ctx),
            Action::Delete => unreachable!(),
        }
    }

    fn chunks_abort_hint(&mut self) {
        // El ensamblador de chunks vive en Term; aquí no hay estado de
        // transmisión parcial. Los deletes abortan uploads en el llamador.
    }

    fn query(&mut self, cmd: &GraphicsCommand) -> GraphicsResponse {
        let id = cmd.keys.image_id.unwrap_or(0);
        match self.decode_payload(cmd) {
            Ok(_) => self.ok_resp(cmd, id),
            Err(e) => self.err_resp(cmd, id, e),
        }
    }

    fn transmit(
        &mut self,
        cmd: &GraphicsCommand,
        place: bool,
        ctx: &ExecContext,
    ) -> GraphicsResponse {
        let decoded = match self.decode_payload(cmd) {
            Ok(img) => img,
            Err(e) => return self.err_resp(cmd, cmd.keys.image_id.unwrap_or(0), e),
        };
        let id = match self.insert_image(cmd, decoded) {
            Ok(id) => id,
            Err(e) => return self.err_resp(cmd, cmd.keys.image_id.unwrap_or(0), e),
        };
        if place {
            if let Err(e) = self.add_placement(cmd, id, ctx) {
                return self.err_resp(cmd, id.0, e);
            }
        }
        self.ok_resp(cmd, id.0)
    }

    fn place(&mut self, cmd: &GraphicsCommand, ctx: &ExecContext) -> GraphicsResponse {
        let id = match self.resolve_id(cmd) {
            Some(id) => id,
            None => {
                return self.err_resp(cmd, cmd.keys.image_id.unwrap_or(0), StoreError::NotFound)
            }
        };
        if let Err(e) = self.add_placement(cmd, id, ctx) {
            return self.err_resp(cmd, id.0, e);
        }
        self.ok_resp(cmd, id.0)
    }

    fn delete(&mut self, cmd: &GraphicsCommand, _ctx: &ExecContext) -> GraphicsResponse {
        let d = cmd.keys.delete.unwrap_or(b'a');
        let free_data = d.is_ascii_uppercase();
        let kind = d.to_ascii_lowercase();
        match kind {
            b'a' => {
                self.current_mut().items.clear();
                if free_data {
                    self.drop_unreferenced();
                }
            }
            b'i' => {
                let Some(id) = self.resolve_id(cmd) else {
                    return self.ok_resp(cmd, cmd.keys.image_id.unwrap_or(0));
                };
                let pid = cmd.keys.placement_id.unwrap_or(0);
                self.current_mut().items.retain(|p| {
                    if p.image_id != id {
                        return true;
                    }
                    if pid == 0 {
                        false
                    } else {
                        p.placement_id != pid
                    }
                });
                if free_data && !self.referenced_ids().contains(&id) {
                    self.remove_image(id);
                }
            }
            _ => {}
        }
        self.ok_resp(cmd, cmd.keys.image_id.unwrap_or(0))
    }

    fn insert_image(
        &mut self,
        cmd: &GraphicsCommand,
        decoded: DecodedImage,
    ) -> Result<ImageId, StoreError> {
        let needed = decoded.byte_len();
        self.ensure_quota(needed)?;

        let requested = cmd.keys.image_id.unwrap_or(0);
        let id = if requested == 0 {
            self.allocate_id()
        } else {
            let id = ImageId(requested);
            if let Some(old) = self.images.remove(&id) {
                self.pixel_bytes = self.pixel_bytes.saturating_sub(old.byte_len());
                self.evicted.push(id);
            }
            self.primary.items.retain(|p| p.image_id != id);
            self.alt.items.retain(|p| p.image_id != id);
            id
        };

        if let Some(num) = cmd.keys.image_number {
            self.numbers.insert(num, id);
        }

        self.pixel_bytes = self.pixel_bytes.saturating_add(needed);
        self.generation = self.generation.saturating_add(1);
        let mut decoded = decoded;
        decoded.generation = self.generation;
        self.images.insert(id, decoded);
        Ok(id)
    }

    fn add_placement(
        &mut self,
        cmd: &GraphicsCommand,
        id: ImageId,
        ctx: &ExecContext,
    ) -> Result<(), StoreError> {
        let img = self.images.get(&id).ok_or(StoreError::NotFound)?;
        let (src_x, src_y, src_w, src_h) = src_rect(&cmd.keys, img.width, img.height);
        let (cols, rows) = dst_cells(&cmd.keys, src_w, src_h, ctx);
        let pid = cmd.keys.placement_id.unwrap_or(0);
        if pid != 0 {
            self.current_mut()
                .items
                .retain(|p| !(p.image_id == id && p.placement_id == pid));
        }
        self.current_mut().items.push(Placement {
            image_id: id,
            placement_id: pid,
            logical_row: ctx.logical_row,
            col: ctx.cursor_col,
            rows,
            cols,
            src_x,
            src_y,
            src_w,
            src_h,
            z: cmd.keys.z.unwrap_or(0),
        });
        Ok(())
    }

    fn resolve_id(&self, cmd: &GraphicsCommand) -> Option<ImageId> {
        if let Some(id) = cmd.keys.image_id {
            let id = ImageId(id);
            self.images.contains_key(&id).then_some(id)
        } else if let Some(num) = cmd.keys.image_number {
            self.numbers.get(&num).copied()
        } else {
            None
        }
    }

    fn allocate_id(&mut self) -> ImageId {
        loop {
            let id = ImageId(self.next_id);
            self.next_id = if self.next_id == u32::MAX {
                1
            } else {
                self.next_id + 1
            };
            if id.0 != 0 && !self.images.contains_key(&id) {
                return id;
            }
        }
    }

    fn ensure_quota(&mut self, needed: usize) -> Result<(), StoreError> {
        if needed > MAX_PIXEL_BYTES {
            return Err(StoreError::NoSpace);
        }
        if self.pixel_bytes.saturating_add(needed) <= MAX_PIXEL_BYTES {
            return Ok(());
        }
        self.evict_unplaced();
        if self.pixel_bytes.saturating_add(needed) <= MAX_PIXEL_BYTES {
            Ok(())
        } else {
            Err(StoreError::NoSpace)
        }
    }

    fn evict_unplaced(&mut self) {
        let referenced = self.referenced_ids();
        let orphans: Vec<ImageId> = self
            .images
            .keys()
            .copied()
            .filter(|id| !referenced.contains(id))
            .collect();
        for id in orphans {
            self.remove_image(id);
        }
    }

    fn drop_unreferenced(&mut self) {
        let referenced = self.referenced_ids();
        let orphans: Vec<ImageId> = self
            .images
            .keys()
            .copied()
            .filter(|id| !referenced.contains(id))
            .collect();
        for id in orphans {
            self.remove_image(id);
        }
    }

    fn referenced_ids(&self) -> std::collections::HashSet<ImageId> {
        self.primary
            .items
            .iter()
            .chain(self.alt.items.iter())
            .map(|p| p.image_id)
            .collect()
    }

    fn remove_image(&mut self, id: ImageId) {
        if let Some(img) = self.images.remove(&id) {
            self.pixel_bytes = self.pixel_bytes.saturating_sub(img.byte_len());
            self.evicted.push(id);
            self.numbers.retain(|_, v| *v != id);
        }
    }

    fn decode_payload(&self, cmd: &GraphicsCommand) -> Result<DecodedImage, StoreError> {
        let mut data = load_bytes(cmd)?;
        if cmd.keys.compression == Some(b'z') {
            data = inflate(&data)?;
        }
        let format = cmd.keys.format.unwrap_or(32);
        match format {
            32 => raw_pixels(&data, cmd.keys.width, cmd.keys.height, 4),
            24 => {
                let rgb = raw_pixels(&data, cmd.keys.width, cmd.keys.height, 3)?;
                Ok(rgb_to_rgba(rgb))
            }
            100 => decode_png(&data),
            _ => Err(StoreError::Invalid),
        }
    }

    fn current(&self) -> &ScreenPlacements {
        if self.alt_screen {
            &self.alt
        } else {
            &self.primary
        }
    }

    fn current_mut(&mut self) -> &mut ScreenPlacements {
        if self.alt_screen {
            &mut self.alt
        } else {
            &mut self.primary
        }
    }

    fn ok_resp(&self, cmd: &GraphicsCommand, id: u32) -> GraphicsResponse {
        GraphicsResponse {
            image_id: ImageId(id),
            image_number: cmd.keys.image_number.unwrap_or(0),
            placement_id: cmd.keys.placement_id.unwrap_or(0),
            error: None,
        }
    }

    fn err_resp(&self, cmd: &GraphicsCommand, id: u32, err: StoreError) -> GraphicsResponse {
        GraphicsResponse {
            image_id: ImageId(id),
            image_number: cmd.keys.image_number.unwrap_or(0),
            placement_id: cmd.keys.placement_id.unwrap_or(0),
            error: Some(err.code().to_string()),
        }
    }
}

/// Contexto de cursor/grid para colocar y borrar.
pub struct ExecContext {
    pub cursor_col: usize,
    pub logical_row: usize,
    pub grid_rows: usize,
    pub grid_cols: usize,
    pub cell_px: (u32, u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StoreError {
    Invalid,
    Payload,
    NotFound,
    NoSpace,
    TooBig,
    Io,
}

impl StoreError {
    fn code(self) -> &'static str {
        match self {
            Self::Invalid | Self::Payload => "EINVAL",
            Self::NotFound => "ENOENT",
            Self::NoSpace => "ENOSPC",
            Self::TooBig => "EFBIG",
            Self::Io => "EIO",
        }
    }
}

impl From<GraphicsError> for StoreError {
    fn from(e: GraphicsError) -> Self {
        match e {
            GraphicsError::Payload => Self::Payload,
            GraphicsError::Invalid => Self::Invalid,
        }
    }
}

fn logical_to_visible(logical_row: usize, vp: Viewport) -> Option<usize> {
    let sb_len = vp.scrollback_len;
    let offset = vp.scrollback_offset.max(0) as usize;
    let viewport_start = sb_len.saturating_sub(offset);
    if logical_row < viewport_start {
        return None;
    }
    Some(logical_row - viewport_start)
}

fn src_rect(keys: &Keys, img_w: u32, img_h: u32) -> (u32, u32, u32, u32) {
    let x = keys.src_x.unwrap_or(0).min(img_w);
    let y = keys.src_y.unwrap_or(0).min(img_h);
    let w = keys.src_w.unwrap_or(img_w.saturating_sub(x)).min(img_w - x);
    let h = keys.src_h.unwrap_or(img_h.saturating_sub(y)).min(img_h - y);
    (x, y, w, h)
}

fn dst_cells(keys: &Keys, src_w: u32, src_h: u32, ctx: &ExecContext) -> (u32, u32) {
    let (cw, ch) = ctx.cell_px;
    let cw = cw.max(1);
    let ch = ch.max(1);
    match (keys.columns, keys.rows) {
        (Some(c), Some(r)) => (c.max(1), r.max(1)),
        (Some(c), None) => {
            let c = c.max(1);
            let px_w = c.saturating_mul(cw);
            let px_h = if src_w == 0 {
                src_h
            } else {
                (src_h as u64 * px_w as u64 / src_w as u64) as u32
            };
            (c, px_h.div_ceil(ch).max(1))
        }
        (None, Some(r)) => {
            let r = r.max(1);
            let px_h = r.saturating_mul(ch);
            let px_w = if src_h == 0 {
                src_w
            } else {
                (src_w as u64 * px_h as u64 / src_h as u64) as u32
            };
            (px_w.div_ceil(cw).max(1), r)
        }
        (None, None) => (src_w.div_ceil(cw).max(1), src_h.div_ceil(ch).max(1)),
    }
}

fn load_bytes(cmd: &GraphicsCommand) -> Result<Vec<u8>, StoreError> {
    match cmd.keys.transmission.unwrap_or(b'd') {
        b'd' => Ok(cmd.payload.clone()),
        b'f' => read_image_file(
            &cmd.payload,
            cmd.keys.data_offset,
            cmd.keys.data_size,
            false,
        ),
        b't' => read_image_file(&cmd.payload, cmd.keys.data_offset, cmd.keys.data_size, true),
        _ => Err(StoreError::Invalid),
    }
}

fn read_image_file(
    payload: &[u8],
    offset: Option<u32>,
    size: Option<u32>,
    delete_temp: bool,
) -> Result<Vec<u8>, StoreError> {
    let path = std::str::from_utf8(payload).map_err(|_| StoreError::Invalid)?;
    let path = Path::new(path);
    if !path.is_absolute() || has_parent_dir(path) {
        return Err(StoreError::Invalid);
    }
    let meta = std::fs::metadata(path).map_err(|_| StoreError::NotFound)?;
    if !meta.is_file() {
        return Err(StoreError::Invalid);
    }
    let mut file = std::fs::File::open(path).map_err(|_| StoreError::NotFound)?;
    let start = u64::from(offset.unwrap_or(0));
    if start > 0 {
        std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(start))
            .map_err(|_| StoreError::Io)?;
    }
    let limit = size
        .map(u64::from)
        .unwrap_or_else(|| meta.len().saturating_sub(start))
        .min(MAX_FILE_BYTES);
    let mut buf = Vec::new();
    file.take(limit)
        .read_to_end(&mut buf)
        .map_err(|_| StoreError::Io)?;
    if delete_temp && is_deletable_temp(path) {
        let _ = std::fs::remove_file(path);
    }
    Ok(buf)
}

fn has_parent_dir(path: &Path) -> bool {
    path.components().any(|c| matches!(c, Component::ParentDir))
}

fn is_deletable_temp(path: &Path) -> bool {
    let lossy = path.to_string_lossy();
    if !lossy.contains("tty-graphics-protocol") {
        return false;
    }
    let Ok(canon) = path.canonicalize() else {
        return false;
    };
    allowed_temp_dirs()
        .into_iter()
        .filter_map(|d| d.canonicalize().ok())
        .any(|root| canon.starts_with(root))
}

fn allowed_temp_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![std::env::temp_dir()];
    #[cfg(unix)]
    {
        dirs.push(PathBuf::from("/tmp"));
        dirs.push(PathBuf::from("/dev/shm"));
        if let Ok(tdir) = std::env::var("TMPDIR") {
            dirs.push(PathBuf::from(tdir));
        }
    }
    #[cfg(windows)]
    {
        if let Ok(t) = std::env::var("TMP") {
            dirs.push(PathBuf::from(t));
        }
        if let Ok(t) = std::env::var("TEMP") {
            dirs.push(PathBuf::from(t));
        }
    }
    dirs
}

fn inflate(data: &[u8]) -> Result<Vec<u8>, StoreError> {
    let mut decoder = flate2::read::ZlibDecoder::new(data);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|_| StoreError::Invalid)?;
    if out.len() > MAX_PIXEL_BYTES {
        return Err(StoreError::TooBig);
    }
    Ok(out)
}

fn raw_pixels(
    data: &[u8],
    width: Option<u32>,
    height: Option<u32>,
    bpp: usize,
) -> Result<DecodedImage, StoreError> {
    let w = width.ok_or(StoreError::Invalid)?;
    let h = height.ok_or(StoreError::Invalid)?;
    if w == 0 || h == 0 || w > MAX_DIM || h > MAX_DIM {
        return Err(StoreError::Invalid);
    }
    let expected = w as usize * h as usize * bpp;
    if data.len() < expected {
        return Err(StoreError::Invalid);
    }
    Ok(DecodedImage {
        width: w,
        height: h,
        rgba: data[..expected].to_vec(),
        generation: 0,
    })
}

fn rgb_to_rgba(mut img: DecodedImage) -> DecodedImage {
    let px = (img.width as usize) * (img.height as usize);
    let mut rgba = Vec::with_capacity(px * 4);
    for chunk in img.rgba.chunks_exact(3).take(px) {
        rgba.extend_from_slice(chunk);
        rgba.push(255);
    }
    img.rgba = rgba;
    img
}

fn decode_png(data: &[u8]) -> Result<DecodedImage, StoreError> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(data));
    decoder.set_transformations(
        png::Transformations::EXPAND | png::Transformations::STRIP_16 | png::Transformations::ALPHA,
    );
    let mut reader = decoder.read_info().map_err(|_| StoreError::Invalid)?;
    let info = reader.info();
    let w = info.width;
    let h = info.height;
    if w == 0 || h == 0 || w > MAX_DIM || h > MAX_DIM {
        return Err(StoreError::Invalid);
    }
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let frame = reader
        .next_frame(&mut buf)
        .map_err(|_| StoreError::Invalid)?;
    let needed = (w as usize) * (h as usize) * 4;
    let mut rgba = vec![0u8; needed];
    match frame.color_type {
        png::ColorType::Rgba => {
            let src = &buf[..frame.buffer_size()];
            rgba[..src.len().min(needed)].copy_from_slice(&src[..src.len().min(needed)]);
        }
        png::ColorType::Rgb => {
            for (dst, src) in rgba.chunks_exact_mut(4).zip(buf.chunks_exact(3)) {
                dst[0] = src[0];
                dst[1] = src[1];
                dst[2] = src[2];
                dst[3] = 255;
            }
        }
        png::ColorType::Grayscale => {
            for (dst, &y) in rgba.chunks_exact_mut(4).zip(buf.iter()) {
                dst[0] = y;
                dst[1] = y;
                dst[2] = y;
                dst[3] = 255;
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for (dst, src) in rgba.chunks_exact_mut(4).zip(buf.chunks_exact(2)) {
                dst[0] = src[0];
                dst[1] = src[0];
                dst[2] = src[0];
                dst[3] = src[1];
            }
        }
        png::ColorType::Indexed => return Err(StoreError::Invalid),
    }
    Ok(DecodedImage {
        width: w,
        height: h,
        rgba,
        generation: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::protocol::parse;

    fn ctx_at(row: usize, col: usize) -> ExecContext {
        ExecContext {
            cursor_col: col,
            logical_row: row,
            grid_rows: 24,
            grid_cols: 80,
            cell_px: (10, 20),
        }
    }

    fn png_2x2() -> Vec<u8> {
        let pixels = [
            255u8, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
        ];
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, 2, 2);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().unwrap();
        writer.write_image_data(&pixels).unwrap();
        drop(writer);
        out
    }

    fn transmit_png(store: &mut GraphicsStore, ctx: &ExecContext) -> GraphicsResponse {
        let png = png_2x2();
        let b64 = crate::base64::encode(&png);
        let raw = format!("Ga=T,f=100,i=1;{b64}");
        let cmd = parse(raw.as_bytes()).unwrap();
        store.execute(cmd, ctx)
    }

    #[test]
    fn imagen_png_se_decodifica_y_cabe_en_cuota() {
        let mut store = GraphicsStore::default();
        let resp = transmit_png(&mut store, &ctx_at(0, 0));
        assert!(resp.error.is_none());
        let img = store.image(ImageId(1)).unwrap();
        assert_eq!(img.width, 2);
        assert_eq!(img.height, 2);
        assert_eq!(img.rgba.len(), 16);
    }

    #[test]
    fn imagen_sobre_cuota_responde_error_y_no_se_guarda() {
        let mut store = GraphicsStore::default();
        let cmd = parse(b"Ga=t,f=32,s=5000,v=5000,i=2;").unwrap();
        let resp = store.execute(cmd, &ctx_at(0, 0));
        assert_eq!(resp.error.as_deref(), Some("EINVAL"));
        assert!(store.image(ImageId(2)).is_none());
    }

    #[test]
    fn placement_scrollea_con_el_contenido() {
        let mut store = GraphicsStore::default();
        transmit_png(&mut store, &ctx_at(10, 0));
        // Tres líneas nuevas al scrollback: el ancla lógica 10 sigue en 10,
        // y con 3 de historia queda en visible 7.
        let vis = store.visible_placements(Viewport {
            rows: 24,
            cols: 80,
            scrollback_len: 3,
            scrollback_offset: 0,
        });
        assert_eq!(vis.len(), 1);
        assert_eq!(vis[0].dst_cells.0, 7);
    }

    #[test]
    fn placement_muere_cuando_su_linea_sale_del_scrollback() {
        let mut store = GraphicsStore::default();
        transmit_png(&mut store, &ctx_at(2, 0));
        store.reconcile_trim(5);
        let vis = store.visible_placements(Viewport {
            rows: 24,
            cols: 80,
            scrollback_len: 0,
            scrollback_offset: 0,
        });
        assert!(vis.is_empty());
    }

    #[test]
    fn alt_screen_tiene_placements_propios() {
        let mut store = GraphicsStore::default();
        transmit_png(&mut store, &ctx_at(1, 0));
        store.enter_alt_screen();
        let vis = store.visible_placements(Viewport {
            rows: 24,
            cols: 80,
            scrollback_len: 0,
            scrollback_offset: 0,
        });
        assert!(vis.is_empty());
        let cmd = parse(b"Ga=T,f=24,s=1,v=1,i=9;AAAA").unwrap();
        store.execute(cmd, &ctx_at(0, 0));
        store.exit_alt_screen();
        assert!(store.image(ImageId(9)).is_none());
        let vis = store.visible_placements(Viewport {
            rows: 24,
            cols: 80,
            scrollback_len: 0,
            scrollback_offset: 0,
        });
        assert_eq!(vis.len(), 1);
        assert_eq!(vis[0].image_id, ImageId(1));
    }

    #[test]
    fn delete_por_id_y_delete_all() {
        let mut store = GraphicsStore::default();
        transmit_png(&mut store, &ctx_at(0, 0));
        let cmd = parse(b"Ga=T,f=24,s=1,v=1,i=3;AAAA").unwrap();
        store.execute(cmd, &ctx_at(0, 1));
        let del = parse(b"Ga=d,d=i,i=1").unwrap();
        store.execute(del, &ctx_at(0, 0));
        assert_eq!(store.current().items.len(), 1);
        let del_all = parse(b"Ga=d,d=a").unwrap();
        store.execute(del_all, &ctx_at(0, 0));
        assert!(store.current().items.is_empty());
    }

    #[test]
    fn query_no_almacena() {
        let mut store = GraphicsStore::default();
        let cmd = parse(b"Ga=q,i=31,s=1,v=1,t=d,f=24;AAAA").unwrap();
        let resp = store.execute(cmd, &ctx_at(0, 0));
        assert!(resp.error.is_none());
        assert!(store.image(ImageId(31)).is_none());
    }

    #[test]
    fn archivo_inexistente_es_enoent() {
        let mut store = GraphicsStore::default();
        let path = b"/no/existe/baud-graphics-missing.png";
        let b64 = crate::base64::encode(path);
        let raw = format!("Ga=t,t=f,f=100,i=4;{b64}");
        let cmd = parse(raw.as_bytes()).unwrap();
        let resp = store.execute(cmd, &ctx_at(0, 0));
        assert_eq!(resp.error.as_deref(), Some("ENOENT"));
    }
}
