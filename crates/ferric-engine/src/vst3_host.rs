//! Pure-Rust Zero-SDK COM C-ABI `.vst3` Plugin Scanner & Real-Time Audio/MIDI/Parameter/GUI Host!
//! Implements Steinberg VST3 COM vtables directly (`IPluginFactory`, `IComponent`, `IAudioProcessor`,
//! `IEditController`, `IParameterChanges`, `IEventList`, `IBStream` state & `IPlugView` Win32 `HWND` GUI).

use crate::clap_host::DiscoveredExternalPlugin;
use crate::plugin_window::{NativePluginGuiView, PluginQuarantine};
use ferric_dsp::{ExternalPluginAdapter, MidiEvent};
use ferric_model::{ExternalPluginFormat, PluginParamDescriptor};
use libloading::Library;
use std::ffi::{c_char, c_void, CStr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

pub type TResult = i32;
pub const K_RESULT_OK: TResult = 0;
pub const K_NO_INTERFACE: TResult = -2147467262; // 0x80004002

pub type TUID = [u8; 16];

/// Helper matching Steinberg `INLINE_UID(l1, l2, l3, l4)` on Windows COM layout!
pub const fn vst3_com_iid(l1: u32, l2: u32, l3: u32, l4: u32) -> TUID {
    let b1 = l1.to_le_bytes(); // Data1: u32 LE
    let b2 = l2.to_be_bytes(); // Data2 (u16) + Data3 (u16)
    let b3 = l3.to_be_bytes(); // Data4[0..4]
    let b4 = l4.to_be_bytes(); // Data4[4..8]
    [
        b1[0], b1[1], b1[2], b1[3],
        b2[0], b2[1],
        b2[2], b2[3],
        b3[0], b3[1], b3[2], b3[3],
        b4[0], b4[1], b4[2], b4[3],
    ]
}

// Note: Steinberg's Windows `INLINE_UID` macro stores `Data1` as u32 LE, `Data2` as u16 LE, `Data3` as u16 LE!
pub const fn steinberg_win_iid(l1: u32, l2: u32, l3: u32, l4: u32) -> TUID {
    let d1 = l1.to_le_bytes();
    let d2 = ((l2 >> 16) as u16).to_le_bytes();
    let d3 = ((l2 & 0xFFFF) as u16).to_le_bytes();
    let d4a = l3.to_be_bytes();
    let d4b = l4.to_be_bytes();
    [
        d1[0], d1[1], d1[2], d1[3],
        d2[0], d2[1],
        d3[0], d3[1],
        d4a[0], d4a[1], d4a[2], d4a[3],
        d4b[0], d4b[1], d4b[2], d4b[3],
    ]
}

// Standard Steinberg VST 3.7+ Interface IIDs (Windows COM layout)
pub const IID_FUNKNOWN: TUID = steinberg_win_iid(0x00000000, 0x00000000, 0xC0000000, 0x00000046);
pub const IID_ICOMPONENT: TUID = steinberg_win_iid(0xE831FF31, 0xF2D54301, 0x928EBBEE, 0x25697802);
pub const IID_IAUDIOPROCESSOR: TUID = steinberg_win_iid(0x42043F99, 0xB7DA453C, 0xA569E79D, 0x9AAEC33D);
pub const IID_IEDITCONTROLLER: TUID = steinberg_win_iid(0xDCD7BBE3, 0x7742448D, 0xA874AACC, 0x979C759E);
pub const IID_IHOSTAPPLICATION: TUID = steinberg_win_iid(0x58E595CC, 0xDB2D4969, 0x8B6AAF8C, 0x36A664E5);
pub const IID_IBSTREAM: TUID = steinberg_win_iid(0xC3BF6EA2, 0x30994752, 0x9B6BF990, 0x1EE33E9B);
pub const IID_IPARAMETERCHANGES: TUID = steinberg_win_iid(0xA4779663, 0x0BB64A56, 0xB44384A8, 0x466FEB9D);
pub const IID_IPARAMVALUEQUEUE: TUID = steinberg_win_iid(0x01263A18, 0xED074F6F, 0x98C9D356, 0x4686F9BA);
pub const IID_IEVENTLIST: TUID = steinberg_win_iid(0x3A2C4214, 0x346349FE, 0xB2C4F397, 0xB9695A44);

// --- Core COM VTables ---

#[repr(C)]
pub(crate) struct FUnknownVtbl {
    pub query_interface:
        unsafe extern "system" fn(this: *mut c_void, iid: *const TUID, obj: *mut *mut c_void) -> TResult,
    pub add_ref: unsafe extern "system" fn(this: *mut c_void) -> u32,
    pub release: unsafe extern "system" fn(this: *mut c_void) -> u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct PFactoryInfo {
    pub vendor: [c_char; 64],
    pub url: [c_char; 256],
    pub email: [c_char; 128],
    pub flags: i32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct PClassInfo {
    pub cid: TUID,
    pub cardinality: i32,
    pub category: [c_char; 32],
    pub name: [c_char; 64],
}

#[repr(C)]
pub(crate) struct IPluginFactoryVtbl {
    pub unknown: FUnknownVtbl,
    pub get_factory_info:
        unsafe extern "system" fn(this: *mut c_void, info: *mut PFactoryInfo) -> TResult,
    pub count_classes: unsafe extern "system" fn(this: *mut c_void) -> i32,
    pub get_class_info:
        unsafe extern "system" fn(this: *mut c_void, index: i32, info: *mut PClassInfo) -> TResult,
    pub create_instance: unsafe extern "system" fn(
        this: *mut c_void,
        cid: *const TUID,
        iid: *const TUID,
        obj: *mut *mut c_void,
    ) -> TResult,
}

#[repr(C)]
pub(crate) struct IPluginBaseVtbl {
    pub unknown: FUnknownVtbl,
    pub initialize:
        unsafe extern "system" fn(this: *mut c_void, context: *mut c_void) -> TResult,
    pub terminate: unsafe extern "system" fn(this: *mut c_void) -> TResult,
}

#[repr(C)]
pub(crate) struct BusInfo {
    pub media_type: i32,
    pub direction: i32,
    pub channel_count: i32,
    pub name: [u16; 128],
    pub bus_type: i32,
    pub flags: u32,
}

#[repr(C)]
pub(crate) struct IComponentVtbl {
    pub base: IPluginBaseVtbl,
    pub get_controller_class_id:
        unsafe extern "system" fn(this: *mut c_void, class_id: *mut TUID) -> TResult,
    pub set_io_mode: unsafe extern "system" fn(this: *mut c_void, mode: i32) -> TResult,
    pub get_bus_count:
        unsafe extern "system" fn(this: *mut c_void, media_type: i32, dir: i32) -> i32,
    pub get_bus_info: unsafe extern "system" fn(
        this: *mut c_void,
        media_type: i32,
        dir: i32,
        index: i32,
        bus: *mut BusInfo,
    ) -> TResult,
    pub get_routing_info: unsafe extern "system" fn(
        this: *mut c_void,
        in_info: *mut c_void,
        out_info: *mut c_void,
    ) -> TResult,
    pub activate_bus: unsafe extern "system" fn(
        this: *mut c_void,
        media_type: i32,
        dir: i32,
        index: i32,
        state: u8,
    ) -> TResult,
    pub set_active: unsafe extern "system" fn(this: *mut c_void, state: u8) -> TResult,
    pub set_state:
        unsafe extern "system" fn(this: *mut c_void, state: *mut c_void) -> TResult,
    pub get_state:
        unsafe extern "system" fn(this: *mut c_void, state: *mut c_void) -> TResult,
}

#[repr(C)]
pub(crate) struct ProcessSetup {
    pub process_mode: i32,        // 0 = kRealtime
    pub symbolic_sample_size: i32, // 0 = kSample32
    pub max_samples_per_block: i32,
    pub sample_rate: f64,
}

#[repr(C)]
pub(crate) struct AudioBusBuffers {
    pub num_channels: i32,
    pub silence_flags: u64,
    pub channel_buffers32: *mut *mut f32,
}

#[repr(C)]
pub(crate) struct ProcessData {
    pub process_mode: i32,
    pub symbolic_sample_size: i32,
    pub num_samples: i32,
    pub num_inputs: i32,
    pub num_outputs: i32,
    pub inputs: *mut AudioBusBuffers,
    pub outputs: *mut AudioBusBuffers,
    pub input_param_changes: *mut c_void,
    pub output_param_changes: *mut c_void,
    pub input_events: *mut c_void,
    pub output_events: *mut c_void,
    pub process_context: *mut c_void,
}

#[repr(C)]
pub(crate) struct IAudioProcessorVtbl {
    pub unknown: FUnknownVtbl,
    pub set_bus_arrangements: unsafe extern "system" fn(
        this: *mut c_void,
        inputs: *mut u64,
        num_ins: i32,
        outputs: *mut u64,
        num_outs: i32,
    ) -> TResult,
    pub get_bus_arrangement: unsafe extern "system" fn(
        this: *mut c_void,
        dir: i32,
        index: i32,
        arr: *mut u64,
    ) -> TResult,
    pub can_process_sample_size:
        unsafe extern "system" fn(this: *mut c_void, symbolic_sample_size: i32) -> TResult,
    pub get_latency_samples: unsafe extern "system" fn(this: *mut c_void) -> u32,
    pub setup_processing:
        unsafe extern "system" fn(this: *mut c_void, setup: *mut ProcessSetup) -> TResult,
    pub set_processing: unsafe extern "system" fn(this: *mut c_void, state: u8) -> TResult,
    pub process:
        unsafe extern "system" fn(this: *mut c_void, data: *mut ProcessData) -> TResult,
    pub get_tail_samples: unsafe extern "system" fn(this: *mut c_void) -> u32,
}

#[repr(C)]
pub(crate) struct ParameterInfo {
    pub id: u32,
    pub title: [u16; 128],
    pub short_title: [u16; 128],
    pub units: [u16; 128],
    pub step_count: i32,
    pub default_normalized_value: f64,
    pub unit_id: i32,
    pub flags: i32,
}

#[repr(C)]
pub(crate) struct IEditControllerVtbl {
    pub base: IPluginBaseVtbl,
    pub set_component_state:
        unsafe extern "system" fn(this: *mut c_void, state: *mut c_void) -> TResult,
    pub set_state:
        unsafe extern "system" fn(this: *mut c_void, state: *mut c_void) -> TResult,
    pub get_state:
        unsafe extern "system" fn(this: *mut c_void, state: *mut c_void) -> TResult,
    pub get_parameter_count: unsafe extern "system" fn(this: *mut c_void) -> i32,
    pub get_parameter_info: unsafe extern "system" fn(
        this: *mut c_void,
        param_index: i32,
        info: *mut ParameterInfo,
    ) -> TResult,
    pub get_param_string_by_value: unsafe extern "system" fn(
        this: *mut c_void,
        id: u32,
        value_normalized: f64,
        string: *mut [u16; 128],
    ) -> TResult,
    pub get_param_value_by_string: unsafe extern "system" fn(
        this: *mut c_void,
        id: u32,
        string: *const u16,
        value_normalized: *mut f64,
    ) -> TResult,
    pub normalized_param_to_plain:
        unsafe extern "system" fn(this: *mut c_void, id: u32, value_normalized: f64) -> f64,
    pub plain_param_to_normalized:
        unsafe extern "system" fn(this: *mut c_void, id: u32, plain_value: f64) -> f64,
    pub get_param_normalized: unsafe extern "system" fn(this: *mut c_void, id: u32) -> f64,
    pub set_param_normalized:
        unsafe extern "system" fn(this: *mut c_void, id: u32, value: f64) -> TResult,
    pub set_component_handler:
        unsafe extern "system" fn(this: *mut c_void, handler: *mut c_void) -> TResult,
    pub create_view:
        unsafe extern "system" fn(this: *mut c_void, name: *const c_char) -> *mut c_void,
}

#[repr(C)]
#[derive(Default)]
pub(crate) struct ViewRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
pub(crate) struct IPlugViewVtbl {
    pub unknown: FUnknownVtbl,
    pub is_platform_type_supported:
        unsafe extern "system" fn(this: *mut c_void, type_str: *const c_char) -> TResult,
    pub attached: unsafe extern "system" fn(
        this: *mut c_void,
        parent: *mut c_void,
        type_str: *const c_char,
    ) -> TResult,
    pub removed: unsafe extern "system" fn(this: *mut c_void) -> TResult,
    pub on_wheel: unsafe extern "system" fn(this: *mut c_void, distance: f32) -> TResult,
    pub on_key_down:
        unsafe extern "system" fn(this: *mut c_void, key: u16, key_code: i16, modifiers: i16) -> TResult,
    pub on_key_up:
        unsafe extern "system" fn(this: *mut c_void, key: u16, key_code: i16, modifiers: i16) -> TResult,
    pub get_size: unsafe extern "system" fn(this: *mut c_void, size: *mut ViewRect) -> TResult,
    pub on_size: unsafe extern "system" fn(this: *mut c_void, new_size: *mut ViewRect) -> TResult,
    pub on_focus: unsafe extern "system" fn(this: *mut c_void, state: u8) -> TResult,
    pub set_frame: unsafe extern "system" fn(this: *mut c_void, frame: *mut c_void) -> TResult,
    pub can_resize: unsafe extern "system" fn(this: *mut c_void) -> TResult,
    pub check_size_constraint:
        unsafe extern "system" fn(this: *mut c_void, rect: *mut ViewRect) -> TResult,
}

// --- Host-Provided COM Objects (`IHostApplication`, `IBStream`, `IParameterChanges`, `IEventList`) ---

#[repr(C)]
struct HostAppVtbl {
    unknown: FUnknownVtbl,
    get_name: unsafe extern "system" fn(this: *mut c_void, name: *mut [u16; 128]) -> TResult,
    create_instance: unsafe extern "system" fn(
        this: *mut c_void,
        cid: *const TUID,
        iid: *const TUID,
        obj: *mut *mut c_void,
    ) -> TResult,
}

#[repr(C)]
struct HostAppCom {
    vtbl: *const HostAppVtbl,
}

unsafe extern "system" fn host_app_qi(
    this: *mut c_void,
    iid: *const TUID,
    obj: *mut *mut c_void,
) -> TResult {
    if !iid.is_null() && (*iid == IID_FUNKNOWN || *iid == IID_IHOSTAPPLICATION) {
        *obj = this;
        return K_RESULT_OK;
    }
    *obj = std::ptr::null_mut();
    K_NO_INTERFACE
}
unsafe extern "system" fn host_app_ref(_this: *mut c_void) -> u32 {
    1
}
unsafe extern "system" fn host_app_get_name(
    _this: *mut c_void,
    name: *mut [u16; 128],
) -> TResult {
    if !name.is_null() {
        let s = "FerricDAW";
        let dst = &mut *name;
        dst.fill(0);
        for (i, ch) in s.encode_utf16().take(127).enumerate() {
            dst[i] = ch;
        }
    }
    K_RESULT_OK
}
unsafe extern "system" fn host_app_create_instance(
    _this: *mut c_void,
    _cid: *const TUID,
    _iid: *const TUID,
    obj: *mut *mut c_void,
) -> TResult {
    if !obj.is_null() {
        *obj = std::ptr::null_mut();
    }
    K_NO_INTERFACE
}

static HOST_APP_VTBL: HostAppVtbl = HostAppVtbl {
    unknown: FUnknownVtbl {
        query_interface: host_app_qi,
        add_ref: host_app_ref,
        release: host_app_ref,
    },
    get_name: host_app_get_name,
    create_instance: host_app_create_instance,
};

// --- `IBStream` In-Memory COM Implementation for `IComponent::getState` / `setState` ---

#[repr(C)]
struct IBStreamVtbl {
    unknown: FUnknownVtbl,
    read: unsafe extern "system" fn(
        this: *mut c_void,
        buffer: *mut c_void,
        num_bytes: i32,
        num_bytes_read: *mut i32,
    ) -> TResult,
    write: unsafe extern "system" fn(
        this: *mut c_void,
        buffer: *mut c_void,
        num_bytes: i32,
        num_bytes_written: *mut i32,
    ) -> TResult,
    seek: unsafe extern "system" fn(
        this: *mut c_void,
        pos: i64,
        mode: i32,
        result: *mut i64,
    ) -> TResult,
    tell: unsafe extern "system" fn(this: *mut c_void, pos: *mut i64) -> TResult,
}

#[repr(C)]
struct MemoryBStream {
    vtbl: *const IBStreamVtbl,
    data: Vec<u8>,
    cursor: usize,
}

unsafe extern "system" fn bstream_qi(
    this: *mut c_void,
    iid: *const TUID,
    obj: *mut *mut c_void,
) -> TResult {
    if !iid.is_null() && (*iid == IID_FUNKNOWN || *iid == IID_IBSTREAM) {
        *obj = this;
        return K_RESULT_OK;
    }
    *obj = std::ptr::null_mut();
    K_NO_INTERFACE
}

unsafe extern "system" fn bstream_read(
    this: *mut c_void,
    buffer: *mut c_void,
    num_bytes: i32,
    num_bytes_read: *mut i32,
) -> TResult {
    let s = &mut *(this as *mut MemoryBStream);
    let rem = s.data.len().saturating_sub(s.cursor);
    let n = (num_bytes.max(0) as usize).min(rem);
    if n > 0 && !buffer.is_null() {
        std::ptr::copy_nonoverlapping(s.data[s.cursor..].as_ptr(), buffer as *mut u8, n);
        s.cursor += n;
    }
    if !num_bytes_read.is_null() {
        *num_bytes_read = n as i32;
    }
    K_RESULT_OK
}

unsafe extern "system" fn bstream_write(
    this: *mut c_void,
    buffer: *mut c_void,
    num_bytes: i32,
    num_bytes_written: *mut i32,
) -> TResult {
    let s = &mut *(this as *mut MemoryBStream);
    let n = num_bytes.max(0) as usize;
    if n > 0 && !buffer.is_null() {
        let src = std::slice::from_raw_parts(buffer as *const u8, n);
        if s.cursor + n > s.data.len() {
            s.data.resize(s.cursor + n, 0);
        }
        s.data[s.cursor..s.cursor + n].copy_from_slice(src);
        s.cursor += n;
    }
    if !num_bytes_written.is_null() {
        *num_bytes_written = n as i32;
    }
    K_RESULT_OK
}

unsafe extern "system" fn bstream_seek(
    this: *mut c_void,
    pos: i64,
    mode: i32,
    result: *mut i64,
) -> TResult {
    let s = &mut *(this as *mut MemoryBStream);
    let base: i64 = match mode {
        1 => s.cursor as i64,      // kIBSeekCur
        2 => s.data.len() as i64,  // kIBSeekEnd
        _ => 0,                    // kIBSeekSet
    };
    s.cursor = (base + pos).max(0) as usize;
    if !result.is_null() {
        *result = s.cursor as i64;
    }
    K_RESULT_OK
}

unsafe extern "system" fn bstream_tell(this: *mut c_void, pos: *mut i64) -> TResult {
    let s = &*(this as *const MemoryBStream);
    if !pos.is_null() {
        *pos = s.cursor as i64;
    }
    K_RESULT_OK
}

static BSTREAM_VTBL: IBStreamVtbl = IBStreamVtbl {
    unknown: FUnknownVtbl {
        query_interface: bstream_qi,
        add_ref: host_app_ref,
        release: host_app_ref,
    },
    read: bstream_read,
    write: bstream_write,
    seek: bstream_seek,
    tell: bstream_tell,
};

// --- `IParameterChanges` & `IParamValueQueue` Zero-Alloc COM Implementation ---

#[repr(C)]
struct IParamValueQueueVtbl {
    unknown: FUnknownVtbl,
    get_parameter_id: unsafe extern "system" fn(this: *mut c_void) -> u32,
    get_point_count: unsafe extern "system" fn(this: *mut c_void) -> i32,
    get_point: unsafe extern "system" fn(
        this: *mut c_void,
        index: i32,
        sample_offset: *mut i32,
        value: *mut f64,
    ) -> TResult,
    add_point: unsafe extern "system" fn(
        this: *mut c_void,
        sample_offset: i32,
        value: f64,
        index: *mut i32,
    ) -> TResult,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct HostParamQueue {
    vtbl: *const IParamValueQueueVtbl,
    param_id: u32,
    value: f64,
}

unsafe extern "system" fn pvq_qi(
    this: *mut c_void,
    iid: *const TUID,
    obj: *mut *mut c_void,
) -> TResult {
    if !iid.is_null() && (*iid == IID_FUNKNOWN || *iid == IID_IPARAMVALUEQUEUE) {
        *obj = this;
        return K_RESULT_OK;
    }
    *obj = std::ptr::null_mut();
    K_NO_INTERFACE
}
unsafe extern "system" fn pvq_get_id(this: *mut c_void) -> u32 {
    (*(this as *const HostParamQueue)).param_id
}
unsafe extern "system" fn pvq_get_count(_this: *mut c_void) -> i32 {
    1
}
unsafe extern "system" fn pvq_get_point(
    this: *mut c_void,
    index: i32,
    sample_offset: *mut i32,
    value: *mut f64,
) -> TResult {
    if index != 0 {
        return -1;
    }
    let q = &*(this as *const HostParamQueue);
    if !sample_offset.is_null() {
        *sample_offset = 0;
    }
    if !value.is_null() {
        *value = q.value;
    }
    K_RESULT_OK
}
unsafe extern "system" fn pvq_add_point(
    _this: *mut c_void,
    _offset: i32,
    _val: f64,
    idx: *mut i32,
) -> TResult {
    if !idx.is_null() {
        *idx = 0;
    }
    K_RESULT_OK
}

static PVQ_VTBL: IParamValueQueueVtbl = IParamValueQueueVtbl {
    unknown: FUnknownVtbl {
        query_interface: pvq_qi,
        add_ref: host_app_ref,
        release: host_app_ref,
    },
    get_parameter_id: pvq_get_id,
    get_point_count: pvq_get_count,
    get_point: pvq_get_point,
    add_point: pvq_add_point,
};

#[repr(C)]
struct IParameterChangesVtbl {
    unknown: FUnknownVtbl,
    get_parameter_count: unsafe extern "system" fn(this: *mut c_void) -> i32,
    get_parameter_data: unsafe extern "system" fn(this: *mut c_void, index: i32) -> *mut c_void,
    add_parameter_data: unsafe extern "system" fn(
        this: *mut c_void,
        id: *const u32,
        index: *mut i32,
    ) -> *mut c_void,
}

#[repr(C)]
struct HostParamChanges {
    vtbl: *const IParameterChangesVtbl,
    queues: [HostParamQueue; 64],
    count: usize,
}

unsafe extern "system" fn pc_qi(
    this: *mut c_void,
    iid: *const TUID,
    obj: *mut *mut c_void,
) -> TResult {
    if !iid.is_null() && (*iid == IID_FUNKNOWN || *iid == IID_IPARAMETERCHANGES) {
        *obj = this;
        return K_RESULT_OK;
    }
    *obj = std::ptr::null_mut();
    K_NO_INTERFACE
}
unsafe extern "system" fn pc_count(this: *mut c_void) -> i32 {
    (*(this as *const HostParamChanges)).count as i32
}
unsafe extern "system" fn pc_get_data(this: *mut c_void, index: i32) -> *mut c_void {
    let pc = &mut *(this as *mut HostParamChanges);
    if index >= 0 && (index as usize) < pc.count {
        &mut pc.queues[index as usize] as *mut HostParamQueue as *mut c_void
    } else {
        std::ptr::null_mut()
    }
}
unsafe extern "system" fn pc_add_data(
    _this: *mut c_void,
    _id: *const u32,
    _idx: *mut i32,
) -> *mut c_void {
    std::ptr::null_mut()
}

static PC_VTBL: IParameterChangesVtbl = IParameterChangesVtbl {
    unknown: FUnknownVtbl {
        query_interface: pc_qi,
        add_ref: host_app_ref,
        release: host_app_ref,
    },
    get_parameter_count: pc_count,
    get_parameter_data: pc_get_data,
    add_parameter_data: pc_add_data,
};

// --- `IEventList` Zero-Alloc COM Implementation for VST3 MIDI Notes ---

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct Vst3NoteOnEvent {
    pub channel: i16,
    pub pitch: i16,
    pub tuning: f32,
    pub velocity: f32,
    pub length: i32,
    pub note_id: i32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct Vst3NoteOffEvent {
    pub channel: i16,
    pub pitch: i16,
    pub velocity: f32,
    pub note_id: i32,
    pub tuning: f32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) union Vst3EventData {
    pub note_on: Vst3NoteOnEvent,
    pub note_off: Vst3NoteOffEvent,
    pub pad: [u8; 32],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct Vst3Event {
    pub bus_index: i32,
    pub sample_offset: i32,
    pub ppq_position: f64,
    pub flags: u16,
    pub event_type: u16, // 0 = kNoteOnEvent, 1 = kNoteOffEvent
    pub data: Vst3EventData,
}

#[repr(C)]
struct IEventListVtbl {
    unknown: FUnknownVtbl,
    get_event_count: unsafe extern "system" fn(this: *mut c_void) -> i32,
    get_event:
        unsafe extern "system" fn(this: *mut c_void, index: i32, e: *mut Vst3Event) -> TResult,
    add_event: unsafe extern "system" fn(this: *mut c_void, e: *mut Vst3Event) -> TResult,
}

#[repr(C)]
struct HostEventList {
    vtbl: *const IEventListVtbl,
    events: [Vst3Event; 128],
    count: usize,
}

unsafe extern "system" fn ev_qi(
    this: *mut c_void,
    iid: *const TUID,
    obj: *mut *mut c_void,
) -> TResult {
    if !iid.is_null() && (*iid == IID_FUNKNOWN || *iid == IID_IEVENTLIST) {
        *obj = this;
        return K_RESULT_OK;
    }
    *obj = std::ptr::null_mut();
    K_NO_INTERFACE
}
unsafe extern "system" fn ev_count(this: *mut c_void) -> i32 {
    (*(this as *const HostEventList)).count as i32
}
unsafe extern "system" fn ev_get(this: *mut c_void, index: i32, e: *mut Vst3Event) -> TResult {
    let list = &*(this as *const HostEventList);
    if index >= 0 && (index as usize) < list.count && !e.is_null() {
        *e = list.events[index as usize];
        K_RESULT_OK
    } else {
        -1
    }
}
unsafe extern "system" fn ev_add(_this: *mut c_void, _e: *mut Vst3Event) -> TResult {
    K_RESULT_OK
}

static EV_VTBL: IEventListVtbl = IEventListVtbl {
    unknown: FUnknownVtbl {
        query_interface: ev_qi,
        add_ref: host_app_ref,
        release: host_app_ref,
    },
    get_event_count: ev_count,
    get_event: ev_get,
    add_event: ev_add,
};

// --- VST3 Bundle Discovery & Hex TUID Encoding ---

pub fn encode_tuid_hex(tuid: &TUID) -> String {
    let mut s = String::with_capacity(32);
    for b in tuid {
        s.push_str(&format!("{:02X}", b));
    }
    s
}

pub fn decode_tuid_hex(hex: &str) -> Option<TUID> {
    if hex.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for i in 0..16 {
        out[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

unsafe fn fixed_cchar_str(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).trim().to_string()
}

fn utf16_buf_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len]).trim().to_string()
}

/// Resolves either a flat `.vst3` DLL or a modern VST3 bundle folder (`MyPlugin.vst3/Contents/x86_64-win/MyPlugin.vst3`).
pub fn resolve_vst3_binary_path(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }
    if path.is_dir() {
        let win64 = path.join("Contents").join("x86_64-win");
        if let Ok(entries) = std::fs::read_dir(&win64) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_file()
                    && p.extension()
                        .and_then(|x| x.to_str())
                        .map(|x| x.eq_ignore_ascii_case("vst3"))
                        .unwrap_or(false)
                {
                    return Some(p);
                }
            }
        }
    }
    None
}

pub fn scan_vst3_plugins(
    extra_dirs: &[PathBuf],
    quarantine: &mut PluginQuarantine,
) -> Vec<DiscoveredExternalPlugin> {
    let mut search_dirs = Vec::new();
    if let Ok(common) = std::env::var("COMMONPROGRAMFILES") {
        search_dirs.push(PathBuf::from(common).join("VST3"));
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        search_dirs.push(PathBuf::from(local).join("Programs\\Common\\VST3"));
    }
    search_dirs.extend(extra_dirs.iter().cloned());

    let mut found = Vec::new();
    for dir in search_dirs {
        collect_vst3_entries(&dir, quarantine, &mut found);
    }
    found
}

fn collect_vst3_entries(
    dir: &Path,
    quarantine: &mut PluginQuarantine,
    out: &mut Vec<DiscoveredExternalPlugin>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let is_vst3_ext = p
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("vst3"))
            .unwrap_or(false);

        if is_vst3_ext {
            if let Some(dll_path) = resolve_vst3_binary_path(&p) {
                if quarantine.is_quarantined(&dll_path) {
                    continue;
                }
                quarantine.mark_suspect(&dll_path);
                if let Ok(mut list) = inspect_vst3_binary(&dll_path) {
                    quarantine.unmark_clean(&dll_path);
                    out.append(&mut list);
                }
            }
        } else if p.is_dir() {
            collect_vst3_entries(&p, quarantine, out);
        }
    }
}

