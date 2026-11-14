//! Native Win32 Floating Plugin Editor Window (`HWND`) Manager & Crash-Quarantine Registry.
//! Hosts floating graphical editors for both `.clap` (`clap.gui`) and `.vst3` (`IPlugView`) plugins!

use ferric_model::FxId;
use std::collections::HashSet;
use std::ffi::c_void;
use std::path::{Path, PathBuf};

/// UI-thread lifecycle handle implemented by a `.clap` (`clap_plugin_gui`) or `.vst3` (`IPlugView`) editor view.
pub trait NativePluginGuiView {
    /// Attaches the plugin's native child view into `hwnd` and returns initial `(width, height)`.
    fn attach_to_hwnd(&mut self, hwnd: *mut c_void) -> Result<(u32, u32), String>;
    /// Detaches and destroys the plugin's GUI view cleanly before the `HWND` closes.
    fn detach(&mut self);
    /// Polls main-thread callbacks if requested by the plugin (`on_main_thread`).
    fn poll_main_thread(&mut self) {}
    /// Extracts current binary patch state (`clap.state` / `IBStream`) on the UI thread for `.ferric` saving!
    fn save_state_bytes(&mut self) -> Option<Vec<u8>> {
        None
    }
    /// Loads binary patch state (`clap.state` / `IBStream`) on the UI thread!
    fn load_state_bytes(&mut self, _bytes: &[u8]) -> bool {
        false
    }
}

pub struct FloatingPluginWindow {
    pub fx_id: FxId,
    pub title: String,
    pub hwnd: *mut c_void,
    pub view: Box<dyn NativePluginGuiView>,
    pub is_editor_open: bool,
}

impl FloatingPluginWindow {
    pub fn new_headless(fx_id: FxId, title: String, view: Box<dyn NativePluginGuiView>) -> Self {
        Self {
            fx_id,
            title,
            hwnd: std::ptr::null_mut(),
            view,
            is_editor_open: false,
        }
    }

    pub fn open_window(&mut self) -> Result<(), String> {
        if self.is_editor_open && !self.hwnd.is_null() {
            focus_hwnd(self.hwnd);
            pump_win32_messages();
            return Ok(());
        }
        let hwnd = create_native_host_window(&self.title, 820, 540)?;
        match self.view.attach_to_hwnd(hwnd) {
            Ok((w, h)) => {
                resize_and_show_hwnd(hwnd, w.clamp(280, 2560), h.clamp(180, 1600));
                pump_win32_messages();
                self.hwnd = hwnd;
                self.is_editor_open = true;
                Ok(())
            }
            Err(err) => {
                destroy_native_host_window(hwnd);
                pump_win32_messages();
                Err(err)
            }
        }
    }

    pub fn close_window(&mut self) {
        if self.is_editor_open {
            self.view.detach();
            pump_win32_messages();
            self.is_editor_open = false;
        }
        if !self.hwnd.is_null() {
            destroy_native_host_window(self.hwnd);
            pump_win32_messages();
            self.hwnd = std::ptr::null_mut();
        }
    }

    /// Checks if the user clicked the Win32 `X` close button on the floating window!
    pub fn poll(&mut self) {
        pump_win32_messages();
        self.view.poll_main_thread();
        if self.is_editor_open && !self.hwnd.is_null() && !is_hwnd_visible(self.hwnd) {
            self.view.detach();
            destroy_native_host_window(self.hwnd);
            pump_win32_messages();
            self.hwnd = std::ptr::null_mut();
            self.is_editor_open = false;
        }
    }
}

impl Drop for FloatingPluginWindow {
    fn drop(&mut self) {
        self.close_window();
    }
}

/// Persistent Quarantine Registry so a crashing/corrupt third-party C++ `.vst3` or `.clap` DLL
/// is automatically skipped on future scans until the user clicks "Clear Quarantine".
#[derive(Debug, Default, Clone)]
pub struct PluginQuarantine {
    quarantined: HashSet<PathBuf>,
    file_path: PathBuf,
}

