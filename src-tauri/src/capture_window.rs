use std::num::NonZeroU32;
use std::sync::{mpsc, Arc, OnceLock};

use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::window::{Fullscreen, Window, WindowId, WindowLevel};

// ── Public types ──────────────────────────────────────────────────────────────

/// Commands sent into the event loop (from async tasks or begin_capture).
pub enum CaptureCommand {
    /// Begin a new capture session with fresh screenshot data.
    StartCapture {
        rgba: Vec<u8>,
        img_w: u32,
        img_h: u32,
        scale_factor: f64,
        monitor_x: i32,
        monitor_y: i32,
        event_tx: mpsc::Sender<CaptureEvent>,
    },
    /// Display a translated result image over the selection area.
    ShowResult {
        rgba_bytes: Vec<u8>,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
    },
    /// Show a spinning loader over the current selection while API call is in progress.
    ShowLoading,
    /// Close the current capture window.
    Close,
}

/// Events emitted from the capture window to Rust async tasks.
pub enum CaptureEvent {
    /// User finished dragging a selection.
    Selection { x: u32, y: u32, w: u32, h: u32 },
    /// User pressed ESC or the window was closed.
    Cancelled,
}

// ── Singleton event loop ───────────────────────────────────────────────────────
//
// winit on Windows only allows ONE EventLoop per process (building a second one
// panics with "RecreationAttempt"). We keep a single background thread running
// the event loop for the entire app lifetime. Individual captures are started by
// sending CaptureCommand::StartCapture through the proxy.

static CAPTURE_PROXY: OnceLock<EventLoopProxy<CaptureCommand>> = OnceLock::new();

/// Get (or lazily create) the singleton event-loop proxy.
/// The event loop lives on a dedicated background thread for the app's lifetime.
pub fn capture_proxy() -> EventLoopProxy<CaptureCommand> {
    CAPTURE_PROXY
        .get_or_init(|| {
            let (proxy_tx, proxy_rx) = mpsc::sync_channel::<EventLoopProxy<CaptureCommand>>(1);
            std::thread::Builder::new()
                .name("capture-event-loop".into())
                .spawn(move || {
                    #[cfg(target_os = "windows")]
                    let event_loop = {
                        use winit::platform::windows::EventLoopBuilderExtWindows;
                        EventLoop::<CaptureCommand>::with_user_event()
                            .with_any_thread(true)
                            .build()
                            .expect("failed to build winit event loop")
                    };
                    #[cfg(not(target_os = "windows"))]
                    let event_loop = EventLoop::<CaptureCommand>::with_user_event()
                        .build()
                        .expect("failed to build winit event loop");

                    let proxy = event_loop.create_proxy();
                    let _ = proxy_tx.send(proxy);

                    let mut handler = CaptureHandler::idle();
                    event_loop
                        .run_app(&mut handler)
                        .expect("capture event loop crashed");
                })
                .expect("failed to spawn capture thread");

            proxy_rx
                .recv()
                .expect("capture event loop died before sending proxy")
        })
        .clone()
}

/// Begin a new capture session.  Does not block — the window appears asynchronously.
pub fn start_capture(
    rgba: Vec<u8>,
    img_w: u32,
    img_h: u32,
    scale_factor: f64,
    monitor_x: i32,
    monitor_y: i32,
    event_tx: mpsc::Sender<CaptureEvent>,
) {
    let _ = capture_proxy().send_event(CaptureCommand::StartCapture {
        rgba,
        img_w,
        img_h,
        scale_factor,
        monitor_x,
        monitor_y,
        event_tx,
    });
}

// ── Internal handler ──────────────────────────────────────────────────────────

/// Possible states the handler can be in.
enum HandlerState {
    /// No active capture; window is closed.
    Idle,
    /// Active capture: window exists, user is selecting.
    Selecting(CaptureSession),
}