pub fn inspect_vst3_binary(dll_path: &Path) -> Result<Vec<DiscoveredExternalPlugin>, String> {
    unsafe {
        let lib = Library::new(dll_path).map_err(|e| e.to_string())?;
        if let Ok(init_dll) = lib.get::<unsafe extern "system" fn() -> bool>(b"InitDll\0") {
            let _ = init_dll();
        }
        let get_factory: libloading::Symbol<unsafe extern "system" fn() -> *mut c_void> =
            lib.get(b"GetPluginFactory\0").map_err(|e| e.to_string())?;
        let fac_obj = get_factory();
        if fac_obj.is_null() {
            return Err("GetPluginFactory returned null".into());
        }

        let fac_vtbl = *(fac_obj as *const *const IPluginFactoryVtbl);
        let mut fac_info = PFactoryInfo {
            vendor: [0; 64],
            url: [0; 256],
            email: [0; 128],
            flags: 0,
        };
        let _ = ((*fac_vtbl).get_factory_info)(fac_obj, &mut fac_info);
        let vendor = fixed_cchar_str(&fac_info.vendor);

        let n_classes = ((*fac_vtbl).count_classes)(fac_obj).clamp(0, 128);
        let mut out = Vec::new();
        for idx in 0..n_classes {
            let mut ci = PClassInfo {
                cid: [0; 16],
                cardinality: 0,
                category: [0; 32],
                name: [0; 64],
            };
            if ((*fac_vtbl).get_class_info)(fac_obj, idx, &mut ci) == K_RESULT_OK {
                let cat = fixed_cchar_str(&ci.category);
                // Only expose Audio Module Class entries (skip separate EditController helper classes!)
                if cat.eq_ignore_ascii_case("Audio Module Class") {
                    let name = fixed_cchar_str(&ci.name);
                    out.push(DiscoveredExternalPlugin {
                        format: ExternalPluginFormat::Vst3,
                        bundle_path: dll_path.to_path_buf(),
                        id: encode_tuid_hex(&ci.cid),
                        name: if name.is_empty() {
                            "VST3 Plugin".into()
                        } else {
                            name
                        },
                        vendor: if vendor.is_empty() {
                            "VST3".into()
                        } else {
                            vendor.clone()
                        },
                        version: "VST3".into(),
                        is_instrument: false,
                    });
                }
            }
        }

        let _ = ((*fac_vtbl).unknown.release)(fac_obj);
        if let Ok(exit_dll) = lib.get::<unsafe extern "system" fn() -> bool>(b"ExitDll\0") {
            let _ = exit_dll();
        }
        Ok(out)
    }
}

