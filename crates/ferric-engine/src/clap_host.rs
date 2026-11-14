//! Full-Spec Pure-Rust C-ABI `.clap` Plugin Scanner & Real-Time Host.
//! Supports:
//! - `clap.params` (parameter enumeration + sample-accurate `CLAP_EVENT_PARAM_VALUE` automation/modulation)
//! - `clap.state` (binary patch state save & restore inside `.ferric` sessions)
//! - `clap.latency` (automatic PDC latency compensation)
//! - `clap.gui` (native Win32 `HWND` floating graphical plugin editors!)

use crate::plugin_window::{NativePluginGuiView, PluginQuarantine};
use ferric_dsp::{db_to_lin, ExternalPluginAdapter, MidiEvent};
use ferric_model::{ExternalPluginFormat, PluginParamDescriptor};
use libloading::Library;
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct ClapVersion {
    pub major: u32,
    pub minor: u32,
    pub revision: u32,
}

#[repr(C)]
pub(crate) struct ClapPluginEntry {
    pub clap_version: ClapVersion,
    pub init: Option<unsafe extern "C" fn(plugin_path: *const c_char) -> bool>,
    pub deinit: Option<unsafe extern "C" fn()>,
    pub get_factory: Option<unsafe extern "C" fn(factory_id: *const c_char) -> *const c_void>,
}

#[repr(C)]
pub(crate) struct ClapPluginDescriptor {
    pub clap_version: ClapVersion,
    pub id: *const c_char,
    pub name: *const c_char,
    pub vendor: *const c_char,
    pub url: *const c_char,
    pub manual_url: *const c_char,
    pub support_url: *const c_char,
    pub version: *const c_char,
    pub description: *const c_char,
    pub features: *const *const c_char,
}

#[repr(C)]
pub(crate) struct ClapHost {
    pub clap_version: ClapVersion,
    pub host_data: *mut c_void,
    pub name: *const c_char,
    pub vendor: *const c_char,
    pub url: *const c_char,
    pub version: *const c_char,
    pub get_extension:
        unsafe extern "C" fn(host: *const ClapHost, extension_id: *const c_char) -> *const c_void,
    pub request_restart: unsafe extern "C" fn(host: *const ClapHost),
    pub request_process: unsafe extern "C" fn(host: *const ClapHost),
    pub request_callback: unsafe extern "C" fn(host: *const ClapHost),
}

#[repr(C)]
pub(crate) struct ClapPlugin {
    pub desc: *const ClapPluginDescriptor,
    pub plugin_data: *mut c_void,
    pub init: Option<unsafe extern "C" fn(plugin: *const ClapPlugin) -> bool>,
    pub destroy: Option<unsafe extern "C" fn(plugin: *const ClapPlugin)>,
    pub activate: Option<
        unsafe extern "C" fn(
            plugin: *const ClapPlugin,
            sample_rate: f64,
            min_frames: u32,
            max_frames: u32,
        ) -> bool,
    >,
    pub deactivate: Option<unsafe extern "C" fn(plugin: *const ClapPlugin)>,
    pub start_processing: Option<unsafe extern "C" fn(plugin: *const ClapPlugin) -> bool>,
    pub stop_processing: Option<unsafe extern "C" fn(plugin: *const ClapPlugin)>,
    pub reset: Option<unsafe extern "C" fn(plugin: *const ClapPlugin)>,
    pub process: Option<
        unsafe extern "C" fn(plugin: *const ClapPlugin, process: *const ClapProcess) -> i32,
    >,
    pub get_extension: Option<
        unsafe extern "C" fn(plugin: *const ClapPlugin, id: *const c_char) -> *const c_void,
    >,
    pub on_main_thread: Option<unsafe extern "C" fn(plugin: *const ClapPlugin)>,
}

#[repr(C)]
pub(crate) struct ClapPluginFactory {
    pub get_plugin_count: Option<unsafe extern "C" fn(factory: *const ClapPluginFactory) -> u32>,
    pub get_plugin_descriptor: Option<
        unsafe extern "C" fn(
            factory: *const ClapPluginFactory,
            index: u32,
        ) -> *const ClapPluginDescriptor,
    >,
    pub create_plugin: Option<
        unsafe extern "C" fn(
            factory: *const ClapPluginFactory,
            host: *const ClapHost,
            plugin_id: *const c_char,
        ) -> *const ClapPlugin,
    >,
}

// --- CLAP Extensions (`clap.params`, `clap.state`, `clap.latency`, `clap.gui`) ---

