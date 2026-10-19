//! Windows only: puts the program icon (embedded in the .exe by build.rs) on the main
//! window and its window class, so the taskbar, Alt+Tab and the title bar all show it.
//!
//! winit only sets the small window icon and leaves the class icon empty; eframe's own
//! attempt to set the big icon can silently fail, and the taskbar then falls back to the
//! generic "blank page" icon.

use raw_window_handle::{HasWindowHandle, RawWindowHandle};

const IMAGE_ICON: u32 = 1;
const LR_DEFAULTCOLOR: u32 = 0;
const WM_SETICON: u32 = 0x0080;
const ICON_SMALL: usize = 0;
const ICON_BIG: usize = 1;
const SM_CXICON: i32 = 11;
const SM_CYICON: i32 = 12;
const SM_CXSMICON: i32 = 49;
const SM_CYSMICON: i32 = 50;
const GCLP_HICON: i32 = -14;
const GCLP_HICONSM: i32 = -34;
/// Resource ID winresource gives the application icon.
const APP_ICON_ID: usize = 1;

#[link(name = "user32")]
unsafe extern "system" {
    fn LoadImageW(hinst: isize, name: *const u16, kind: u32, cx: i32, cy: i32, flags: u32)
    -> isize;
    fn SendMessageW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize;
    fn GetSystemMetrics(index: i32) -> i32;
    #[cfg(target_pointer_width = "64")]
    fn SetClassLongPtrW(hwnd: isize, index: i32, value: isize) -> isize;
    #[cfg(target_pointer_width = "32")]
    fn SetClassLongW(hwnd: isize, index: i32, value: i32) -> u32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> isize;
}

/// Loads the embedded icon in the given size (0 = system default).
fn load(cx: i32, cy: i32) -> isize {
    // SAFETY: plain Win32 calls; the resource name is an integer ID (MAKEINTRESOURCE).
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        LoadImageW(
            module,
            APP_ICON_ID as *const u16,
            IMAGE_ICON,
            cx,
            cy,
            LR_DEFAULTCOLOR,
        )
    }
}

fn set_class_icon(hwnd: isize, index: i32, icon: isize) {
    // SAFETY: hwnd is our own live window, icon a valid HICON.
    unsafe {
        #[cfg(target_pointer_width = "64")]
        SetClassLongPtrW(hwnd, index, icon);
        #[cfg(target_pointer_width = "32")]
        SetClassLongW(hwnd, index, icon as i32);
    }
}

/// Applies the program icon to the window behind `window`.
pub fn apply(window: &impl HasWindowHandle) {
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return;
    };
    let hwnd = win32.hwnd.get();

    // SAFETY: GetSystemMetrics has no side effects.
    let (big_w, big_h, small_w, small_h) = unsafe {
        (
            GetSystemMetrics(SM_CXICON),
            GetSystemMetrics(SM_CYICON),
            GetSystemMetrics(SM_CXSMICON),
            GetSystemMetrics(SM_CYSMICON),
        )
    };
    let big = load(big_w, big_h);
    let small = load(small_w, small_h);
    if big != 0 {
        set_class_icon(hwnd, GCLP_HICON, big);
        // SAFETY: hwnd is our own window, big a valid HICON.
        unsafe { SendMessageW(hwnd, WM_SETICON, ICON_BIG, big) };
    }
    if small != 0 {
        set_class_icon(hwnd, GCLP_HICONSM, small);
        // SAFETY: as above.
        unsafe { SendMessageW(hwnd, WM_SETICON, ICON_SMALL, small) };
    }
}