// --- UI-Thread Handle for `IPlugView` (`HWND`) + `IComponent`/`IEditController` Binary Patch State ---

const VST3_STATE_MAGIC: &[u8; 4] = b"FVS3";

pub struct Vst3GuiAndStateHandle {
    _lib: Option<Arc<Library>>,
    component_ptr: *mut c_void,
    controller_ptr: *mut c_void,
    plug_view_ptr: *mut c_void,
    separate_controller: bool,
}

impl NativePluginGuiView for Vst3GuiAndStateHandle {
    fn attach_to_hwnd(&mut self, hwnd: *mut c_void) -> Result<(u32, u32), String> {
        if self.controller_ptr.is_null() {
            return Err("This VST3 plugin does not expose an IEditController GUI".into());
        }
        unsafe {
            let ctrl_vtbl = *(self.controller_ptr as *const *const IEditControllerVtbl);
            let view = ((*ctrl_vtbl).create_view)(
                self.controller_ptr,
                b"editor\0".as_ptr() as *const c_char,
            );
            if view.is_null() {
                return Err("IEditController::createView('editor') returned null (use FerricDAW's auto-mapped parameter knobs!)".into());
            }
            let view_vtbl = *(view as *const *const IPlugViewVtbl);
            let hwnd_type = b"HWND\0".as_ptr() as *const c_char;
            if ((*view_vtbl).is_platform_type_supported)(view, hwnd_type) != K_RESULT_OK {
                let _ = ((*view_vtbl).unknown.release)(view);
                return Err("VST3 IPlugView does not support Win32 HWND".into());
            }

            let mut rect = ViewRect::default();
            let _ = ((*view_vtbl).get_size)(view, &mut rect);
            let w = (rect.right - rect.left).max(640) as u32;
            let h = (rect.bottom - rect.top).max(420) as u32;

            if ((*view_vtbl).attached)(view, hwnd, hwnd_type) != K_RESULT_OK {
                let _ = ((*view_vtbl).unknown.release)(view);
                return Err("IPlugView::attached(HWND) failed".into());
            }
            self.plug_view_ptr = view;
            Ok((w, h))
        }
    }

