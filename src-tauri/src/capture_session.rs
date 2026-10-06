//! Persistent Windows Graphics Capture session for the scanner's target window.
//!
//! xcap builds a capture item, frame pool and session for every single frame, starts it,
//! and blocks until the compositor delivers a first frame — that wait is the steady
//! ~60–70ms `capture_ms` in the scanner log, alongside a burst of D3D driver calls each
//! tick. (Its capture-item cache doesn't help either: `xcap::Window`'s Drop evicts it, and
//! the scanner drops the whole window list it enumerates every tick.)
//!
//! This keeps one session open per bound window and hands out the newest frame on
//! request. A free-threaded `FrameArrived` handler only swaps a reference to the latest
//! frame (no copying), so a capture is just a GPU→CPU copy of a frame that already
//! exists. The output reproduces xcap's exactly — the window's DWM extended frame bounds
//! cropped from (0,0) of the captured surface, BGRA swapped to RGBA — so OCR regions see
//! the same pixels. Border/cursor settings are the same best-effort calls xcap makes.
//!
//! Anything unexpected (session can't start, window gone or renamed, frame size not
//! matching yet) returns `None` and the caller falls back to the regular xcap path.

use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use image::RgbaImage;
use windows::core::{factory, IInspectable, Interface};
use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Graphics::Capture::{
    Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem,
    GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{HMODULE, HWND, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D,
    D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsProcessDPIAware, IsWindow,
};

use crate::capture::CaptureFailure;
use crate::db;
use crate::settings::TargetWindow;

/// Ceiling on how often the compositor hands the session a new frame. Without it, an
/// open session gets a copy of the window every time it presents — up to the emulator's
/// own frame rate (120 fps here) — which is GPU work the game competes with. The scanner
/// reads at most one frame a second, so ~10 fps keeps frames fresh to within ~100ms.
/// Supported from Windows 11 24H2; on older builds the call fails and is ignored.
const MIN_UPDATE_INTERVAL: Duration = Duration::from_millis(100);

/// How long a capture waits for a just-started session's first frame before letting the
/// caller fall back to the regular path for this tick.
const FIRST_FRAME_WAIT: Duration = Duration::from_millis(500);

/// One buffer is held as "latest" between captures; the other receives the next frame.
const FRAME_BUFFERS: i32 = 2;

const PIXEL_FORMAT: DirectXPixelFormat = DirectXPixelFormat::B8G8R8A8UIntNormalized;

struct Device {
    d3d: ID3D11Device,
    context: ID3D11DeviceContext,
    winrt: IDirect3DDevice,
}

impl Device {
    fn create() -> windows::core::Result<Device> {
        let mut d3d = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut d3d),
                None,
                Some(&mut context),
            )?;
        }
        let d3d: ID3D11Device = d3d.ok_or_else(windows::core::Error::empty)?;
        let context = context.ok_or_else(windows::core::Error::empty)?;
        let dxgi: IDXGIDevice = d3d.cast()?;
        let winrt: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? }.cast()?;
        Ok(Device { d3d, context, winrt })
    }
}

struct Bound {
    hwnd: HWND,
    pid: u32,
    title_substring: String,
    process_name: String,
    user_selected: bool,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    frame_token: i64,
    pool_size: SizeInt32,
    latest: Arc<Mutex<Option<Direct3D11CaptureFrame>>>,
    /// CPU-readable copy target, reused while the window size stays the same.
    staging: Option<(ID3D11Texture2D, u32, u32)>,
}