impl PluginQuarantine {
    pub fn load_default() -> Self {
        let base = std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        let dir = base.join("FerricDAW");
        let _ = std::fs::create_dir_all(&dir);
        let file_path = dir.join("plugin_quarantine.txt");
        let mut quarantined = HashSet::new();
        if let Ok(txt) = std::fs::read_to_string(&file_path) {
            for line in txt.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    quarantined.insert(PathBuf::from(trimmed));
                }
            }
        }
        Self {
            quarantined,
            file_path,
        }
    }

    pub fn is_quarantined(&self, path: &Path) -> bool {
        self.quarantined.contains(path)
    }

    pub fn count(&self) -> usize {
        self.quarantined.len()
    }

    pub fn mark_suspect(&mut self, path: &Path) {
        self.quarantined.insert(path.to_path_buf());
        self.flush();
    }

    pub fn unmark_clean(&mut self, path: &Path) {
        if self.quarantined.remove(path) {
            self.flush();
        }
    }

    pub fn clear_all(&mut self) {
        self.quarantined.clear();
        let _ = std::fs::remove_file(&self.file_path);
    }

    fn flush(&self) {
        let mut out = String::new();
        for p in &self.quarantined {
            out.push_str(&p.to_string_lossy());
            out.push('\n');
        }
        let _ = std::fs::write(&self.file_path, out);
    }
}

// -------------------------------------------------------------------------
// Platform Win32 Implementation (`HWND` Floating Editor Host)
// -------------------------------------------------------------------------

#[cfg(windows)]
mod win32_impl {
    use std::ffi::c_void;
    use std::sync::Once;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{GetStockObject, BLACK_BRUSH};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AdjustWindowRectEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
        IsWindowVisible, LoadCursorW, PeekMessageW, RegisterClassW, SetForegroundWindow,
        SetWindowPos, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT,
        IDC_ARROW, MSG, PM_REMOVE, SWP_NOMOVE, SWP_NOZORDER, SW_HIDE, SW_SHOW, WM_CLOSE,
        WM_QUIT, WNDCLASSW, WS_CAPTION, WS_CLIPCHILDREN, WS_MINIMIZEBOX, WS_OVERLAPPED,
        WS_POPUPWINDOW, WS_SYSMENU,
    };

    static REGISTER_CLASS: Once = Once::new();
    const CLASS_NAME: &[u16] = &[
        b'F' as u16,
        b'e' as u16,
        b'r' as u16,
        b'r' as u16,
        b'i' as u16,
        b'c' as u16,
        b'P' as u16,
        b'l' as u16,
        b'u' as u16,
        b'g' as u16,
        b'W' as u16,
        b'n' as u16,
        b'd' as u16,
        0,
    ];

    unsafe extern "system" fn plugin_wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_CLOSE {
            // Hide first so `FloatingPluginWindow::poll()` detaches `clap.gui` / `IPlugView` safely!
            ShowWindow(hwnd, SW_HIDE);
            return 0;
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    pub fn create_window(title: &str, client_w: u32, client_h: u32) -> Result<*mut c_void, String> {
        unsafe {
            let hinstance = GetModuleHandleW(std::ptr::null());
            REGISTER_CLASS.call_once(|| {
                let wc = WNDCLASSW {
                    style: CS_HREDRAW | CS_VREDRAW,
                    lpfnWndProc: Some(plugin_wnd_proc),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: hinstance,
                    hIcon: std::ptr::null_mut(),
                    hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
                    hbrBackground: GetStockObject(BLACK_BRUSH),
                    lpszMenuName: std::ptr::null(),
                    lpszClassName: CLASS_NAME.as_ptr(),
                };
                RegisterClassW(&wc);
            });

            let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
            let mut rc = RECT {
                left: 0,
                top: 0,
                right: client_w as i32,
                bottom: client_h as i32,
            };
            AdjustWindowRectEx(&mut rc, style, 0, 0);

            let mut wide_title: Vec<u16> = title.encode_utf16().collect();
            wide_title.push(0);

            let hwnd = CreateWindowExW(
                0,
                CLASS_NAME.as_ptr(),
                wide_title.as_ptr(),
                style,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                (rc.right - rc.left).max(200),
                (rc.bottom - rc.top).max(140),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                hinstance,
                std::ptr::null(),
            );
            if hwnd.is_null() {
                let _ = WS_POPUPWINDOW;
                return Err("CreateWindowExW returned null HWND".into());
            }
            Ok(hwnd)
        }
    }

    pub fn resize_and_show(hwnd: *mut c_void, client_w: u32, client_h: u32) {
        unsafe {
            let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
            let mut rc = RECT {
                left: 0,
                top: 0,
                right: client_w as i32,
                bottom: client_h as i32,
            };
            AdjustWindowRectEx(&mut rc, style, 0, 0);
            SetWindowPos(
                hwnd as HWND,
                std::ptr::null_mut(),
                0,
                0,
                (rc.right - rc.left).max(200),
                (rc.bottom - rc.top).max(140),
                SWP_NOMOVE | SWP_NOZORDER,
            );
            ShowWindow(hwnd as HWND, SW_SHOW);
            SetForegroundWindow(hwnd as HWND);
        }
    }

    pub fn focus(hwnd: *mut c_void) {
        unsafe {
            ShowWindow(hwnd as HWND, SW_SHOW);
            SetForegroundWindow(hwnd as HWND);
        }
    }

    pub fn is_visible(hwnd: *mut c_void) -> bool {
        unsafe { IsWindowVisible(hwnd as HWND) != 0 }
    }

    pub fn destroy(hwnd: *mut c_void) {
        unsafe {
            if !hwnd.is_null() {
                DestroyWindow(hwnd as HWND);
            }
        }
    }

    pub fn pump_messages() {
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            let mut guard = 0usize;
            while guard < 64 && PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0
            {
                if msg.message == WM_QUIT {
                    break;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
                guard += 1;
            }
        }
    }
}

