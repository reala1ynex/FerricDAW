//! Pure-Rust C-ABI Open-Standard `.clap` Plugin Scanner & Real-Time Stereo/MIDI Host.
//! Dynamically discovers and hosts `.clap` audio & instrument plugins without C++ SDK dependencies.

use ferric_dsp::{db_to_lin, ExternalPluginAdapter, MidiEvent};
use libloading::Library;
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[repr(C)]
#[derive(Copy, Clone)]
struct ClapVersion {
    major: u32,
    minor: u32,
    revision: u32,
}

#[repr(C)]
struct ClapPluginEntry {
    clap_version: ClapVersion,
    init: Option<unsafe extern "C" fn(plugin_path: *const c_char) -> bool>,
    deinit: Option<unsafe extern "C" fn()>,
    get_factory: Option<unsafe extern "C" fn(factory_id: *const c_char) -> *const c_void>,
}

#[repr(C)]
struct ClapPluginDescriptor {
    clap_version: ClapVersion,
    id: *const c_char,
    name: *const c_char,
    vendor: *const c_char,
    url: *const c_char,
    manual_url: *const c_char,
    support_url: *const c_char,
    version: *const c_char,
    description: *const c_char,
    features: *const *const c_char,
}

#[repr(C)]
struct ClapHost {
    clap_version: ClapVersion,
    host_data: *mut c_void,
    name: *const c_char,
    vendor: *const c_char,
    url: *const c_char,
    version: *const c_char,
    get_extension:
        unsafe extern "C" fn(host: *const ClapHost, extension_id: *const c_char) -> *const c_void,
    request_restart: unsafe extern "C" fn(host: *const ClapHost),
    request_process: unsafe extern "C" fn(host: *const ClapHost),
    request_callback: unsafe extern "C" fn(host: *const ClapHost),
}

#[repr(C)]
struct ClapPlugin {
    desc: *const ClapPluginDescriptor,
    plugin_data: *mut c_void,
    init: Option<unsafe extern "C" fn(plugin: *const ClapPlugin) -> bool>,
    destroy: Option<unsafe extern "C" fn(plugin: *const ClapPlugin)>,
    activate: Option<
        unsafe extern "C" fn(
            plugin: *const ClapPlugin,
            sample_rate: f64,
            min_frames: u32,
            max_frames: u32,
        ) -> bool,
    >,
    deactivate: Option<unsafe extern "C" fn(plugin: *const ClapPlugin)>,
    start_processing: Option<unsafe extern "C" fn(plugin: *const ClapPlugin) -> bool>,
    stop_processing: Option<unsafe extern "C" fn(plugin: *const ClapPlugin)>,
    reset: Option<unsafe extern "C" fn(plugin: *const ClapPlugin)>,
    process: Option<
        unsafe extern "C" fn(plugin: *const ClapPlugin, process: *const ClapProcess) -> i32,
    >,
    get_extension: Option<
        unsafe extern "C" fn(plugin: *const ClapPlugin, id: *const c_char) -> *const c_void,
    >,
    on_main_thread: Option<unsafe extern "C" fn(plugin: *const ClapPlugin)>,
}

#[repr(C)]
struct ClapPluginFactory {
    get_plugin_count: Option<unsafe extern "C" fn(factory: *const ClapPluginFactory) -> u32>,
    get_plugin_descriptor: Option<
        unsafe extern "C" fn(
            factory: *const ClapPluginFactory,
            index: u32,
        ) -> *const ClapPluginDescriptor,
    >,
    create_plugin: Option<
        unsafe extern "C" fn(
            factory: *const ClapPluginFactory,
            host: *const ClapHost,
            plugin_id: *const c_char,
        ) -> *const ClapPlugin,
    >,
}