    fn detach(&mut self) {
        if !self.plug_view_ptr.is_null() {
            unsafe {
                let vtbl = *(self.plug_view_ptr as *const *const IPlugViewVtbl);
                let _ = ((*vtbl).removed)(self.plug_view_ptr);
                let _ = ((*vtbl).unknown.release)(self.plug_view_ptr);
            }
            self.plug_view_ptr = std::ptr::null_mut();
        }
    }

    fn save_state_bytes(&mut self) -> Option<Vec<u8>> {
        if self.component_ptr.is_null() {
            return None;
        }
        unsafe {
            let comp_vtbl = *(self.component_ptr as *const *const IComponentVtbl);
            let mut comp_stream = MemoryBStream {
                vtbl: &BSTREAM_VTBL,
                data: Vec::new(),
                cursor: 0,
            };
            let _ = ((*comp_vtbl).get_state)(
                self.component_ptr,
                &mut comp_stream as *mut MemoryBStream as *mut c_void,
            );

            let mut ctrl_stream = MemoryBStream {
                vtbl: &BSTREAM_VTBL,
                data: Vec::new(),
                cursor: 0,
            };
            if !self.controller_ptr.is_null() && self.separate_controller {
                let ctrl_vtbl = *(self.controller_ptr as *const *const IEditControllerVtbl);
                let _ = ((*ctrl_vtbl).get_state)(
                    self.controller_ptr,
                    &mut ctrl_stream as *mut MemoryBStream as *mut c_void,
                );
            }

            if comp_stream.data.is_empty() && ctrl_stream.data.is_empty() {
                return None;
            }
            let mut pack = Vec::with_capacity(12 + comp_stream.data.len() + ctrl_stream.data.len());
            pack.extend_from_slice(VST3_STATE_MAGIC);
            pack.extend_from_slice(&(comp_stream.data.len() as u32).to_le_bytes());
            pack.extend_from_slice(&comp_stream.data);
            pack.extend_from_slice(&(ctrl_stream.data.len() as u32).to_le_bytes());
            pack.extend_from_slice(&ctrl_stream.data);
            Some(pack)
        }
    }