#[cfg(windows)]
fn create_native_host_window(title: &str, w: u32, h: u32) -> Result<*mut c_void, String> {
    win32_impl::create_window(title, w, h)
}
#[cfg(windows)]
fn resize_and_show_hwnd(hwnd: *mut c_void, w: u32, h: u32) {
    win32_impl::resize_and_show(hwnd, w, h);
}
#[cfg(windows)]
fn focus_hwnd(hwnd: *mut c_void) {
    win32_impl::focus(hwnd);
}
#[cfg(windows)]
fn is_hwnd_visible(hwnd: *mut c_void) -> bool {
    win32_impl::is_visible(hwnd)
}
#[cfg(windows)]
fn destroy_native_host_window(hwnd: *mut c_void) {
    win32_impl::destroy(hwnd);
}
#[cfg(windows)]
pub fn pump_win32_messages() {
    win32_impl::pump_messages();
}

#[cfg(not(windows))]
fn create_native_host_window(_title: &str, _w: u32, _h: u32) -> Result<*mut c_void, String> {
    Err("Native HWND hosting requires Windows".into())
}
#[cfg(not(windows))]
fn resize_and_show_hwnd(_hwnd: *mut c_void, _w: u32, _h: u32) {}
#[cfg(not(windows))]
fn focus_hwnd(_hwnd: *mut c_void) {}
#[cfg(not(windows))]
fn is_hwnd_visible(_hwnd: *mut c_void) -> bool {
    false
}
#[cfg(not(windows))]
fn destroy_native_host_window(_hwnd: *mut c_void) {}
#[cfg(not(windows))]
pub fn pump_win32_messages() {}