struct CaptureSession {
    img_w: u32,
    img_h: u32,
    scale_factor: f64,
    original_pixels: Vec<u32>,
    darkened_pixels: Vec<u32>,
    event_tx: mpsc::Sender<CaptureEvent>,
    window: Arc<Window>,
    surface: Surface<Arc<Window>, Arc<Window>>,
    // Drag state
    drag_start: Option<PhysicalPosition<f64>>,
    selection: Option<(u32, u32, u32, u32)>,
    is_dragging: bool,
    mouse_pos: PhysicalPosition<f64>,
    // Result overlay
    result: Option<ResultOverlay>,
    // Loading animation
    loading: bool,
    loading_start: Option<std::time::Instant>,
    // Track whether the surface has been resized to the window's real physical size.
    surface_ready: bool,
    // Track whether the window has been made visible after the first successful paint.
    shown: bool,
}

struct ResultOverlay {
    pixels: Vec<u32>,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

struct CaptureHandler {
    state: HandlerState,
    /// Kept alive for the lifetime of the handler so the softbuffer Context lives.
    _ctx_storage: Option<Context<Arc<Window>>>,
}

impl CaptureHandler {
    fn idle() -> Self {
        Self {
            state: HandlerState::Idle,
            _ctx_storage: None,
        }
    }

    /// Create a new capture window for the given RGBA snapshot.
    fn open_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        rgba: Vec<u8>,
        img_w: u32,
        img_h: u32,
        scale_factor: f64,
        monitor_x: i32,
        monitor_y: i32,
        event_tx: mpsc::Sender<CaptureEvent>,
    ) {
        // Find the monitor handle matching the given coordinates
        let target_monitor = event_loop.available_monitors().find(|m| {
            let pos = m.position();
            pos.x == monitor_x && pos.y == monitor_y
        });

        let fullscreen = match target_monitor {
            Some(m) => Fullscreen::Borderless(Some(m)),
            None => Fullscreen::Borderless(None),
        };

        let attrs = Window::default_attributes()
            .with_title("Capture")
            .with_decorations(false)
            .with_resizable(false)
            .with_fullscreen(Some(fullscreen))
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_visible(false);

        #[cfg(target_os = "macos")]
        let attrs = {
            use winit::platform::macos::WindowAttributesExtMacOS;
            attrs.with_fullsize_content_view(true)
        };

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::error!("Failed to create capture window: {e}");
                let _ = event_tx.send(CaptureEvent::Cancelled);
                return;
            }
        };

        let ctx = Context::new(window.clone()).expect("softbuffer context");
        let surface = Surface::new(&ctx, window.clone()).expect("softbuffer surface");

        let original_pixels = rgba_to_softbuffer(&rgba);
        let darkened_pixels = darken_pixels(&original_pixels, 0.55);

        self._ctx_storage = Some(ctx);
        self.state = HandlerState::Selecting(CaptureSession {
            img_w,
            img_h,
            scale_factor,
            original_pixels,
            darkened_pixels,
            event_tx,
            window,
            surface,
            drag_start: None,
            selection: None,
            is_dragging: false,
            mouse_pos: PhysicalPosition::new(0.0, 0.0),
            result: None,
            loading: false,
            loading_start: None,
            surface_ready: false,
            shown: false,
        });

        // Pre-paint before showing the window to avoid white flash.
        // Windows does not send WM_SIZE to invisible windows, so we must resize
        // the surface ourselves using the known screenshot dimensions.
        if let HandlerState::Selecting(ref mut session) = self.state {
            if let (Some(nz_w), Some(nz_h)) = (NonZeroU32::new(img_w), NonZeroU32::new(img_h)) {
                if session.surface.resize(nz_w, nz_h).is_ok() {
                    session.surface_ready = true;
                    tracing::debug!("[capture_window] pre-paint buffer, about to set_visible");
                    if let Ok(mut buffer) = session.surface.buffer_mut() {
                        if buffer.len() == (img_w * img_h) as usize {
                            buffer.copy_from_slice(&session.darkened_pixels);
                            let _ = buffer.present();
                            tracing::debug!("[capture_window] present done, now set_visible");
                            session.shown = true;
                            session.window.set_visible(true);
                            tracing::debug!("[capture_window] set_visible done");
                        }
                    }
                }
            }
        }
    }

    fn close_window(&mut self) {
        // Dropping the session closes the window (Arc<Window> refcount → 0).
        self.state = HandlerState::Idle;
        self._ctx_storage = None;
    }
}