    fn load_state_bytes(&mut self, bytes: &[u8]) -> bool {
        if bytes.len() < 12 || &bytes[0..4] != VST3_STATE_MAGIC || self.component_ptr.is_null() {
            return false;
        }
        let comp_len = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
        if 8 + comp_len + 4 > bytes.len() {
            return false;
        }
        let comp_slice = &bytes[8..8 + comp_len];
        let ctrl_off = 8 + comp_len;
        let ctrl_len = u32::from_le_bytes([
            bytes[ctrl_off],
            bytes[ctrl_off + 1],
            bytes[ctrl_off + 2],
            bytes[ctrl_off + 3],
        ]) as usize;
        let ctrl_slice = if ctrl_off + 4 + ctrl_len <= bytes.len() {
            &bytes[ctrl_off + 4..ctrl_off + 4 + ctrl_len]
        } else {
            &[]
        };

        unsafe {
            let comp_vtbl = *(self.component_ptr as *const *const IComponentVtbl);
            if !comp_slice.is_empty() {
                let mut cs = MemoryBStream {
                    vtbl: &BSTREAM_VTBL,
                    data: comp_slice.to_vec(),
                    cursor: 0,
                };
                let _ = ((*comp_vtbl).set_state)(
                    self.component_ptr,
                    &mut cs as *mut MemoryBStream as *mut c_void,
                );
                if !self.controller_ptr.is_null() {
                    cs.cursor = 0;
                    let ctrl_vtbl = *(self.controller_ptr as *const *const IEditControllerVtbl);
                    let _ = ((*ctrl_vtbl).set_component_state)(
                        self.controller_ptr,
                        &mut cs as *mut MemoryBStream as *mut c_void,
                    );
                }
            }
            if !ctrl_slice.is_empty() && !self.controller_ptr.is_null() {
                let mut es = MemoryBStream {
                    vtbl: &BSTREAM_VTBL,
                    data: ctrl_slice.to_vec(),
                    cursor: 0,
                };
                let ctrl_vtbl = *(self.controller_ptr as *const *const IEditControllerVtbl);
                let _ = ((*ctrl_vtbl).set_state)(
                    self.controller_ptr,
                    &mut es as *mut MemoryBStream as *mut c_void,
                );
            }
        }
        true
    }
}