impl Drop for Bound {
    fn drop(&mut self) {
        let _ = self.pool.RemoveFrameArrived(self.frame_token);
        if let Ok(mut latest) = self.latest.lock() {
            *latest = None;
        }
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

/// The scanner capture worker's session state. Lives on that one thread for its lifetime;
/// if the worker is abandoned after a capture timeout, its session goes with it and the
/// replacement worker starts fresh.
#[derive(Default)]
pub struct WindowSession {
    device: Option<Device>,
    bound: Option<Bound>,
}

impl WindowSession {
    pub fn new() -> Self {
        Self::default()
    }

    /// Capture from the bound session. `None` means it can't answer for this target right
    /// now (nothing bound, a different target, the window gone or retitled, no frame yet,
    /// or an error) and the caller should use the regular capture path.
    pub fn capture(&mut self, target: &TargetWindow) -> Option<Result<RgbaImage, CaptureFailure>> {
        let bound = self.bound.as_mut()?;
        if !bound.serves(target) || !bound.window_still_matches() {
            self.unbind("window changed");
            return None;
        }
        if unsafe { IsIconic(bound.hwnd) }.as_bool() {
            return Some(Err(CaptureFailure::Minimized));
        }
        let device = self.device.as_ref()?;
        match bound.grab(device) {
            Ok(Some(img)) => Some(Ok(img)),
            Ok(None) => None,
            Err(e) => {
                self.unbind(&format!("capture error: {e}"));
                None
            }
        }
    }

    /// Start a session on `window_id` (an xcap window id, i.e. the HWND) after the regular
    /// path successfully captured it for `target`. Failures are logged and leave nothing
    /// bound, so capture simply keeps using the regular path.
    pub fn bind(&mut self, target: &TargetWindow, window_id: u32) {
        // Handles are 32-bit values sign-extended to pointer width.
        let hwnd = HWND(window_id as i32 as isize as *mut c_void);
        if self
            .bound
            .as_ref()
            .is_some_and(|b| b.hwnd == hwnd && b.serves(target))
        {
            return;
        }
        self.bound = None;
        // xcap scales the crop for DPI-unaware callers capturing DPI-aware windows. Tauri
        // makes this process DPI-aware, so that never applies; if it somehow didn't, stay
        // on the regular path rather than risk a differently-sized frame.
        if !unsafe { IsProcessDPIAware() }.as_bool() {
            db::append_app_log("capture session: not started (process is not DPI-aware)");
            return;
        }
        if self.device.is_none() {
            match Device::create() {
                Ok(device) => self.device = Some(device),
                Err(e) => {
                    db::append_app_log(&format!("capture session: D3D device failed: {e}"));
                    return;
                }
            }
        }
        let device = self.device.as_ref().expect("device just created");
        match Bound::start(device, target, hwnd) {
            Ok(bound) => {
                db::append_app_log(&format!(
                    "capture session: started for window {window_id:#x} ({}x{})",
                    bound.pool_size.Width, bound.pool_size.Height
                ));
                self.bound = Some(bound);
            }
            Err(e) => {
                db::append_app_log(&format!("capture session: start failed: {e}"));
            }
        }
    }

    fn unbind(&mut self, reason: &str) {
        if self.bound.take().is_some() {
            db::append_app_log(&format!("capture session: stopped ({reason})"));
        }
    }
}

impl Bound {
    fn start(device: &Device, target: &TargetWindow, hwnd: HWND) -> windows::core::Result<Bound> {
        let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(hwnd)? };
        let pool_size = item.Size()?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &device.winrt,
            PIXEL_FORMAT,
            FRAME_BUFFERS,
            pool_size,
        )?;
        let latest: Arc<Mutex<Option<Direct3D11CaptureFrame>>> = Arc::new(Mutex::new(None));
        let sink = latest.clone();
        // Replacing the held frame releases the previous one, returning its buffer to the
        // pool. Frames are never Close()d here: the capture thread may still be copying
        // from the one being replaced, and holds its own reference until done.
        let frame_token = pool.FrameArrived(&TypedEventHandler::<
            Direct3D11CaptureFramePool,
            IInspectable,
        >::new(move |pool, _| {
            if let Some(pool) = pool.as_ref() {
                if let Ok(frame) = pool.TryGetNextFrame() {
                    if let Ok(mut slot) = sink.lock() {
                        *slot = Some(frame);
                    }
                }
            }
            Ok(())
        }))?;
        let session = pool.CreateCaptureSession(&item)?;
        // Same best-effort settings xcap applies to its sessions.
        let _ = session.SetIsBorderRequired(false);
        let _ = session.SetIsCursorCaptureEnabled(false);
        let _ = session.SetMinUpdateInterval(TimeSpan {
            Duration: (MIN_UPDATE_INTERVAL.as_nanos() / 100) as i64,
        });
        session.StartCapture()?;

        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        Ok(Bound {
            hwnd,
            pid,
            title_substring: target.title_substring.clone(),
            process_name: target.process_name.clone(),
            user_selected: target.user_selected,
            pool,
            session,
            frame_token,
            pool_size,
            latest,
            staging: None,
        })
    }

    fn serves(&self, target: &TargetWindow) -> bool {
        self.title_substring == target.title_substring
            && self.process_name == target.process_name
            && self.user_selected == target.user_selected
    }