impl ApplicationHandler<CaptureCommand> for CaptureHandler {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let session = match &mut self.state {
            HandlerState::Selecting(s) => s,
            HandlerState::Idle => return,
        };

        match event {
            // ── Once the OS tells us the window's real physical size, mark surface ready.
            WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 {
                    let _ = session.surface.resize(
                        NonZeroU32::new(size.width).unwrap(),
                        NonZeroU32::new(size.height).unwrap(),
                    );
                    session.surface_ready = true;
                    session.window.request_redraw();
                }
            }

            WindowEvent::RedrawRequested => {
                redraw_session(session);
            }

            WindowEvent::KeyboardInput {
                event: key_event, ..
            } => {
                use winit::keyboard::{KeyCode, PhysicalKey};
                if key_event.state == ElementState::Pressed {
                    if let PhysicalKey::Code(KeyCode::Escape) = key_event.physical_key {
                        let _ = session.event_tx.send(CaptureEvent::Cancelled);
                        self.close_window();
                    }
                }
            }

            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                match state {
                    ElementState::Pressed => {
                        session.drag_start = Some(session.mouse_pos);
                        session.is_dragging = true;
                        session.selection = None;
                        session.result = None;
                    }
                    ElementState::Released => {
                        if session.is_dragging {
                            if let Some(start) = session.drag_start {
                                let rect = normalize_rect(start, session.mouse_pos);
                                session.selection = Some(rect);
                            }
                            session.is_dragging = false;
                            session.drag_start = None;
                            finish_selection(session);
                        }
                    }
                }
                session.window.request_redraw();
            }

            WindowEvent::CursorMoved { position, .. } => {
                session.mouse_pos = position;
                if session.is_dragging {
                    session.window.request_redraw();
                }
            }

            // Right-click to cancel capture.
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => {
                let _ = session.event_tx.send(CaptureEvent::Cancelled);
                self.close_window();
            }

            WindowEvent::CloseRequested => {
                let _ = session.event_tx.send(CaptureEvent::Cancelled);
                self.close_window();
            }

            _ => {}
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: CaptureCommand) {
        match event {
            CaptureCommand::StartCapture {
                rgba,
                img_w,
                img_h,
                scale_factor,
                monitor_x,
                monitor_y,
                event_tx,
            } => {
                // Always close any previous window before opening a new one.
                self.close_window();
                self.open_window(event_loop, rgba, img_w, img_h, scale_factor, monitor_x, monitor_y, event_tx);
            }

            CaptureCommand::ShowResult {
                rgba_bytes,
                x,
                y,
                w,
                h,
            } => {
                if let HandlerState::Selecting(session) = &mut self.state {
                    let pixels = rgba_to_softbuffer(&rgba_bytes);
                    session.result = Some(ResultOverlay { pixels, x, y, w, h });
                    session.loading = false;
                    session.window.request_redraw();
                }
            }

            CaptureCommand::ShowLoading => {
                if let HandlerState::Selecting(session) = &mut self.state {
                    session.loading = true;
                    session.loading_start = Some(std::time::Instant::now());
                    session.window.request_redraw();
                }
            }

            CaptureCommand::Close => {
                if let HandlerState::Selecting(session) = &mut self.state {
                    let _ = session.event_tx.send(CaptureEvent::Cancelled);
                }
                self.close_window();
            }
        }
    }
}

// ── Per-frame rendering ───────────────────────────────────────────────────────