/// Real-Time Audio/MIDI/Parameter Processor for a hosted `.vst3` plugin!
pub struct HostedVst3Instance {
    _lib: Option<Arc<Library>>,
    component_ptr: *mut c_void,
    processor_ptr: *mut c_void,
    controller_ptr: *mut c_void,
    separate_controller: bool,
    _host_app: Box<HostAppCom>,
    param_ids: Vec<u32>,
    last_sent_params: Vec<f32>,
    latency_smp: usize,
    in_l: Vec<f32>,
    in_r: Vec<f32>,
    out_l: Vec<f32>,
    out_r: Vec<f32>,
    param_changes: Box<HostParamChanges>,
    event_list: Box<HostEventList>,
    processing_started: bool,
    ref_guard: Arc<AtomicU32>,
}

unsafe impl Send for HostedVst3Instance {}

pub struct LoadedVst3Bundle {
    pub audio_processor: HostedVst3Instance,
    pub gui_and_state: Vst3GuiAndStateHandle,
    pub discovered_params: Vec<PluginParamDescriptor>,
}

impl HostedVst3Instance {
    pub fn load_full(
        path: &Path,
        cid_hex: &str,
        sample_rate: f64,
        max_block: usize,
        initial_state: Option<&[u8]>,
    ) -> Result<LoadedVst3Bundle, String> {
        let dll_path =
            resolve_vst3_binary_path(path).ok_or_else(|| "VST3 binary not found".to_string())?;
        let cid = decode_tuid_hex(cid_hex).ok_or_else(|| "Invalid 32-char VST3 CID hex".to_string())?;

        unsafe {
            let lib = Arc::new(Library::new(&dll_path).map_err(|e| e.to_string())?);
            if let Ok(init_dll) = lib.get::<unsafe extern "system" fn() -> bool>(b"InitDll\0") {
                let _ = init_dll();
            }
            let get_factory: libloading::Symbol<unsafe extern "system" fn() -> *mut c_void> =
                lib.get(b"GetPluginFactory\0").map_err(|e| e.to_string())?;
            let fac_obj = get_factory();
            if fac_obj.is_null() {
                return Err("GetPluginFactory returned null".into());
            }

            let res = Self::instantiate_from_factory(
                Some(lib),
                fac_obj,
                &cid,
                sample_rate,
                max_block,
                initial_state,
            );
            let fac_vtbl = *(fac_obj as *const *const IPluginFactoryVtbl);
            let _ = ((*fac_vtbl).unknown.release)(fac_obj);
            res
        }
    }