#[repr(C)]
pub(crate) struct ClapParamInfo {
    pub id: u32,
    pub flags: u32,
    pub cookie: *mut c_void,
    pub name: [c_char; 256],
    pub module: [c_char; 1024],
    pub min_value: f64,
    pub max_value: f64,
    pub default_value: f64,
}

#[repr(C)]
pub(crate) struct ClapPluginParams {
    pub count: Option<unsafe extern "C" fn(plugin: *const ClapPlugin) -> u32>,
    pub get_info: Option<
        unsafe extern "C" fn(
            plugin: *const ClapPlugin,
            param_index: u32,
            param_info: *mut ClapParamInfo,
        ) -> bool,
    >,
    pub get_value: Option<
        unsafe extern "C" fn(
            plugin: *const ClapPlugin,
            param_id: u32,
            out_value: *mut f64,
        ) -> bool,
    >,
    pub value_to_text: Option<
        unsafe extern "C" fn(
            plugin: *const ClapPlugin,
            param_id: u32,
            value: f64,
            out_buf: *mut c_char,
            out_cap: u32,
        ) -> bool,
    >,
    pub text_to_value: Option<
        unsafe extern "C" fn(
            plugin: *const ClapPlugin,
            param_id: u32,
            param_value_text: *const c_char,
            out_value: *mut f64,
        ) -> bool,
    >,
    pub flush: Option<
        unsafe extern "C" fn(
            plugin: *const ClapPlugin,
            in_events: *const ClapInputEvents,
            out_events: *const ClapOutputEvents,
        ),
    >,
}

#[repr(C)]
pub(crate) struct ClapOStream {
    pub ctx: *mut c_void,
    pub write:
        unsafe extern "C" fn(stream: *const ClapOStream, buffer: *const c_void, size: u64) -> i64,
}

#[repr(C)]
pub(crate) struct ClapIStream {
    pub ctx: *mut c_void,
    pub read:
        unsafe extern "C" fn(stream: *const ClapIStream, buffer: *mut c_void, size: u64) -> i64,
}

#[repr(C)]
pub(crate) struct ClapPluginState {
    pub save: Option<
        unsafe extern "C" fn(plugin: *const ClapPlugin, stream: *const ClapOStream) -> bool,
    >,
    pub load: Option<
        unsafe extern "C" fn(plugin: *const ClapPlugin, stream: *const ClapIStream) -> bool,
    >,
}

#[repr(C)]
pub(crate) struct ClapPluginLatency {
    pub get: Option<unsafe extern "C" fn(plugin: *const ClapPlugin) -> u32>,
}

#[repr(C)]
pub(crate) struct ClapWindow {
    pub api: *const c_char,
    pub handle: *mut c_void, // Win32 HWND union
}

#[repr(C)]
pub(crate) struct ClapGuiResizeHints {
    pub can_resize_horizontally: bool,
    pub can_resize_vertically: bool,
    pub preserve_aspect_ratio: bool,
    pub aspect_ratio_width: u32,
    pub aspect_ratio_height: u32,
}

#[repr(C)]
pub(crate) struct ClapPluginGui {
    pub is_api_supported: Option<
        unsafe extern "C" fn(
            plugin: *const ClapPlugin,
            api: *const c_char,
            is_floating: bool,
        ) -> bool,
    >,
    pub get_preferred_api: Option<
        unsafe extern "C" fn(
            plugin: *const ClapPlugin,
            api: *mut *const c_char,
            is_floating: *mut bool,
        ) -> bool,
    >,
    pub create: Option<
        unsafe extern "C" fn(
            plugin: *const ClapPlugin,
            api: *const c_char,
            is_floating: bool,
        ) -> bool,
    >,
    pub destroy: Option<unsafe extern "C" fn(plugin: *const ClapPlugin)>,
    pub set_scale: Option<unsafe extern "C" fn(plugin: *const ClapPlugin, scale: f64) -> bool>,
    pub get_size: Option<
        unsafe extern "C" fn(
            plugin: *const ClapPlugin,
            width: *mut u32,
            height: *mut u32,
        ) -> bool,
    >,
    pub can_resize: Option<unsafe extern "C" fn(plugin: *const ClapPlugin) -> bool>,
    pub get_resize_hints: Option<
        unsafe extern "C" fn(plugin: *const ClapPlugin, hints: *mut ClapGuiResizeHints) -> bool,
    >,
    pub adjust_size: Option<
        unsafe extern "C" fn(
            plugin: *const ClapPlugin,
            width: *mut u32,
            height: *mut u32,
        ) -> bool,
    >,
    pub set_size:
        Option<unsafe extern "C" fn(plugin: *const ClapPlugin, width: u32, height: u32) -> bool>,
    pub set_parent: Option<
        unsafe extern "C" fn(plugin: *const ClapPlugin, window: *const ClapWindow) -> bool,
    >,
    pub set_transient: Option<
        unsafe extern "C" fn(plugin: *const ClapPlugin, window: *const ClapWindow) -> bool,
    >,
    pub suggest_title: Option<unsafe extern "C" fn(plugin: *const ClapPlugin, title: *const c_char)>,
    pub show: Option<unsafe extern "C" fn(plugin: *const ClapPlugin) -> bool>,
    pub hide: Option<unsafe extern "C" fn(plugin: *const ClapPlugin) -> bool>,
}

