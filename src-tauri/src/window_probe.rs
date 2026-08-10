//! Raw OS-level state of a target window, used only when a capture fails.
//!
//! The window list we capture from (`xcap::Window::all`) silently drops windows that
//! are DWM-cloaked, not visible, or have empty bounds, so "the emulator is minimized",
//! "the emulator moved to another virtual desktop", "the session is locked" and "the
//! emulator hid its own window" all look identical from there: no matching window.
//! This module asks Win32 directly so the log can name which one it was.

/// One line describing every top-level window whose title matches `title`, or a note
/// that no such window exists. Windows-only; other platforms return `None`.
#[cfg(target_os = "windows")]
pub fn describe_matching_windows(title: &str) -> Option<String> {
    use std::ffi::c_void;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, MAX_PATH, RECT, TRUE};
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowRect, GetWindowTextW, IsIconic, IsWindowVisible,
    };

    struct Collector {
        needle: String,
        found: Vec<String>,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, state: LPARAM) -> BOOL {
        // SAFETY: `state` is the `&mut Collector` passed to EnumWindows below, which
        // outlives the enumeration.
        let collector = unsafe { &mut *(state.0 as *mut Collector) };

        let mut buf = [0u16; MAX_PATH as usize];
        let len = unsafe { GetWindowTextW(hwnd, &mut buf) } as usize;
        if len == 0 {
            return TRUE;
        }
        let title = String::from_utf16_lossy(&buf[..len]);
        if !title.to_lowercase().contains(&collector.needle) {
            return TRUE;
        }

        let visible = unsafe { IsWindowVisible(hwnd) }.as_bool();
        let iconic = unsafe { IsIconic(hwnd) }.as_bool();
        let mut cloaked = 0u32;
        let cloaked_ok = unsafe {
            DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                &mut cloaked as *mut u32 as *mut c_void,
                std::mem::size_of::<u32>() as u32,
            )
        }
        .is_ok();
        let mut rect = RECT::default();
        let rect_desc = if unsafe { GetWindowRect(hwnd, &mut rect) }.is_ok() {
            format!(
                "{}x{}@{},{}",
                rect.right - rect.left,
                rect.bottom - rect.top,
                rect.left,
                rect.top
            )
        } else {
            "unknown".to_string()
        };

        collector.found.push(format!(
            "title={title:?} visible={visible} minimized={iconic} cloaked={} rect={rect_desc}",
            if cloaked_ok {
                cloaked.to_string()
            } else {
                "unknown".to_string()
            }
        ));
        TRUE
    }

    let mut collector = Collector {
        needle: title.trim().to_lowercase(),
        found: Vec::new(),
    };
    // SAFETY: `enum_proc` only dereferences the pointer we pass here, and EnumWindows
    // returns before `collector` goes out of scope.
    unsafe {
        EnumWindows(
            Some(enum_proc),
            LPARAM(&mut collector as *mut Collector as isize),
        )
        .ok()?;
    }

    Some(if collector.found.is_empty() {
        "no top-level window with a matching title exists".to_string()
    } else {
        collector.found.join(" | ")
    })
}

#[cfg(not(target_os = "windows"))]
pub fn describe_matching_windows(_title: &str) -> Option<String> {
    None
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::describe_matching_windows;

    #[test]
    fn describes_live_windows_without_matching_anything_special() {
        // An empty needle matches every titled window, which exercises the enumeration
        // callback against whatever is actually on screen.
        let all = describe_matching_windows("").expect("enumeration should succeed");
        assert!(all.contains("visible=") && all.contains("minimized="));

        let missing = describe_matching_windows("wavetrace-no-such-window-3f9a")
            .expect("enumeration should succeed");
        assert!(missing.contains("no top-level window"));
    }
}