fn redraw_session(session: &mut CaptureSession) {
    if !session.surface_ready {
        return;
    }

    let mut buffer = match session.surface.buffer_mut() {
        Ok(b) => b,
        Err(_) => return,
    };

    let buf_len = buffer.len();
    let expected = (session.img_w * session.img_h) as usize;

    if buf_len != expected {
        buffer.fill(0);
        let _ = buffer.present();
        return;
    }

    let width = session.img_w;
    let height = session.img_h;

    // Start with darkened screenshot.
    buffer.copy_from_slice(&session.darkened_pixels);

    // If there's a result overlay, paint it on top.
    if let Some(ref res) = session.result {
        // res.pixels is a compact res.w×res.h image — stride equals res.w, offset (0,0).
        blit_pixels(
            &mut buffer,
            width,
            &res.pixels,
            res.w,
            0,
            0,
            res.x,
            res.y,
            res.w,
            res.h,
        );
        let _ = buffer.present();
        return;
    }

    // Determine current selection rect.
    let sel = if session.is_dragging {
        session
            .drag_start
            .map(|start| normalize_rect(start, session.mouse_pos))
    } else {
        session.selection
    };

    if let Some((sx, sy, sw, sh)) = sel {
        if sw > 0 && sh > 0 {
            // original_pixels is the full img_w×img_h screenshot — stride = img_w,
            // source origin = (sx, sy) so we read the correct region.
            blit_pixels(
                &mut buffer,
                width,
                &session.original_pixels,
                session.img_w,
                sx,
                sy,
                sx,
                sy,
                sw,
                sh,
            );
            draw_border(&mut buffer, width, height, sx, sy, sw, sh, 0x004A9EFF, 2);
        }
    }

    // Loading spinner overlay.
    if session.loading {
        if let Some((sx, sy, sw, sh)) = session.selection {
            let elapsed = session
                .loading_start
                .map(|t| t.elapsed().as_secs_f32())
                .unwrap_or(0.0);
            draw_spinner(&mut buffer, width, height, sx, sy, sw, sh, elapsed);
        }
        session.window.request_redraw();
    }

    let _ = buffer.present();

    // Reveal the window only after the first successful paint — prevents the
    // white-flash that occurs when the OS shows the window before pixels are ready.
    if !session.shown {
        session.shown = true;
        session.window.set_visible(true);
    }
}

fn finish_selection(session: &CaptureSession) {
    if let Some((x, y, w, h)) = session.selection {
        if w > 4 && h > 4 {
            let _ = session
                .event_tx
                .send(CaptureEvent::Selection { x, y, w, h });
        }
    }
}

// ── Pixel helpers ─────────────────────────────────────────────────────────────

/// Convert RGBA bytes (as returned by `screenshots` crate) to softbuffer's 0x00RRGGBB u32s.
fn rgba_to_softbuffer(rgba: &[u8]) -> Vec<u32> {
    rgba.chunks_exact(4)
        .map(|px| ((px[0] as u32) << 16) | ((px[1] as u32) << 8) | (px[2] as u32))
        .collect()
}

fn darken_pixels(pixels: &[u32], factor: f32) -> Vec<u32> {
    pixels
        .iter()
        .map(|&p| {
            let r = (((p >> 16) & 0xFF) as f32 * factor) as u32;
            let g = (((p >> 8) & 0xFF) as f32 * factor) as u32;
            let b = ((p & 0xFF) as f32 * factor) as u32;
            (r << 16) | (g << 8) | b
        })
        .collect()
}

/// Blit a rectangular region from `src` into `dst`.
///
/// - `src_stride`: row stride of `src` in pixels (may differ from `w` when `src` is a
///   sub-region of a larger image, e.g. the full-resolution screenshot).
/// - `src_ox`, `src_oy`: pixel offset within `src` where reading starts.
fn blit_pixels(
    dst: &mut [u32],
    dst_w: u32,
    src: &[u32],
    src_stride: u32,
    src_ox: u32,
    src_oy: u32,
    dx: u32,
    dy: u32,
    w: u32,
    h: u32,
) {
    let dst_w = dst_w as usize;
    let src_stride = src_stride as usize;
    let len = w as usize;
    for row in 0..(h as usize) {
        let dst_start = (dy as usize + row) * dst_w + dx as usize;
        let src_start = (src_oy as usize + row) * src_stride + src_ox as usize;
        if dst_start + len <= dst.len() && src_start + len <= src.len() {
            dst[dst_start..dst_start + len].copy_from_slice(&src[src_start..src_start + len]);
        }
    }
}

