use ferric_model::ExternalPluginFormat;
use serde::{Deserialize, Serialize};

pub const BRIDGE_IO_VERSION: u32 = 1;
pub const DEFAULT_MAX_FRAMES: u32 = 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeMidiEvent {
    pub sample_offset: u32,
    pub note: u8,
    pub velocity: u8,
    pub is_note_on: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BridgeRequest {
    LoadPlugin {
        format: ExternalPluginFormat,
        path: String,
        plugin_id: String,
        state_b64: Option<String>,
        sample_rate: f64,
        max_frames: u32,
    },
    ProcessBlock {
        frames: u32,
        bpm: f64,
        params: Vec<f32>,
        midi: Vec<BridgeMidiEvent>,
    },
    GetState,
    SetState { state_b64: String },
    Shutdown,
    Heartbeat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BridgeResponse {
    Ok,
    Loaded {
        latency_samples: u32,
    },
    Processed,
    State {
        state_b64: String,
    },
    Error {
        message: String,
    },
    Pong,
}

/// Borrowed form of `BridgeRequest::ProcessBlock` with the identical JSON
/// shape, so the audio thread can serialise a block without allocating.
#[derive(Serialize)]
#[serde(tag = "kind", rename = "process_block")]
pub struct ProcessBlockRef<'a> {
    pub frames: u32,
    pub bpm: f64,
    pub params: &'a [f32],
    pub midi: &'a [BridgeMidiEvent],
}

/// Encode a process-block request into `out` (cleared first). Reuses `out`'s
/// capacity, so steady-state calls do not allocate.
pub fn encode_process_block_into(out: &mut Vec<u8>, req: &ProcessBlockRef<'_>) -> Result<(), String> {
    out.clear();
    out.extend_from_slice(&[0u8; 4]);
    serde_json::to_writer(&mut *out, req).map_err(|e| e.to_string())?;
    let len = (out.len() - 4) as u32;
    out[..4].copy_from_slice(&len.to_le_bytes());
    Ok(())
}

/// Exact payload the worker sends for `BridgeResponse::Processed`.
pub const PROCESSED_PAYLOAD: &[u8] = br#"{"kind":"processed"}"#;

/// Decode a response payload (no length prefix).
pub fn decode_response_payload(payload: &[u8]) -> Result<BridgeResponse, String> {
    if payload == PROCESSED_PAYLOAD {
        return Ok(BridgeResponse::Processed);
    }
    serde_json::from_slice(payload).map_err(|e| e.to_string())
}

pub fn encode_request(req: &BridgeRequest) -> Result<Vec<u8>, String> {
    let json = serde_json::to_vec(req).map_err(|e| e.to_string())?;
    let len = (json.len() as u32).to_le_bytes();
    let mut out = Vec::with_capacity(4 + json.len());
    out.extend_from_slice(&len);
    out.extend_from_slice(&json);
    Ok(out)
}

pub fn decode_response(bytes: &[u8]) -> Result<BridgeResponse, String> {
    if bytes.len() < 4 {
        return Err("response too short".into());
    }
    let len = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if bytes.len() < 4 + len {
        return Err("truncated response".into());
    }
    serde_json::from_slice(&bytes[4..4 + len]).map_err(|e| e.to_string())
}

pub fn decode_request(bytes: &[u8]) -> Result<BridgeRequest, String> {
    if bytes.len() < 4 {
        return Err("request too short".into());
    }
    let len = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if bytes.len() < 4 + len {
        return Err("truncated request".into());
    }
    serde_json::from_slice(&bytes[4..4 + len]).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_model::ExternalPluginFormat;

    #[test]
    fn round_trip_process_block_framing() {
        let req = BridgeRequest::ProcessBlock {
            frames: 128,
            bpm: 120.0,
            params: vec![0.5],
            midi: vec![BridgeMidiEvent {
                sample_offset: 4,
                note: 60,
                velocity: 100,
                is_note_on: true,
            }],
        };
        let bytes = encode_request(&req).unwrap();
        let back = decode_request(&bytes).unwrap();
        assert!(matches!(back, BridgeRequest::ProcessBlock { frames: 128, .. }));

        let resp = encode_response(&BridgeResponse::Loaded {
            latency_samples: 64,
        })
        .unwrap();
        assert!(matches!(
            decode_response(&resp).unwrap(),
            BridgeResponse::Loaded { latency_samples: 64 }
        ));

        let load = BridgeRequest::LoadPlugin {
            format: ExternalPluginFormat::Vst3,
            path: "C:\\plugin.dll".into(),
            plugin_id: "uid".into(),
            state_b64: None,
            sample_rate: 48_000.0,
            max_frames: 512,
        };
        // The borrowed fast-path encoder produces the same request.
        let mut buf = Vec::new();
        encode_process_block_into(
            &mut buf,
            &ProcessBlockRef {
                frames: 64,
                bpm: 99.5,
                params: &[0.25, 1.0],
                midi: &[BridgeMidiEvent {
                    sample_offset: 1,
                    note: 61,
                    velocity: 90,
                    is_note_on: false,
                }],
            },
        )
        .unwrap();
        match decode_request(&buf).unwrap() {
            BridgeRequest::ProcessBlock { frames, bpm, params, midi } => {
                assert_eq!((frames, bpm), (64, 99.5));
                assert_eq!(params, vec![0.25, 1.0]);
                assert_eq!(midi[0].note, 61);
            }
            other => panic!("unexpected {other:?}"),
        }
        let cap = buf.capacity();
        encode_process_block_into(
            &mut buf,
            &ProcessBlockRef { frames: 32, bpm: 120.0, params: &[0.5], midi: &[] },
        )
        .unwrap();
        assert_eq!(buf.capacity(), cap, "steady-state encode must reuse the buffer");
        let processed = encode_response(&BridgeResponse::Processed).unwrap();
        assert_eq!(&processed[4..], PROCESSED_PAYLOAD);
        assert!(matches!(
            decode_response_payload(PROCESSED_PAYLOAD).unwrap(),
            BridgeResponse::Processed
        ));

        let load_bytes = encode_request(&load).unwrap();
        assert!(matches!(
            decode_request(&load_bytes).unwrap(),
            BridgeRequest::LoadPlugin { .. }
        ));
    }
}

pub fn encode_response(resp: &BridgeResponse) -> Result<Vec<u8>, String> {
    let json = serde_json::to_vec(resp).map_err(|e| e.to_string())?;
    let len = (json.len() as u32).to_le_bytes();
    let mut out = Vec::with_capacity(4 + json.len());
    out.extend_from_slice(&len);
    out.extend_from_slice(&json);
    Ok(out)
}