#[repr(C)]
struct ClapAudioBuffer {
    data32: *mut *mut f32,
    data64: *mut *mut f64,
    channel_count: u32,
    latency: u32,
    constant_mask: u64,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct ClapEventHeader {
    size: u32,
    time: u32,
    space_id: u16,
    type_: u16,
    flags: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct ClapEventNote {
    header: ClapEventHeader,
    note_id: i32,
    port_index: i16,
    channel: i16,
    key: i16,
    velocity: f64,
}

#[repr(C)]
struct ClapInputEvents {
    ctx: *mut c_void,
    size: unsafe extern "C" fn(list: *const ClapInputEvents) -> u32,
    get: unsafe extern "C" fn(list: *const ClapInputEvents, index: u32) -> *const ClapEventHeader,
}

#[repr(C)]
struct ClapOutputEvents {
    ctx: *mut c_void,
    try_push:
        unsafe extern "C" fn(list: *const ClapOutputEvents, event: *const ClapEventHeader) -> bool,
}

#[repr(C)]
struct ClapProcess {
    steady_time: i64,
    frames_count: u32,
    transport: *const c_void,
    audio_inputs: *const ClapAudioBuffer,
    audio_outputs: *mut ClapAudioBuffer,
    audio_inputs_count: u32,
    audio_outputs_count: u32,
    in_events: *const ClapInputEvents,
    out_events: *const ClapOutputEvents,
}

const CLAP_EVENT_NOTE_ON: u16 = 0;
const CLAP_EVENT_NOTE_OFF: u16 = 1;

unsafe extern "C" fn host_get_extension(
    _host: *const ClapHost,
    _ext_id: *const c_char,
) -> *const c_void {
    std::ptr::null()
}
unsafe extern "C" fn host_noop(_host: *const ClapHost) {}

unsafe extern "C" fn in_events_size(list: *const ClapInputEvents) -> u32 {
    let vec_ref = &*((*list).ctx as *const Vec<ClapEventNote>);
    vec_ref.len() as u32
}

unsafe extern "C" fn in_events_get(
    list: *const ClapInputEvents,
    index: u32,
) -> *const ClapEventHeader {
    let vec_ref = &*((*list).ctx as *const Vec<ClapEventNote>);
    match vec_ref.get(index as usize) {
        Some(ev) => &ev.header as *const ClapEventHeader,
        None => std::ptr::null(),
    }
}

unsafe extern "C" fn out_events_push(
    _list: *const ClapOutputEvents,
    _ev: *const ClapEventHeader,
) -> bool {
    true
}

#[derive(Debug, Clone)]
pub struct DiscoveredClapPlugin {
    pub bundle_path: PathBuf,
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
}

/// Scans standard OS CLAP folders plus any custom folder for `.clap` plugin binaries!
pub fn scan_clap_plugins(extra_dirs: &[PathBuf]) -> Vec<DiscoveredClapPlugin> {
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
        collect_clap_files(&dir, &mut found);
    }
    found
}

fn collect_clap_files(dir: &Path, out: &mut Vec<DiscoveredClapPlugin>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_clap_files(&p, out);
        } else if p
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("clap"))
            .unwrap_or(false)
        {
            if let Ok(mut list) = inspect_clap_bundle(&p) {
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

pub fn inspect_clap_bundle(path: &Path) -> Result<Vec<DiscoveredClapPlugin>, String> {
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
                            plugins.push(DiscoveredClapPlugin {
                                bundle_path: path.to_path_buf(),
                                id: cstr_to_string((*desc).id, "unknown.clap"),
                                name: cstr_to_string((*desc).name, "CLAP Plugin"),
                                vendor: cstr_to_string((*desc).vendor, "Vendor"),
                                version: cstr_to_string((*desc).version, "1.0"),
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

/// Hosted live instance of an external `.clap` plugin implementing `ExternalPluginAdapter`.
pub struct HostedClapInstance {
    _lib: Arc<Library>,
    plugin_ptr: *const ClapPlugin,
    _host_box: Box<ClapHost>,
    _host_strings: (CString, CString, CString, CString),
    in_l: Vec<f32>,
    in_r: Vec<f32>,
    out_l: Vec<f32>,
    out_r: Vec<f32>,
    note_events: Vec<ClapEventNote>,
    steady_time: i64,
    processing_started: bool,
}

// SAFETY: `HostedClapInstance` is owned exclusively by the single audio callback thread once registered.
unsafe impl Send for HostedClapInstance {}

impl HostedClapInstance {
    pub fn load(
        path: &Path,
        plugin_id: &str,
        sample_rate: f64,
        max_block: usize,
    ) -> Result<Self, String> {
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

            let h_name = CString::new("FerricDAW").unwrap();
            let h_vendor = CString::new("FerricDAW Open Source").unwrap();
            let h_url = CString::new("https://github.com/ferricdaw").unwrap();
            let h_ver = CString::new("0.2.0").unwrap();

            let host_box = Box::new(ClapHost {
                clap_version: ClapVersion {
                    major: 1,
                    minor: 2,
                    revision: 0,
                },
                host_data: std::ptr::null_mut(),
                name: h_name.as_ptr(),
                vendor: h_vendor.as_ptr(),
                url: h_url.as_ptr(),
                version: h_ver.as_ptr(),
                get_extension: host_get_extension,
                request_restart: host_noop,
                request_process: host_noop,
                request_callback: host_noop,
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
            if let Some(activate) = (*plugin_ptr).activate {
                let _ = activate(plugin_ptr, sample_rate, 16, max_block.max(4096) as u32);
            }

            let blk = max_block.max(4096);
            Ok(Self {
                _lib: lib,
                plugin_ptr,
                _host_box: host_box,
                _host_strings: (h_name, h_vendor, h_url, h_ver),
                in_l: vec![0.0; blk],
                in_r: vec![0.0; blk],
                out_l: vec![0.0; blk],
                out_r: vec![0.0; blk],
                note_events: Vec::with_capacity(128),
                steady_time: 0,
                processing_started: false,
            })
        }
    }
}

impl ExternalPluginAdapter for HostedClapInstance {
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

            self.note_events.clear();
            for ev in midi_events {
                if self.note_events.len() < self.note_events.capacity() {
                    self.note_events.push(ClapEventNote {
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
                ctx: &mut self.note_events as *mut Vec<ClapEventNote> as *mut c_void,
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

            let trim = db_to_lin(params.get(4).copied().unwrap_or(0.0));
            for i in 0..frames {
                left[i] = self.out_l[i] * trim;
                right[i] = self.out_r[i] * trim;
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