fn draw_border(
    buf: &mut [u32],
    buf_w: u32,
    buf_h: u32,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    color: u32,
    thickness: u32,
) {
    let bw = buf_w as usize;
    let x2 = (x + w).min(buf_w);
    let y2 = (y + h).min(buf_h);
    for t in 0..thickness {
        let top = (y + t) as usize;
        let bot = y2.saturating_sub(1).saturating_sub(t) as usize;
        for col in x..x2 {
            let c = col as usize;
            if top < buf_h as usize {
                let i = top * bw + c;
                if i < buf.len() {
                    buf[i] = color;
                }
            }
            if bot != top && bot < buf_h as usize {
                let i = bot * bw + c;
                if i < buf.len() {
                    buf[i] = color;
                }
            }
        }
        let left = (x + t) as usize;
        let right = x2.saturating_sub(1).saturating_sub(t) as usize;
        for row in y..y2 {
            let r = row as usize;
            if r < buf_h as usize {
                let li = r * bw + left;
                if li < buf.len() {
                    buf[li] = color;
                }
                if right != left {
                    let ri = r * bw + right;
                    if ri < buf.len() {
                        buf[ri] = color;
                    }
                }
            }
        }
    }
}

/// Draw a spinning arc loader centered on the selection rect.
fn draw_spinner(
    buf: &mut [u32],
    buf_w: u32,
    buf_h: u32,
    sx: u32,
    sy: u32,
    sw: u32,
    sh: u32,
    elapsed: f32,
) {
    let cx = sx as f32 + sw as f32 / 2.0;
    let cy = sy as f32 + sh as f32 / 2.0;
    let r = (sw.min(sh) as f32 * 0.15).clamp(12.0, 28.0);
    let line_w = 3u32;
    let arc_span = std::f32::consts::PI * 1.5; // 270°
    let angle_start = elapsed * std::f32::consts::TAU; // 1 rotation/sec

    let steps = ((r + line_w as f32) * std::f32::consts::TAU * 2.0) as usize + 8;
    for i in 0..steps {
        let a = angle_start + (i as f32 / steps as f32) * arc_span;
        for w in 0..line_w {
            let rr = r - line_w as f32 / 2.0 + w as f32;
            let px = (cx + rr * a.cos()).round() as i32;
            let py = (cy + rr * a.sin()).round() as i32;
            if px >= 0 && py >= 0 && px < buf_w as i32 && py < buf_h as i32 {
                let idx = py as usize * buf_w as usize + px as usize;
                if idx < buf.len() {
                    buf[idx] = 0x004A9EFF;
                }
            }
        }
    }
}

fn normalize_rect(a: PhysicalPosition<f64>, b: PhysicalPosition<f64>) -> (u32, u32, u32, u32) {
    let x1 = a.x.min(b.x).max(0.0) as u32;
    let y1 = a.y.min(b.y).max(0.0) as u32;
    let x2 = a.x.max(b.x).max(0.0) as u32;
    let y2 = a.y.max(b.y).max(0.0) as u32;
    (x1, y1, x2.saturating_sub(x1), y2.saturating_sub(y1))
}

// ── Crop / encode helpers (used by commands.rs) ───────────────────────────────

pub fn crop_rgba(rgba: &[u8], img_w: u32, x: u32, y: u32, w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for row in y..(y + h) {
        let start = ((row * img_w + x) * 4) as usize;
        let end = start + (w * 4) as usize;
        if end <= rgba.len() {
            out.extend_from_slice(&rgba[start..end]);
        }
    }
    out
}

pub fn encode_png(rgba: &[u8], w: u32, h: u32) -> crate::error::AppResult<Vec<u8>> {
    use image::{ImageBuffer, RgbaImage};
    let img: RgbaImage = ImageBuffer::from_raw(w, h, rgba.to_vec())
        .ok_or_else(|| crate::error::AppError::Capture("invalid RGBA dimensions for PNG".into()))?;
    let mut png_bytes: Vec<u8> = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut png_bytes),
        image::ImageFormat::Png,
    )
    .map_err(|e| crate::error::AppError::Capture(format!("PNG encode error: {e}")))?;
    Ok(png_bytes)
}