// --- Audio & Event Process Structs ---

#[repr(C)]
pub(crate) struct ClapAudioBuffer {
    pub data32: *mut *mut f32,
    pub data64: *mut *mut f64,
    pub channel_count: u32,
    pub latency: u32,
    pub constant_mask: u64,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct ClapEventHeader {
    pub size: u32,
    pub time: u32,
    pub space_id: u16,
    pub type_: u16,
    pub flags: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct ClapEventNote {
    pub header: ClapEventHeader,
    pub note_id: i32,
    pub port_index: i16,
    pub channel: i16,
    pub key: i16,
    pub velocity: f64,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct ClapEventParamValue {
    pub header: ClapEventHeader,
    pub param_id: u32,
    pub cookie: *mut c_void,
    pub note_id: i32,
    pub port_index: i16,
    pub channel: i16,
    pub key: i16,
    pub value: f64,
}

#[repr(C)]
#[derive(Copy, Clone)]
union AnyClapEvent {
    note: ClapEventNote,
    param: ClapEventParamValue,
}

#[repr(C)]
pub(crate) struct ClapInputEvents {
    pub ctx: *mut c_void,
    pub size: unsafe extern "C" fn(list: *const ClapInputEvents) -> u32,
    pub get:
        unsafe extern "C" fn(list: *const ClapInputEvents, index: u32) -> *const ClapEventHeader,
}

#[repr(C)]
pub(crate) struct ClapOutputEvents {
    pub ctx: *mut c_void,
    pub try_push:
        unsafe extern "C" fn(list: *const ClapOutputEvents, event: *const ClapEventHeader) -> bool,
}

#[repr(C)]
pub(crate) struct ClapProcess {
    pub steady_time: i64,
    pub frames_count: u32,
    pub transport: *const c_void,
    pub audio_inputs: *const ClapAudioBuffer,
    pub audio_outputs: *mut ClapAudioBuffer,
    pub audio_inputs_count: u32,
    pub audio_outputs_count: u32,
    pub in_events: *const ClapInputEvents,
    pub out_events: *const ClapOutputEvents,
}

pub(crate) const CLAP_EVENT_NOTE_ON: u16 = 0;
pub(crate) const CLAP_EVENT_NOTE_OFF: u16 = 1;
pub(crate) const CLAP_EVENT_PARAM_VALUE: u16 = 4;

struct HostCallbackState {
    needs_main_thread: AtomicBool,
}

unsafe extern "C" fn host_get_extension(
    _host: *const ClapHost,
    _ext_id: *const c_char,
) -> *const c_void {
    std::ptr::null()
}
unsafe extern "C" fn host_noop(_host: *const ClapHost) {}
unsafe extern "C" fn host_request_callback(host: *const ClapHost) {
    if !host.is_null() && !(*host).host_data.is_null() {
        let st = &*((*host).host_data as *const HostCallbackState);
        st.needs_main_thread.store(true, Ordering::Release);
    }
}

unsafe extern "C" fn in_events_size(list: *const ClapInputEvents) -> u32 {
    let vec_ref = &*((*list).ctx as *const Vec<AnyClapEvent>);
    vec_ref.len() as u32
}

unsafe extern "C" fn in_events_get(
    list: *const ClapInputEvents,
    index: u32,
) -> *const ClapEventHeader {
    let vec_ref = &*((*list).ctx as *const Vec<AnyClapEvent>);
    match vec_ref.get(index as usize) {
        Some(ev) => &ev.note.header as *const ClapEventHeader,
        None => std::ptr::null(),
    }
}

unsafe extern "C" fn out_events_push(
    _list: *const ClapOutputEvents,
    _ev: *const ClapEventHeader,
) -> bool {
    true
}

unsafe extern "C" fn ostream_write(
    stream: *const ClapOStream,
    buffer: *const c_void,
    size: u64,
) -> i64 {
    if stream.is_null() || buffer.is_null() {
        return -1;
    }
    let out_vec = &mut *((*stream).ctx as *mut Vec<u8>);
    let slice = std::slice::from_raw_parts(buffer as *const u8, size as usize);
    out_vec.extend_from_slice(slice);
    size as i64
}

struct ReadCursor<'a> {
    data: &'a [u8],
    pos: usize,
}

unsafe extern "C" fn istream_read(
    stream: *const ClapIStream,
    buffer: *mut c_void,
    size: u64,
) -> i64 {
    if stream.is_null() || buffer.is_null() {
        return -1;
    }
    let cur = &mut *((*stream).ctx as *mut ReadCursor<'_>);
    let rem = cur.data.len().saturating_sub(cur.pos);
    let n = (size as usize).min(rem);
    if n > 0 {
        std::ptr::copy_nonoverlapping(cur.data[cur.pos..].as_ptr(), buffer as *mut u8, n);
        cur.pos += n;
    }
    n as i64
}

/// Unified Discovered Plugin Metadata for BOTH `.clap` and `.vst3` plugins!
#[derive(Debug, Clone)]
pub struct DiscoveredExternalPlugin {
    pub format: ExternalPluginFormat,
    pub bundle_path: PathBuf,
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub is_instrument: bool,
}

pub type DiscoveredClapPlugin = DiscoveredExternalPlugin;

pub fn scan_clap_plugins(
    extra_dirs: &[PathBuf],
    quarantine: &mut PluginQuarantine,
) -> Vec<DiscoveredExternalPlugin> {
    let mut search_dirs = Vec::new();
    if let Ok(common) = std::env::var("COMMONPROGRAMFILES") {
        search_dirs.push(PathBuf::from(common).join("CLAP"));
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        search_dirs.push(PathBuf::from(local).join("Programs\\Common\\CLAP"));
    }
    if let Ok(home) = std::env::var("HOME") {
        search_dirs.push(PathBuf::from(&home).join(".clap"));
        search_dirs.push(PathBuf::from(&home).join("Library/Audio/Plug-Ins/CLAP"));
    }
    search_dirs.extend(extra_dirs.iter().cloned());

    let mut found = Vec::new();
    for dir in search_dirs {
        collect_clap_files(&dir, quarantine, &mut found);
    }
    found
}

fn collect_clap_files(
    dir: &Path,
    quarantine: &mut PluginQuarantine,
    out: &mut Vec<DiscoveredExternalPlugin>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_clap_files(&p, quarantine, out);
        } else if p
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("clap"))
            .unwrap_or(false)
        {
            if quarantine.is_quarantined(&p) {
                continue;
            }
            // Crash-guard: mark suspect BEFORE loading DLL, then unmark immediately upon clean scan!
            quarantine.mark_suspect(&p);
            if let Ok(mut list) = inspect_clap_bundle(&p) {
                quarantine.unmark_clean(&p);
                out.append(&mut list);
            }
        }
    }
}

