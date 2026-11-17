//! True WASAPI Exclusive-mode stereo output (device-locking) via Windows COM.
//! Uses the default render endpoint (Shared input stays on cpal). On failure the
//! caller opens Shared WASAPI with the recovered process callback.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// Owns the exclusive render thread; dropping stops playback and unlocks the device.
pub struct ExclusiveOutputSession {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Drop for ExclusiveOutputSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.join.take() {
            let _ = h.join();
        }
    }
}

/// Start exclusive stereo output. On failure returns the process closure so Shared WASAPI can own it.
#[cfg(windows)]
pub fn try_start_exclusive_owned<F>(
    preferred_name: Option<&str>,
    sample_rate: u32,
    buffer_frames: u32,
    process: F,
) -> Result<(ExclusiveOutputSession, u32), (Option<F>, String)>
where
    F: FnMut(&mut [f32], usize) + Send + 'static,
{
    use windows::core::PCWSTR;
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows::Win32::Media::Audio::{
        eConsole, eRender, IAudioClient, IAudioRenderClient, IMMDevice, IMMDeviceEnumerator,
        MMDeviceEnumerator, AUDCLNT_SHAREMODE_EXCLUSIVE, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
        DEVICE_STATE_ACTIVE, WAVEFORMATEX, WAVE_FORMAT_PCM,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED,
        STGM_READ,
    };
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
    use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;

    fn device_friendly_name(device: &IMMDevice) -> Option<String> {
        unsafe {
            let store = device.OpenPropertyStore(STGM_READ).ok()?;
            let key: PROPERTYKEY = PKEY_Device_FriendlyName;
            let pv = store.GetValue(&key).ok()?;
            let s = pv.to_string();
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        }
    }

    fn find_render_device(
        enumerator: &IMMDeviceEnumerator,
        preferred: Option<&str>,
    ) -> Result<IMMDevice, String> {
        if let Some(want) = preferred.filter(|s| !s.trim().is_empty()) {
            let want_l = want.to_lowercase();
            let collection = unsafe {
                enumerator
                    .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
                    .map_err(|e| format!("EnumAudioEndpoints: {e}"))?
            };
            let count = unsafe { collection.GetCount().unwrap_or(0) };
            let mut best: Option<(usize, IMMDevice)> = None;
            for i in 0..count {
                let Ok(dev) = (unsafe { collection.Item(i) }) else {
                    continue;
                };
                let Some(name) = device_friendly_name(&dev) else {
                    continue;
                };
                let name_l = name.to_lowercase();
                if name_l == want_l {
                    return Ok(dev);
                }
                if name_l.contains(&want_l) || want_l.contains(&name_l) {
                    let score = name_l.len().abs_diff(want_l.len());
                    if best.as_ref().map(|(s, _)| score < *s).unwrap_or(true) {
                        best = Some((score, dev));
                    }
                }
            }
            if let Some((_, dev)) = best {
                return Ok(dev);
            }
        }
        unsafe {
            enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|e| format!("default endpoint: {e}"))
        }
    }

    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = stop.clone();
    let period_hint = buffer_frames.clamp(32, 2048);
    let preferred_owned = preferred_name.map(|s| s.to_string());
    let process_slot: Arc<Mutex<Option<F>>> = Arc::new(Mutex::new(Some(process)));
    let process_for_thread = process_slot.clone();
    let (tx, rx) = std::sync::mpsc::channel::<Result<u32, String>>();

    let join = match std::thread::Builder::new()
        .name("ferric-wasapi-excl".into())
        .spawn(move || {
            if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
                let _ = tx.send(Err("CoInitializeEx failed".into()));
                return;
            }

            let opened = (|| -> Result<
                (
                    IAudioClient,
                    IAudioRenderClient,
                    windows::Win32::Foundation::HANDLE,
                    u32,
                    u16,
                    bool,
                ),
                String,
            > {
                let enumerator: IMMDeviceEnumerator = unsafe {
                    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                        .map_err(|e| format!("MMDeviceEnumerator: {e}"))?
                };

                // Prefer the user-selected output (Hardware Rig); fall back to default console.
                let device: IMMDevice =
                    find_render_device(&enumerator, preferred_owned.as_deref())?;

                let client: IAudioClient = unsafe {
                    device
                        .Activate(CLSCTX_ALL, None)
                        .map_err(|e| format!("Activate IAudioClient: {e}"))?
                };

                // Exclusive must negotiate explicitly — do not adopt Shared "closest" formats.
                const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;
                let mix_sr = unsafe {
                    client
                        .GetMixFormat()
                        .ok()
                        .map(|m| {
                            let sr = (*m).nSamplesPerSec;
                            CoTaskMemFree(Some(m as *const _));
                            sr
                        })
                        .unwrap_or(sample_rate)
                };

                let make_fmt = |tag: u16, bits: u16, sr: u32| -> WAVEFORMATEX {
                    let bytes = (bits / 8) as u32;
                    WAVEFORMATEX {
                        wFormatTag: tag,
                        nChannels: 2,
                        nSamplesPerSec: sr,
                        nAvgBytesPerSec: sr * 2 * bytes,
                        nBlockAlign: (2 * bytes) as u16,
                        wBitsPerSample: bits,
                        cbSize: 0,
                    }
                };
                let exclusive_ok = |client: &IAudioClient, fmt: &WAVEFORMATEX| -> bool {
                    let mut closest: *mut WAVEFORMATEX = std::ptr::null_mut();
                    let supported = unsafe {
                        client.IsFormatSupported(
                            AUDCLNT_SHAREMODE_EXCLUSIVE,
                            fmt,
                            Some(&mut closest),
                        )
                    };
                    if !closest.is_null() {
                        // Exclusive rarely returns a usable closest; free and ignore.
                        unsafe { CoTaskMemFree(Some(closest as *const _)) };
                    }
                    supported.is_ok()
                };

                let candidates = [
                    make_fmt(WAVE_FORMAT_IEEE_FLOAT, 32, sample_rate),
                    make_fmt(WAVE_FORMAT_PCM as u16, 16, sample_rate),
                    make_fmt(WAVE_FORMAT_IEEE_FLOAT, 32, mix_sr),
                    make_fmt(WAVE_FORMAT_PCM as u16, 16, mix_sr),
                ];
                let mut chosen: Option<WAVEFORMATEX> = None;
                for cand in candidates {
                    if exclusive_ok(&client, &cand) {
                        chosen = Some(cand);
                        break;
                    }
                }
                let fmt = chosen.ok_or_else(|| {
                    "Device rejected exclusive float32/PCM16 stereo at project and mix rates".to_string()
                })?;
                let is_float = fmt.wFormatTag == WAVE_FORMAT_IEEE_FLOAT;

                let hns = ((period_hint as i64) * 10_000_000) / (fmt.nSamplesPerSec as i64);
                let hns = hns.clamp(2_500_000, 30_000_000);

                unsafe {
                    client
                        .Initialize(
                            AUDCLNT_SHAREMODE_EXCLUSIVE,
                            AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                            hns,
                            hns,
                            &fmt,
                            None,
                        )
                        .map_err(|e| format!("IAudioClient::Initialize exclusive: {e}"))?;
                }

                let event = unsafe {
                    CreateEventW(None, false, false, PCWSTR::null())
                        .map_err(|e| format!("CreateEvent: {e}"))?
                };
                unsafe {
                    client
                        .SetEventHandle(event)
                        .map_err(|e| format!("SetEventHandle: {e}"))?;
                }

                let render: IAudioRenderClient = unsafe {
                    client
                        .GetService()
                        .map_err(|e| format!("GetService IAudioRenderClient: {e}"))?
                };
                let buf_frames = unsafe {
                    client
                        .GetBufferSize()
                        .map_err(|e| format!("GetBufferSize: {e}"))?
                };
                let channels = fmt.nChannels;
                unsafe {
                    client
                        .Start()
                        .map_err(|e| format!("IAudioClient::Start: {e}"))?;
                }
                Ok((client, render, event, buf_frames, channels, is_float))
            })();

            match opened {
                Err(msg) => {
                    let _ = tx.send(Err(msg));
                }
                Ok((client, render, event, buf_frames, channels, is_float)) => {
                    let Some(mut process) =
                        process_for_thread.lock().ok().and_then(|mut g| g.take())
                    else {
                        let _ = tx.send(Err("Exclusive process callback missing".into()));
                        return;
                    };
                    let _ = tx.send(Ok(buf_frames));
                    let ch = channels.max(1) as usize;
                    let mut scratch =
                        vec![0.0_f32; (buf_frames as usize).saturating_mul(2).max(512)];

                    while !stop_flag.load(Ordering::Relaxed) {
                        let wait = unsafe { WaitForSingleObject(event, 50) };
                        if wait == WAIT_TIMEOUT {
                            continue;
                        }
                        if wait != WAIT_OBJECT_0 {
                            break;
                        }
                        let padding = unsafe { client.GetCurrentPadding() }.unwrap_or(0);
                        let available = buf_frames.saturating_sub(padding);
                        if available == 0 {
                            continue;
                        }
                        let Ok(ptr) = (unsafe { render.GetBuffer(available) }) else {
                            continue;
                        };
                        let frames = available as usize;
                        let need = frames * 2;
                        if scratch.len() < need {
                            scratch.resize(need, 0.0);
                        }
                        process(&mut scratch[..need], 2);
                        if is_float {
                            let dst = unsafe {
                                std::slice::from_raw_parts_mut(ptr as *mut f32, frames * ch)
                            };
                            for f in 0..frames {
                                dst[f * ch] = scratch[f * 2].clamp(-1.0, 1.0);
                                if ch > 1 {
                                    dst[f * ch + 1] = scratch[f * 2 + 1].clamp(-1.0, 1.0);
                                }
                            }
                        } else {
                            let dst = unsafe {
                                std::slice::from_raw_parts_mut(ptr as *mut i16, frames * ch)
                            };
                            for f in 0..frames {
                                let l = (scratch[f * 2].clamp(-1.0, 1.0) * 32767.0) as i16;
                                let r = (scratch[f * 2 + 1].clamp(-1.0, 1.0) * 32767.0) as i16;
                                dst[f * ch] = l;
                                if ch > 1 {
                                    dst[f * ch + 1] = r;
                                }
                            }
                        }
                        let _ = unsafe { render.ReleaseBuffer(available, 0) };
                    }
                    let _ = unsafe { client.Stop() };
                    let _ = unsafe { CloseHandle(event) };
                }
            }
        }) {
        Ok(j) => j,
        Err(e) => {
            let process = process_slot.lock().ok().and_then(|mut g| g.take());
            return Err((process, format!("Exclusive thread spawn failed: {e}")));
        }
    };

    match rx.recv() {
        Ok(Ok(period)) => Ok((
            ExclusiveOutputSession {
                stop,
                join: Some(join),
            },
            period,
        )),
        Ok(Err(msg)) => {
            stop.store(true, Ordering::SeqCst);
            let _ = join.join();
            let process = process_slot.lock().ok().and_then(|mut g| g.take());
            Err((process, msg))
        }
        Err(_) => {
            stop.store(true, Ordering::SeqCst);
            let _ = join.join();
            let process = process_slot.lock().ok().and_then(|mut g| g.take());
            Err((process, "Exclusive thread died before ready.".into()))
        }
    }
}

#[cfg(not(windows))]
pub fn try_start_exclusive_owned<F>(
    _preferred_name: Option<&str>,
    _sample_rate: u32,
    _buffer_frames: u32,
    process: F,
) -> Result<(ExclusiveOutputSession, u32), (Option<F>, String)>
where
    F: FnMut(&mut [f32], usize) + Send + 'static,
{
    Err((
        Some(process),
        "WASAPI Exclusive is only available on Windows.".into(),
    ))
}