    pub(crate) unsafe fn instantiate_from_factory(
        lib: Option<Arc<Library>>,
        fac_obj: *mut c_void,
        cid: &TUID,
        sample_rate: f64,
        max_block: usize,
        initial_state: Option<&[u8]>,
    ) -> Result<LoadedVst3Bundle, String> {
        let fac_vtbl = *(fac_obj as *const *const IPluginFactoryVtbl);
        let mut host_app = Box::new(HostAppCom {
            vtbl: &HOST_APP_VTBL,
        });

        let mut comp_ptr: *mut c_void = std::ptr::null_mut();
        if ((*fac_vtbl).create_instance)(fac_obj, cid, &IID_ICOMPONENT, &mut comp_ptr)
            != K_RESULT_OK
            || comp_ptr.is_null()
        {
            return Err("VST3 createInstance(IComponent) failed".into());
        }

        let comp_vtbl = *(comp_ptr as *const *const IComponentVtbl);
        if ((*comp_vtbl).base.initialize)(comp_ptr, &mut *host_app as *mut HostAppCom as *mut c_void)
            != K_RESULT_OK
        {
            let _ = ((*comp_vtbl).base.unknown.release)(comp_ptr);
            return Err("IComponent::initialize failed".into());
        }

        // Query `IAudioProcessor`
        let mut proc_ptr: *mut c_void = std::ptr::null_mut();
        if ((*comp_vtbl).base.unknown.query_interface)(
            comp_ptr,
            &IID_IAUDIOPROCESSOR,
            &mut proc_ptr,
        ) != K_RESULT_OK
            || proc_ptr.is_null()
        {
            let _ = ((*comp_vtbl).base.terminate)(comp_ptr);
            let _ = ((*comp_vtbl).base.unknown.release)(comp_ptr);
            return Err("IComponent does not implement IAudioProcessor".into());
        }

        // Query single-component `IEditController` OR instantiate separate controller via `getControllerClassId`!
        let mut ctrl_ptr: *mut c_void = std::ptr::null_mut();
        let mut separate_controller = false;
        if ((*comp_vtbl).base.unknown.query_interface)(
            comp_ptr,
            &IID_IEDITCONTROLLER,
            &mut ctrl_ptr,
        ) != K_RESULT_OK
            || ctrl_ptr.is_null()
        {
            let mut ctrl_cid: TUID = [0; 16];
            if ((*comp_vtbl).get_controller_class_id)(comp_ptr, &mut ctrl_cid) == K_RESULT_OK
                && ((*fac_vtbl).create_instance)(
                    fac_obj,
                    &ctrl_cid,
                    &IID_IEDITCONTROLLER,
                    &mut ctrl_ptr,
                ) == K_RESULT_OK
                && !ctrl_ptr.is_null()
            {
                let ctrl_vtbl = *(ctrl_ptr as *const *const IEditControllerVtbl);
                let _ = ((*ctrl_vtbl).base.initialize)(
                    ctrl_ptr,
                    &mut *host_app as *mut HostAppCom as *mut c_void,
                );
                separate_controller = true;
            }
        }

        let mut gui_and_state = Vst3GuiAndStateHandle {
            _lib: lib.clone(),
            component_ptr: comp_ptr,
            controller_ptr: ctrl_ptr,
            plug_view_ptr: std::ptr::null_mut(),
            separate_controller,
        };

        if let Some(state_bytes) = initial_state {
            let _ = gui_and_state.load_state_bytes(state_bytes);
        }

        // Enumerate up to 64 normalized VST3 parameters (`0.0 ..= 1.0`) from `IEditController`!
        let mut discovered_params = Vec::new();
        let mut param_ids = Vec::new();
        let mut last_sent_params = Vec::new();
        if !ctrl_ptr.is_null() {
            let ctrl_vtbl = *(ctrl_ptr as *const *const IEditControllerVtbl);
            let p_count = ((*ctrl_vtbl).get_parameter_count)(ctrl_ptr).clamp(0, 64);
            for i in 0..p_count {
                let mut p_info: ParameterInfo = std::mem::zeroed();
                if ((*ctrl_vtbl).get_parameter_info)(ctrl_ptr, i, &mut p_info) == K_RESULT_OK {
                    let cur_norm = ((*ctrl_vtbl).get_param_normalized)(ctrl_ptr, p_info.id)
                        .clamp(0.0, 1.0) as f32;
                    let mut title = utf16_buf_to_string(&p_info.title);
                    if title.is_empty() {
                        title = format!("Param {}", i + 1);
                    }
                    let units = utf16_buf_to_string(&p_info.units);
                    discovered_params.push(PluginParamDescriptor {
                        plugin_param_id: p_info.id,
                        name: title,
                        module: units,
                        min: 0.0,
                        max: 1.0,
                        default: cur_norm,
                    });
                    param_ids.push(p_info.id);
                    last_sent_params.push(cur_norm);
                }
            }
        }

        // Activate main audio/event buses & configure 32-bit real-time processing!
        let n_in_audio = ((*comp_vtbl).get_bus_count)(comp_ptr, 0, 0);
        if n_in_audio > 0 {
            let _ = ((*comp_vtbl).activate_bus)(comp_ptr, 0, 0, 0, 1);
        }
        let n_out_audio = ((*comp_vtbl).get_bus_count)(comp_ptr, 0, 1);
        if n_out_audio > 0 {
            let _ = ((*comp_vtbl).activate_bus)(comp_ptr, 0, 1, 0, 1);
        }
        let n_in_ev = ((*comp_vtbl).get_bus_count)(comp_ptr, 1, 0);
        if n_in_ev > 0 {
            let _ = ((*comp_vtbl).activate_bus)(comp_ptr, 1, 0, 0, 1);
        }

        let proc_vtbl = *(proc_ptr as *const *const IAudioProcessorVtbl);
        let blk = max_block.max(4096);
        let mut setup = ProcessSetup {
            process_mode: 0,         // kRealtime
            symbolic_sample_size: 0, // kSample32
            max_samples_per_block: blk as i32,
            sample_rate,
        };
        let _ = ((*proc_vtbl).setup_processing)(proc_ptr, &mut setup);
        let _ = ((*comp_vtbl).set_active)(comp_ptr, 1);
        let latency_smp = ((*proc_vtbl).get_latency_samples)(proc_ptr) as usize;

        let empty_q = HostParamQueue {
            vtbl: &PVQ_VTBL,
            param_id: 0,
            value: 0.0,
        };
        let param_changes = Box::new(HostParamChanges {
            vtbl: &PC_VTBL,
            queues: [empty_q; 64],
            count: 0,
        });
        let zero_ev: Vst3Event = std::mem::zeroed();
        let event_list = Box::new(HostEventList {
            vtbl: &EV_VTBL,
            events: [zero_ev; 128],
            count: 0,
        });

        let audio_processor = Self {
            _lib: lib,
            component_ptr: comp_ptr,
            processor_ptr: proc_ptr,
            controller_ptr: ctrl_ptr,
            separate_controller,
            _host_app: host_app,
            param_ids,
            last_sent_params,
            latency_smp,
            in_l: vec![0.0; blk],
            in_r: vec![0.0; blk],
            out_l: vec![0.0; blk],
            out_r: vec![0.0; blk],
            param_changes,
            event_list,
            processing_started: false,
            ref_guard: Arc::new(AtomicU32::new(1)),
        };

        let _ = vst3_com_iid;
        Ok(LoadedVst3Bundle {
            audio_processor,
            gui_and_state,
            discovered_params,
        })
    }
}