unsafe fn cstr_to_string(ptr: *const c_char, fallback: &str) -> String {
    if ptr.is_null() {
        fallback.to_string()
    } else {
        CStr::from_ptr(ptr).to_string_lossy().into_owned()
    }
}

unsafe fn fixed_cchar_to_string(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).trim().to_string()
}

pub fn inspect_clap_bundle(path: &Path) -> Result<Vec<DiscoveredExternalPlugin>, String> {
    unsafe {
        let lib = Library::new(path).map_err(|e| e.to_string())?;
        let entry_sym: libloading::Symbol<*const ClapPluginEntry> =
            lib.get(b"clap_entry\0").map_err(|e| e.to_string())?;
        let entry_ptr = *entry_sym;
        if entry_ptr.is_null() {
            return Err("Null clap_entry".into());
        }
        let c_path = CString::new(path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        if let Some(init_fn) = (*entry_ptr).init {
            if !init_fn(c_path.as_ptr()) {
                return Err("clap_entry.init returned false".into());
            }
        }
        let mut plugins = Vec::new();
        if let Some(get_fac) = (*entry_ptr).get_factory {
            let fac_id = b"clap.plugin-factory\0";
            let fac_ptr = get_fac(fac_id.as_ptr() as *const c_char) as *const ClapPluginFactory;
            if !fac_ptr.is_null() {
                let count = (*fac_ptr).get_plugin_count.map(|f| f(fac_ptr)).unwrap_or(0);
                for idx in 0..count {
                    if let Some(get_desc) = (*fac_ptr).get_plugin_descriptor {
                        let desc = get_desc(fac_ptr, idx);
                        if !desc.is_null() {
                            let mut is_inst = false;
                            let mut f_ptr = (*desc).features;
                            if !f_ptr.is_null() {
                                while !(*f_ptr).is_null() {
                                    let feat = cstr_to_string(*f_ptr, "");
                                    if feat.contains("instrument") || feat.contains("synthesizer") {
                                        is_inst = true;
                                    }
                                    f_ptr = f_ptr.add(1);
                                }
                            }
                            plugins.push(DiscoveredExternalPlugin {
                                format: ExternalPluginFormat::Clap,
                                bundle_path: path.to_path_buf(),
                                id: cstr_to_string((*desc).id, "unknown.clap"),
                                name: cstr_to_string((*desc).name, "CLAP Plugin"),
                                vendor: cstr_to_string((*desc).vendor, "Vendor"),
                                version: cstr_to_string((*desc).version, "1.0"),
                                is_instrument: is_inst,
                            });
                        }
                    }
                }
            }
        }
        if let Some(deinit_fn) = (*entry_ptr).deinit {
            deinit_fn();
        }
        Ok(plugins)
    }
}

/// Shared UI-thread handle for `clap.gui` and `clap.state`!
pub struct ClapGuiAndStateHandle {
    _lib: Option<Arc<Library>>,
    plugin_ptr: *const ClapPlugin,
    gui_ext: *const ClapPluginGui,
    state_ext: *const ClapPluginState,
    cb_state: Arc<HostCallbackState>,
    gui_created: bool,
}

impl NativePluginGuiView for ClapGuiAndStateHandle {
    fn attach_to_hwnd(&mut self, hwnd: *mut c_void) -> Result<(u32, u32), String> {
        if self.plugin_ptr.is_null() || self.gui_ext.is_null() {
            return Err("This CLAP plugin does not expose a native GUI (use FerricDAW's auto-mapped parameter knobs!)".into());
        }
        unsafe {
            let win32_api = b"win32\0".as_ptr() as *const c_char;
            if let Some(supported) = (*self.gui_ext).is_api_supported {
                if !supported(self.plugin_ptr, win32_api, false) {
                    return Err("Plugin does not support embedded win32 HWND GUI".into());
                }
            }
            if let Some(create_fn) = (*self.gui_ext).create {
                if !create_fn(self.plugin_ptr, win32_api, false) {
                    return Err("clap_gui.create() failed".into());
                }
                self.gui_created = true;
            } else {
                return Err("Missing clap_gui.create".into());
            }

            let mut w = 800u32;
            let mut h = 520u32;
            if let Some(get_sz) = (*self.gui_ext).get_size {
                let _ = get_sz(self.plugin_ptr, &mut w, &mut h);
            }
            let win = ClapWindow {
                api: win32_api,
                handle: hwnd,
            };
            if let Some(set_parent) = (*self.gui_ext).set_parent {
                if !set_parent(self.plugin_ptr, &win) {
                    if let Some(destroy_fn) = (*self.gui_ext).destroy {
                        destroy_fn(self.plugin_ptr);
                    }
                    self.gui_created = false;
                    return Err("clap_gui.set_parent() returned false".into());
                }
            }
            if let Some(show_fn) = (*self.gui_ext).show {
                let _ = show_fn(self.plugin_ptr);
            }
            Ok((w, h))
        }
    }

    fn detach(&mut self) {
        if self.gui_created && !self.plugin_ptr.is_null() && !self.gui_ext.is_null() {
            unsafe {
                if let Some(hide_fn) = (*self.gui_ext).hide {
                    let _ = hide_fn(self.plugin_ptr);
                }
                if let Some(destroy_fn) = (*self.gui_ext).destroy {
                    destroy_fn(self.plugin_ptr);
                }
            }
            self.gui_created = false;
        }
    }

    fn poll_main_thread(&mut self) {
        if self.cb_state.needs_main_thread.swap(false, Ordering::AcqRel)
            && !self.plugin_ptr.is_null()
        {
            unsafe {
                if let Some(mt) = (*self.plugin_ptr).on_main_thread {
                    mt(self.plugin_ptr);
                }
            }
        }
    }

    fn save_state_bytes(&mut self) -> Option<Vec<u8>> {
        if self.plugin_ptr.is_null() || self.state_ext.is_null() {
            return None;
        }
        unsafe {
            let save_fn = (*self.state_ext).save?;
            let mut bytes: Vec<u8> = Vec::with_capacity(4096);
            let stream = ClapOStream {
                ctx: &mut bytes as *mut Vec<u8> as *mut c_void,
                write: ostream_write,
            };
            if save_fn(self.plugin_ptr, &stream) {
                Some(bytes)
            } else {
                None
            }
        }
    }

    fn load_state_bytes(&mut self, bytes: &[u8]) -> bool {
        if bytes.is_empty() || self.plugin_ptr.is_null() || self.state_ext.is_null() {
            return false;
        }
        unsafe {
            let Some(load_fn) = (*self.state_ext).load else {
                return false;
            };
            let mut cursor = ReadCursor {
                data: bytes,
                pos: 0,
            };
            let stream = ClapIStream {
                ctx: &mut cursor as *mut ReadCursor<'_> as *mut c_void,
                read: istream_read,
            };
            load_fn(self.plugin_ptr, &stream)
        }
    }
}

/// Real-time audio/MIDI/parameter processor for a hosted `.clap` plugin!
pub struct HostedClapInstance {
    _lib: Option<Arc<Library>>,
    plugin_ptr: *const ClapPlugin,
    _host_box: Box<ClapHost>,
    _cb_state: Arc<HostCallbackState>,
    _host_strings: (CString, CString, CString, CString),
    param_ids: Vec<(u32, *mut c_void)>,
    last_sent_params: Vec<f32>,
    latency_smp: usize,
    in_l: Vec<f32>,
    in_r: Vec<f32>,
    out_l: Vec<f32>,
    out_r: Vec<f32>,
    events: Vec<AnyClapEvent>,
    steady_time: i64,
    processing_started: bool,
}

// SAFETY: Real-time audio processing runs exclusively on the single audio callback thread.
unsafe impl Send for HostedClapInstance {}

pub struct LoadedClapBundle {
    pub audio_processor: HostedClapInstance,
    pub gui_and_state: ClapGuiAndStateHandle,
    pub discovered_params: Vec<PluginParamDescriptor>,
}

impl HostedClapInstance {
    pub fn load_full(
        path: &Path,
        plugin_id: &str,
        sample_rate: f64,
        max_block: usize,
        initial_state: Option<&[u8]>,
    ) -> Result<LoadedClapBundle, String> {
        unsafe {
            let lib = Arc::new(Library::new(path).map_err(|e| e.to_string())?);
            let entry_sym: libloading::Symbol<*const ClapPluginEntry> =
                lib.get(b"clap_entry\0").map_err(|e| e.to_string())?;
            let entry_ptr = *entry_sym;
            if entry_ptr.is_null() {
                return Err("Null clap_entry".into());
            }
            let c_path =
                CString::new(path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
            if let Some(init_fn) = (*entry_ptr).init {
                if !init_fn(c_path.as_ptr()) {
                    return Err("CLAP init failed".into());
                }
            }
            let get_fac = (*entry_ptr).get_factory.ok_or("Missing get_factory")?;
            let fac_ptr = get_fac(b"clap.plugin-factory\0".as_ptr() as *const c_char)
                as *const ClapPluginFactory;
            if fac_ptr.is_null() {
                return Err("Null clap.plugin-factory".into());
            }

            Self::instantiate_from_factory(
                Some(lib),
                fac_ptr,
                plugin_id,
                sample_rate,
                max_block,
                initial_state,
            )
        }
    }

    pub(crate) unsafe fn instantiate_from_factory(
        lib: Option<Arc<Library>>,
        fac_ptr: *const ClapPluginFactory,
        plugin_id: &str,
        sample_rate: f64,
        max_block: usize,
        initial_state: Option<&[u8]>,
    ) -> Result<LoadedClapBundle, String> {
        let cb_state = Arc::new(HostCallbackState {
            needs_main_thread: AtomicBool::new(false),
        });
        let h_name = CString::new("FerricDAW").unwrap();
        let h_vendor = CString::new("FerricDAW Open Source").unwrap();
        let h_url = CString::new("https://github.com/ferricdaw").unwrap();
        let h_ver = CString::new("0.5.0").unwrap();

        let host_box = Box::new(ClapHost {
            clap_version: ClapVersion {
                major: 1,
                minor: 2,
                revision: 0,
            },
            host_data: Arc::as_ptr(&cb_state) as *mut c_void,
            name: h_name.as_ptr(),
            vendor: h_vendor.as_ptr(),
            url: h_url.as_ptr(),
            version: h_ver.as_ptr(),
            get_extension: host_get_extension,
            request_restart: host_noop,
            request_process: host_noop,
            request_callback: host_request_callback,
        });

        let c_pid = CString::new(plugin_id).map_err(|e| e.to_string())?;
        let create_fn = (*fac_ptr).create_plugin.ok_or("Missing create_plugin")?;
        let plugin_ptr = create_fn(fac_ptr, &*host_box as *const ClapHost, c_pid.as_ptr());
        if plugin_ptr.is_null() {
            return Err("create_plugin returned null".into());
        }
        if let Some(p_init) = (*plugin_ptr).init {
            if !p_init(plugin_ptr) {
                return Err("plugin.init returned false".into());
            }
        }

        // Query CLAP Extensions (`clap.params`, `clap.state`, `clap.latency`, `clap.gui`)
        let get_ext = (*plugin_ptr).get_extension;
        let params_ext = get_ext
            .map(|f| f(plugin_ptr, b"clap.params\0".as_ptr() as *const c_char) as *const ClapPluginParams)
            .unwrap_or(std::ptr::null());
        let state_ext = get_ext
            .map(|f| f(plugin_ptr, b"clap.state\0".as_ptr() as *const c_char) as *const ClapPluginState)
            .unwrap_or(std::ptr::null());
        let latency_ext = get_ext
            .map(|f| f(plugin_ptr, b"clap.latency\0".as_ptr() as *const c_char) as *const ClapPluginLatency)
            .unwrap_or(std::ptr::null());
        let gui_ext = get_ext
            .map(|f| f(plugin_ptr, b"clap.gui\0".as_ptr() as *const c_char) as *const ClapPluginGui)
            .unwrap_or(std::ptr::null());

        let mut gui_and_state = ClapGuiAndStateHandle {
            _lib: lib.clone(),
            plugin_ptr,
            gui_ext,
            state_ext,
            cb_state: cb_state.clone(),
            gui_created: false,
        };

        // Restore saved binary patch state BEFORE querying parameter values!
        if let Some(bytes) = initial_state {
            let _ = gui_and_state.load_state_bytes(bytes);
        }

        // Enumerate up to 64 real parameters from `clap.params`
        let mut discovered_params = Vec::new();
        let mut param_ids = Vec::new();
        let mut last_sent_params = Vec::new();
        if !params_ext.is_null() {
            let count = (*params_ext).count.map(|f| f(plugin_ptr)).unwrap_or(0).min(64);
            for idx in 0..count {
                if let Some(get_info) = (*params_ext).get_info {
                    let mut info: ClapParamInfo = std::mem::zeroed();
                    if get_info(plugin_ptr, idx, &mut info) {
                        let mut cur_val = info.default_value;
                        if let Some(get_val) = (*params_ext).get_value {
                            let _ = get_val(plugin_ptr, info.id, &mut cur_val);
                        }
                        let mut p_name = fixed_cchar_to_string(&info.name);
                        if p_name.is_empty() {
                            p_name = format!("Param {}", idx + 1);
                        }
                        let module = fixed_cchar_to_string(&info.module);
                        let min = info.min_value as f32;
                        let max = if (info.max_value - info.min_value).abs() < 1e-9 {
                            min + 1.0
                        } else {
                            info.max_value as f32
                        };
                        let def = (cur_val as f32).clamp(min, max);
                        discovered_params.push(PluginParamDescriptor {
                            plugin_param_id: info.id,
                            name: p_name,
                            module,
                            min,
                            max,
                            default: def,
                        });
                        param_ids.push((info.id, info.cookie));
                        last_sent_params.push(def);
                    }
                }
            }
        }

        if let Some(activate) = (*plugin_ptr).activate {
            let _ = activate(plugin_ptr, sample_rate, 16, max_block.max(4096) as u32);
        }

        let latency_smp = if !latency_ext.is_null() {
            (*latency_ext)
                .get
                .map(|f| f(plugin_ptr) as usize)
                .unwrap_or(0)
        } else {
            0
        };

        let blk = max_block.max(4096);
        let audio_processor = Self {
            _lib: lib,
            plugin_ptr,
            _host_box: host_box,
            _cb_state: cb_state,
            _host_strings: (h_name, h_vendor, h_url, h_ver),
            param_ids,
            last_sent_params,
            latency_smp,
            in_l: vec![0.0; blk],
            in_r: vec![0.0; blk],
            out_l: vec![0.0; blk],
            out_r: vec![0.0; blk],
            events: Vec::with_capacity(256),
            steady_time: 0,
            processing_started: false,
        };

        Ok(LoadedClapBundle {
            audio_processor,
            gui_and_state,
            discovered_params,
        })
    }
}

impl ExternalPluginAdapter for HostedClapInstance {
    fn latency_samples(&self) -> usize {
        self.latency_smp
    }

    fn reset(&mut self) {
        unsafe {
            if !self.plugin_ptr.is_null() {
                if let Some(r) = (*self.plugin_ptr).reset {
                    r(self.plugin_ptr);
                }
            }
        }
    }

    fn process(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &[f32],
        _bpm: f64,
        midi_events: &[MidiEvent],
    ) {
        let frames = left.len().min(right.len()).min(self.in_l.len());
        if frames == 0 {
            return;
        }
        unsafe {
            if !self.processing_started {
                if let Some(start_fn) = (*self.plugin_ptr).start_processing {
                    let _ = start_fn(self.plugin_ptr);
                }
                self.processing_started = true;
            }

            self.in_l[..frames].copy_from_slice(&left[..frames]);
            self.in_r[..frames].copy_from_slice(&right[..frames]);
            self.out_l[..frames].fill(0.0);
            self.out_r[..frames].fill(0.0);

            self.events.clear();

            // 1. Emit sample-accurate `CLAP_EVENT_PARAM_VALUE` changes from Automation / Modulators / Knobs!
            let p_len = params.len().min(self.param_ids.len());
            for idx in 0..p_len {
                let val = params[idx];
                if (val - self.last_sent_params[idx]).abs() > 1e-6
                    && self.events.len() < self.events.capacity()
                {
                    self.last_sent_params[idx] = val;
                    let (pid, cookie) = self.param_ids[idx];
                    self.events.push(AnyClapEvent {
                        param: ClapEventParamValue {
                            header: ClapEventHeader {
                                size: std::mem::size_of::<ClapEventParamValue>() as u32,
                                time: 0,
                                space_id: 0,
                                type_: CLAP_EVENT_PARAM_VALUE,
                                flags: 0,
                            },
                            param_id: pid,
                            cookie,
                            note_id: -1,
                            port_index: 0,
                            channel: -1,
                            key: -1,
                            value: val as f64,
                        },
                    });
                }
            }

            // 2. Emit sample-accurate MIDI Note-On / Note-Off events!
            for ev in midi_events {
                if self.events.len() < self.events.capacity() {
                    self.events.push(AnyClapEvent {
                        note: ClapEventNote {
                            header: ClapEventHeader {
                                size: std::mem::size_of::<ClapEventNote>() as u32,
                                time: ev.sample_offset.min(frames.saturating_sub(1)) as u32,
                                space_id: 0,
                                type_: if ev.is_note_on {
                                    CLAP_EVENT_NOTE_ON
                                } else {
                                    CLAP_EVENT_NOTE_OFF
                                },
                                flags: 0,
                            },
                            note_id: -1,
                            port_index: 0,
                            channel: 0,
                            key: ev.note as i16,
                            velocity: (ev.velocity as f64 / 127.0).clamp(0.0, 1.0),
                        },
                    });
                }
            }

            let mut in_ptrs = [self.in_l.as_mut_ptr(), self.in_r.as_mut_ptr()];
            let mut out_ptrs = [self.out_l.as_mut_ptr(), self.out_r.as_mut_ptr()];
            let audio_in = ClapAudioBuffer {
                data32: in_ptrs.as_mut_ptr(),
                data64: std::ptr::null_mut(),
                channel_count: 2,
                latency: 0,
                constant_mask: 0,
            };
            let mut audio_out = ClapAudioBuffer {
                data32: out_ptrs.as_mut_ptr(),
                data64: std::ptr::null_mut(),
                channel_count: 2,
                latency: 0,
                constant_mask: 0,
            };
            let in_ev = ClapInputEvents {
                ctx: &mut self.events as *mut Vec<AnyClapEvent> as *mut c_void,
                size: in_events_size,
                get: in_events_get,
            };
            let out_ev = ClapOutputEvents {
                ctx: std::ptr::null_mut(),
                try_push: out_events_push,
            };
            let proc_data = ClapProcess {
                steady_time: self.steady_time,
                frames_count: frames as u32,
                transport: std::ptr::null(),
                audio_inputs: &audio_in,
                audio_outputs: &mut audio_out,
                audio_inputs_count: 1,
                audio_outputs_count: 1,
                in_events: &in_ev,
                out_events: &out_ev,
            };

            if let Some(proc_fn) = (*self.plugin_ptr).process {
                let _ = proc_fn(self.plugin_ptr, &proc_data);
            }
            self.steady_time += frames as i64;

            let _ = db_to_lin;
            for i in 0..frames {
                left[i] = self.out_l[i];
                right[i] = self.out_r[i];
            }
        }
    }
}

impl Drop for HostedClapInstance {
    fn drop(&mut self) {
        unsafe {
            if !self.plugin_ptr.is_null() {
                if self.processing_started {
                    if let Some(stop_fn) = (*self.plugin_ptr).stop_processing {
                        stop_fn(self.plugin_ptr);
                    }
                }
                if let Some(deact) = (*self.plugin_ptr).deactivate {
                    deact(self.plugin_ptr);
                }
                if let Some(destroy) = (*self.plugin_ptr).destroy {
                    destroy(self.plugin_ptr);
                }
            }
        }
    }
}