    /// Cheap per-tick re-check of what the regular path's window search established: the
    /// window still exists, belongs to the same process (so its app name is unchanged),
    /// and its title still matches the target the way that search compares it.
    fn window_still_matches(&self) -> bool {
        if !unsafe { IsWindow(Some(self.hwnd)) }.as_bool() {
            return false;
        }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(self.hwnd, Some(&mut pid)) };
        if pid != self.pid {
            return false;
        }
        let mut buf = [0u16; 512];
        let len = unsafe { GetWindowTextW(self.hwnd, &mut buf) }.max(0) as usize;
        let title = String::from_utf16_lossy(&buf[..len]).to_lowercase();
        let wanted = self.title_substring.to_lowercase();
        if self.user_selected {
            title == wanted
        } else {
            title.contains(&wanted)
        }
    }

    /// `Ok(None)`: no usable frame for this tick (session just started, or the window's
    /// size is changing) — fall back without dropping the session.
    fn grab(&mut self, device: &Device) -> windows::core::Result<Option<RgbaImage>> {
        let deadline = Instant::now() + FIRST_FRAME_WAIT;
        let frame = loop {
            let held = self.latest.lock().ok().and_then(|slot| slot.clone());
            if let Some(frame) = held {
                break frame;
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(5));
        };

        // The surface follows the window's size only after the pool is recreated at it.
        let content = frame.ContentSize()?;
        if content.Width != self.pool_size.Width || content.Height != self.pool_size.Height {
            self.pool
                .Recreate(&device.winrt, PIXEL_FORMAT, FRAME_BUFFERS, content)?;
            self.pool_size = content;
        }

        let mut bounds = RECT::default();
        unsafe {
            DwmGetWindowAttribute(
                self.hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                &mut bounds as *mut RECT as *mut c_void,
                std::mem::size_of::<RECT>() as u32,
            )?;
        }
        let width = (bounds.right - bounds.left).max(0) as u32;
        let height = (bounds.bottom - bounds.top).max(0) as u32;

        let surface = frame.Surface()?;
        let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
        let texture: ID3D11Texture2D = unsafe { access.GetInterface()? };
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };
        // xcap fails this case ("ROI out of bounds"): a frame from before a resize.
        if width == 0 || height == 0 || width > desc.Width || height > desc.Height {
            return Ok(None);
        }

        let staging = match &self.staging {
            Some((tex, w, h)) if *w == width && *h == height => tex.clone(),
            _ => {
                let mut staging_desc = desc;
                staging_desc.Width = width;
                staging_desc.Height = height;
                staging_desc.BindFlags = 0;
                staging_desc.MiscFlags = 0;
                staging_desc.Usage = D3D11_USAGE_STAGING;
                staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
                let mut tex = None;
                unsafe { device.d3d.CreateTexture2D(&staging_desc, None, Some(&mut tex))? };
                let tex = tex.ok_or_else(windows::core::Error::empty)?;
                self.staging = Some((tex.clone(), width, height));
                tex
            }
        };

        let region = D3D11_BOX {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
            front: 0,
            back: 1,
        };
        let staging_resource: ID3D11Resource = staging.cast()?;
        let source_resource: ID3D11Resource = texture.cast()?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        let rgba = unsafe {
            device.context.CopySubresourceRegion(
                &staging_resource,
                0,
                0,
                0,
                0,
                &source_resource,
                0,
                Some(&region),
            );
            device
                .context
                .Map(&staging_resource, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            let row_bytes = (width * 4) as usize;
            let mut rgba = vec![0u8; row_bytes * height as usize];
            let src = mapped.pData as *const u8;
            for row in 0..height as usize {
                let line = std::slice::from_raw_parts(
                    src.add(row * mapped.RowPitch as usize),
                    row_bytes,
                );
                let out = &mut rgba[row * row_bytes..(row + 1) * row_bytes];
                out.copy_from_slice(line);
                // BGRA → RGBA, as xcap's `bgra_to_rgba` (its alpha fix-up is Windows 7 only).
                for px in out.as_chunks_mut::<4>().0 {
                    px.swap(0, 2);
                }
            }
            device.context.Unmap(&staging_resource, 0);
            rgba
        };
        Ok(RgbaImage::from_raw(width, height, rgba))
    }
}