impl ExternalPluginAdapter for HostedVst3Instance {
    fn latency_samples(&self) -> usize {
        self.latency_smp
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
        if frames == 0 || self.processor_ptr.is_null() {
            return;
        }
        unsafe {
            let proc_vtbl = *(self.processor_ptr as *const *const IAudioProcessorVtbl);
            if !self.processing_started {
                let _ = ((*proc_vtbl).set_processing)(self.processor_ptr, 1);
                self.processing_started = true;
            }

            self.in_l[..frames].copy_from_slice(&left[..frames]);
            self.in_r[..frames].copy_from_slice(&right[..frames]);
            self.out_l[..frames].fill(0.0);
            self.out_r[..frames].fill(0.0);

            // 1. Populate `IParameterChanges` queues for any changed parameters!
            self.param_changes.count = 0;
            let p_len = params.len().min(self.param_ids.len()).min(64);
            for idx in 0..p_len {
                let norm = params[idx].clamp(0.0, 1.0);
                if (norm - self.last_sent_params[idx]).abs() > 1e-6 {
                    self.last_sent_params[idx] = norm;
                    let q_idx = self.param_changes.count;
                    self.param_changes.queues[q_idx] = HostParamQueue {
                        vtbl: &PVQ_VTBL,
                        param_id: self.param_ids[idx],
                        value: norm as f64,
                    };
                    self.param_changes.count += 1;
                }
            }

            // 2. Populate `IEventList` for sample-accurate MIDI Note-On / Note-Off events!
            self.event_list.count = 0;
            for ev in midi_events.iter().take(128) {
                let slot = self.event_list.count;
                let offset = ev.sample_offset.min(frames.saturating_sub(1)) as i32;
                if ev.is_note_on && ev.velocity > 0 {
                    self.event_list.events[slot] = Vst3Event {
                        bus_index: 0,
                        sample_offset: offset,
                        ppq_position: 0.0,
                        flags: 1,
                        event_type: 0, // kNoteOnEvent
                        data: Vst3EventData {
                            note_on: Vst3NoteOnEvent {
                                channel: 0,
                                pitch: ev.note as i16,
                                tuning: 0.0,
                                velocity: (ev.velocity as f32 / 127.0).clamp(0.0, 1.0),
                                length: 0,
                                note_id: -1,
                            },
                        },
                    };
                } else {
                    self.event_list.events[slot] = Vst3Event {
                        bus_index: 0,
                        sample_offset: offset,
                        ppq_position: 0.0,
                        flags: 0,
                        event_type: 1, // kNoteOffEvent
                        data: Vst3EventData {
                            note_off: Vst3NoteOffEvent {
                                channel: 0,
                                pitch: ev.note as i16,
                                velocity: 0.0,
                                note_id: -1,
                                tuning: 0.0,
                            },
                        },
                    };
                }
                self.event_list.count += 1;
            }

            let mut in_ch_ptrs = [self.in_l.as_mut_ptr(), self.in_r.as_mut_ptr()];
            let mut out_ch_ptrs = [self.out_l.as_mut_ptr(), self.out_r.as_mut_ptr()];
            let mut in_bus = AudioBusBuffers {
                num_channels: 2,
                silence_flags: 0,
                channel_buffers32: in_ch_ptrs.as_mut_ptr(),
            };
            let mut out_bus = AudioBusBuffers {
                num_channels: 2,
                silence_flags: 0,
                channel_buffers32: out_ch_ptrs.as_mut_ptr(),
            };

            let mut pdata = ProcessData {
                process_mode: 0,
                symbolic_sample_size: 0,
                num_samples: frames as i32,
                num_inputs: 1,
                num_outputs: 1,
                inputs: &mut in_bus,
                outputs: &mut out_bus,
                input_param_changes: &mut *self.param_changes as *mut HostParamChanges
                    as *mut c_void,
                output_param_changes: std::ptr::null_mut(),
                input_events: &mut *self.event_list as *mut HostEventList as *mut c_void,
                output_events: std::ptr::null_mut(),
                process_context: std::ptr::null_mut(),
            };

            let _ = ((*proc_vtbl).process)(self.processor_ptr, &mut pdata);

            for i in 0..frames {
                left[i] = self.out_l[i];
                right[i] = self.out_r[i];
            }
        }
    }
}

impl Drop for HostedVst3Instance {
    fn drop(&mut self) {
        if self.ref_guard.swap(0, Ordering::AcqRel) == 0 {
            return;
        }
        unsafe {
            if !self.processor_ptr.is_null() {
                let proc_vtbl = *(self.processor_ptr as *const *const IAudioProcessorVtbl);
                if self.processing_started {
                    let _ = ((*proc_vtbl).set_processing)(self.processor_ptr, 0);
                }
                let _ = ((*proc_vtbl).unknown.release)(self.processor_ptr);
            }
            if !self.controller_ptr.is_null() {
                let ctrl_vtbl = *(self.controller_ptr as *const *const IEditControllerVtbl);
                if self.separate_controller {
                    let _ = ((*ctrl_vtbl).base.terminate)(self.controller_ptr);
                }
                let _ = ((*ctrl_vtbl).base.unknown.release)(self.controller_ptr);
            }
            if !self.component_ptr.is_null() {
                let comp_vtbl = *(self.component_ptr as *const *const IComponentVtbl);
                let _ = ((*comp_vtbl).set_active)(self.component_ptr, 0);
                let _ = ((*comp_vtbl).base.terminate)(self.component_ptr);
                let _ = ((*comp_vtbl).base.unknown.release)(self.component_ptr);
            }
            let _ = CStr::from_bytes_with_nul_unchecked(b"\0");
        }
    }
}
