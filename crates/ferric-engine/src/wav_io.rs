use crate::flac_enc::write_flac_stereo_file;
use crate::graph::AudioGraphCore;
use ferric_dsp::{
    master_normalize_and_limit_true_peak, measure_integrated_lufs, measure_true_peak_dbtp,
    LoudnessAndSpectrumAnalyzer, TpdfDither,
};
use ferric_model::{AudioBuffer, Project, TrackId};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use std::fs::File;
use std::path::{Path, PathBuf};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WavIoError {
    #[error("WAV error: {0}")]
    Hound(#[from] hound::Error),
    #[error("Audio codec error: {0}")]
    Codec(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitDepth {
    Pcm16,
    Pcm24,
    Float32,
}

impl BitDepth {
    pub const ALL: &'static [BitDepth] = &[BitDepth::Pcm16, BitDepth::Pcm24, BitDepth::Float32];

    pub fn label(self) -> &'static str {
        match self {
            BitDepth::Pcm16 => "16-bit PCM (CD)",
            BitDepth::Pcm24 => "24-bit PCM (Studio)",
            BitDepth::Float32 => "32-bit Float (Mastering)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Wav,
    Flac,
}

impl ExportFormat {
    pub const ALL: &'static [ExportFormat] = &[ExportFormat::Wav, ExportFormat::Flac];

    pub fn label(self) -> &'static str {
        match self {
            ExportFormat::Wav => "WAV (Uncompressed PCM / Float)",
            ExportFormat::Flac => "FLAC (Lossless Compressed + MD5)",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Wav => "wav",
            ExportFormat::Flac => "flac",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MasteringOptions {
    pub format: ExportFormat,
    pub sample_rate: u32,
    pub bit_depth: BitDepth,
    pub start_beat: f64,
    pub end_beat: f64,
    /// When `Some(target_lufs)` (e.g. `-14.0` for Spotify/YouTube/Apple or `-9.0` for Club),
    /// normalizes integrated BS.1770-4 loudness and applies 2.5 ms lookahead True-Peak brickwall limiting!
    pub normalize_target_lufs: Option<f32>,
    pub true_peak_ceiling_dbtp: f32,
    pub tpdf_dither: bool,
}

impl Default for MasteringOptions {
    fn default() -> Self {
        Self {
            format: ExportFormat::Wav,
            sample_rate: 48_000,
            bit_depth: BitDepth::Pcm24,
            start_beat: 0.0,
            end_beat: 16.0,
            normalize_target_lufs: Some(-14.0),
            true_peak_ceiling_dbtp: -1.0,
            tpdf_dither: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BounceReport {
    pub output_files: Vec<PathBuf>,
    pub duration_secs: f64,
    pub peak_db: f32,
    pub true_peak_dbtp: f32,
    pub integrated_lufs: f32,
    pub short_term_lufs: f32,
}

/// Universal Multi-Codec Audio Decoder (`WAV`, `FLAC`, `MP3`, `OGG/Vorbis`, `AIFF`)!
/// Uses pure-Rust `symphonia` with `hound` fallback and resamples cleanly to `target_sample_rate`.
pub fn import_audio_file(path: &Path, target_sample_rate: u32) -> Result<AudioBuffer, WavIoError> {
    if let Ok(buf) = decode_with_symphonia(path, target_sample_rate) {
        if buf.len_frames() > 0 {
            return Ok(buf);
        }
    }
    import_wav_file(path, target_sample_rate)
}

fn decode_with_symphonia(path: &Path, target_sample_rate: u32) -> Result<AudioBuffer, WavIoError> {
    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| WavIoError::Codec(e.to_string()))?;

    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| WavIoError::Codec("No audio track found".into()))?;
    let track_id = track.id;
    let src_sr = track.codec_params.sample_rate.unwrap_or(44_100).max(8_000);

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| WavIoError::Codec(e.to_string()))?;

    let mut src_l = Vec::new();
    let mut src_r = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(_)) | Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(WavIoError::Codec(e.to_string())),
        };
        if packet.track_id() != track_id {
            continue;
        }
        if let Ok(decoded) = decoder.decode(&packet) {
            let spec = *decoded.spec();
            let ch = spec.channels.count().max(1);
            let frames = decoded.frames();
            let mut sample_buf = SampleBuffer::<f32>::new(frames as u64, spec);
            sample_buf.copy_interleaved_ref(decoded);
            let samples = sample_buf.samples();
            for f in 0..frames {
                let base = f * ch;
                let l = samples[base];
                let r = if ch >= 2 { samples[base + 1] } else { l };
                src_l.push(l);
                src_r.push(r);
            }
        }
    }

    Ok(resample_to_target(
        src_sr,
        target_sample_rate,
        src_l,
        src_r,
        Some(path.to_path_buf()),
    ))
}

pub fn import_wav_file(path: &Path, target_sample_rate: u32) -> Result<AudioBuffer, WavIoError> {
    let mut reader = WavReader::open(path)?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let src_sr = spec.sample_rate.max(8_000);

    let interleaved: Vec<f32> = match spec.sample_format {
        SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap_or(0.0)).collect(),
        SampleFormat::Int => {
            let max_val = ((1_i64 << (spec.bits_per_sample.saturating_sub(1))) as f32).max(1.0);
            reader
                .samples::<i32>()
                .map(|s| (s.unwrap_or(0) as f32 / max_val).clamp(-1.0, 1.0))
                .collect()
        }
    };

    let frames = interleaved.len() / channels;
    let mut src_l = Vec::with_capacity(frames);
    let mut src_r = Vec::with_capacity(frames);
    for f in 0..frames {
        let base = f * channels;
        let l = interleaved[base];
        let r = if channels >= 2 {
            interleaved[base + 1]
        } else {
            l
        };
        src_l.push(l);
        src_r.push(r);
    }

    Ok(resample_to_target(
        src_sr,
        target_sample_rate,
        src_l,
        src_r,
        Some(path.to_path_buf()),
    ))
}

fn resample_to_target(
    src_sr: u32,
    target_sr: u32,
    src_l: Vec<f32>,
    src_r: Vec<f32>,
    path: Option<PathBuf>,
) -> AudioBuffer {
    let frames = src_l.len().min(src_r.len());
    if src_sr == target_sr || frames < 2 {
        return AudioBuffer::from_stereo(target_sr, src_l, src_r, path);
    }
    let ratio = src_sr as f64 / target_sr as f64;
    let out_frames = ((frames as f64) / ratio).ceil() as usize;
    let tmp = AudioBuffer::from_stereo(src_sr, src_l, src_r, None);
    let mut out_l = Vec::with_capacity(out_frames);
    let mut out_r = Vec::with_capacity(out_frames);
    for i in 0..out_frames {
        let (l, r) = tmp.sample_interpolated(i as f64 * ratio);
        out_l.push(l);
        out_r.push(r);
    }
    AudioBuffer::from_stereo(target_sr, out_l, out_r, path)
}

pub fn write_wav_file(
    path: &Path,
    sample_rate: u32,
    bit_depth: BitDepth,
    left: &[f32],
    right: &[f32],
) -> Result<(), WavIoError> {
    write_wav_file_dithered(path, sample_rate, bit_depth, false, left, right)
}

pub fn write_wav_file_dithered(
    path: &Path,
    sample_rate: u32,
    bit_depth: BitDepth,
    tpdf_dither: bool,
    left: &[f32],
    right: &[f32],
) -> Result<(), WavIoError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let spec = match bit_depth {
        BitDepth::Pcm16 => WavSpec {
            channels: 2,
            sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        },
        BitDepth::Pcm24 => WavSpec {
            channels: 2,
            sample_rate,
            bits_per_sample: 24,
            sample_format: SampleFormat::Int,
        },
        BitDepth::Float32 => WavSpec {
            channels: 2,
            sample_rate,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        },
    };

    let mut writer = WavWriter::create(path, spec)?;
    let frames = left.len().min(right.len());
    let mut dither = TpdfDither::default();
    match bit_depth {
        BitDepth::Pcm16 => {
            for i in 0..frames {
                let l = dither.quantize_sample(left[i], 16, tpdf_dither) as i16;
                let r = dither.quantize_sample(right[i], 16, tpdf_dither) as i16;
                writer.write_sample(l)?;
                writer.write_sample(r)?;
            }
        }
        BitDepth::Pcm24 => {
            for i in 0..frames {
                let l = dither.quantize_sample(left[i], 24, tpdf_dither);
                let r = dither.quantize_sample(right[i], 24, tpdf_dither);
                writer.write_sample(l)?;
                writer.write_sample(r)?;
            }
        }
        BitDepth::Float32 => {
            for i in 0..frames {
                writer.write_sample(left[i])?;
                writer.write_sample(right[i])?;
            }
        }
    }
    writer.finalize()?;
    Ok(())
}

pub fn write_exported_audio_file(
    path: &Path,
    opts: &MasteringOptions,
    left: &[f32],
    right: &[f32],
) -> Result<(), WavIoError> {
    match opts.format {
        ExportFormat::Wav => write_wav_file_dithered(
            path,
            opts.sample_rate,
            opts.bit_depth,
            opts.tpdf_dither && opts.bit_depth != BitDepth::Float32,
            left,
            right,
        ),
        ExportFormat::Flac => {
            let bps = match opts.bit_depth {
                BitDepth::Pcm16 => 16,
                BitDepth::Pcm24 | BitDepth::Float32 => 24,
            };
            write_flac_stereo_file(
                path,
                opts.sample_rate,
                bps,
                opts.tpdf_dither,
                left,
                right,
            )?;
            Ok(())
        }
    }
}

/// 1-Click Reversible Track Freeze: Renders a track's pre-fader Instrument + FX output
/// faster-than-realtime into a 32-bit `AudioBuffer` so live DSP drops to 0.0% CPU!
pub fn render_track_freeze_buffer(
    project: &Project,
    track_id: TrackId,
    sample_rate: u32,
) -> Option<AudioBuffer> {
    let mut solo_proj = project.clone();
    solo_proj.loop_enabled = false;
    solo_proj.metronome_enabled = false;
    solo_proj.master.fx_chain.clear();
    solo_proj.master.gain_db = 0.0;
    solo_proj.master.safety_limiter = false;

    for t in &mut solo_proj.tracks {
        if t.id == track_id {
            t.frozen = false;
            t.frozen_buffer = None;
            t.solo = true;
            t.mute = false;
            t.gain_db = 0.0;
            t.pan = 0.0;
            t.parent_folder_id = None;
            t.send_to_master = true;
        } else {
            // Keep sidechain key sources unmuted conceptually, though solo mutes their master sum!
            t.solo = false;
        }
    }

    let end_beat = solo_proj.project_end_beat() + 2.0;
    let total_secs = solo_proj.beats_to_seconds(end_beat);
    let total_frames = (total_secs * sample_rate as f64).ceil() as usize;
    if total_frames == 0 {
        return None;
    }

    let block_size = 512usize;
    let mut core = AudioGraphCore::new(sample_rate as f32, block_size);
    core.sync_processors_from_project(&solo_proj);
    core.playhead_beat = 0.0;
    core.is_playing = true;

    let mut out_l = Vec::with_capacity(total_frames);
    let mut out_r = Vec::with_capacity(total_frames);
    let mut blk_l = vec![0.0_f32; block_size];
    let mut blk_r = vec![0.0_f32; block_size];
    let zero_in = vec![0.0_f32; block_size];

    let mut rendered = 0usize;
    while rendered < total_frames {
        let n = (total_frames - rendered).min(block_size);
        blk_l[..n].fill(0.0);
        blk_r[..n].fill(0.0);
        core.render_block(
            &solo_proj,
            &zero_in[..n],
            &zero_in[..n],
            &[],
            &mut blk_l[..n],
            &mut blk_r[..n],
        );
        out_l.extend_from_slice(&blk_l[..n]);
        out_r.extend_from_slice(&blk_r[..n]);
        rendered += n;
    }

    Some(AudioBuffer::from_stereo(sample_rate, out_l, out_r, None))
}

pub fn render_master_offline(
    project: &Project,
    output_path: &Path,
    sample_rate: u32,
    bit_depth: BitDepth,
    start_beat: f64,
    end_beat: f64,
) -> Result<BounceReport, WavIoError> {
    let opts = MasteringOptions {
        format: ExportFormat::Wav,
        sample_rate,
        bit_depth,
        start_beat,
        end_beat,
        normalize_target_lufs: None,
        true_peak_ceiling_dbtp: -0.2,
        tpdf_dither: bit_depth != BitDepth::Float32,
    };
    render_master_with_options(project, output_path, &opts)
}

pub fn render_master_with_options(
    project: &Project,
    output_path: &Path,
    opts: &MasteringOptions,
) -> Result<BounceReport, WavIoError> {
    let mut render_proj = project.clone();
    render_proj.loop_enabled = false;
    render_proj.metronome_enabled = false;
    // If mastering LUFS normalization is active, bypass the实时 soft clipper during capture
    // so the offline 2.5 ms lookahead True-Peak limiter handles ceiling cleanly!
    if opts.normalize_target_lufs.is_some() {
        render_proj.master.safety_limiter = false;
    }

    let end_beat = opts.end_beat.max(opts.start_beat + 1.0);
    let dur_secs = render_proj.beat_range_duration_secs(opts.start_beat, end_beat) + 0.75;
    let total_frames = (dur_secs * opts.sample_rate as f64).ceil() as usize;

    let block_size = 512usize;
    let mut core = AudioGraphCore::new(opts.sample_rate as f32, block_size);
    core.sync_processors_from_project(&render_proj);
    core.playhead_beat = opts.start_beat;
    core.is_playing = true;

    let mut analyzer = LoudnessAndSpectrumAnalyzer::new(opts.sample_rate as f32);
    let mut out_l = Vec::with_capacity(total_frames);
    let mut out_r = Vec::with_capacity(total_frames);
    let mut blk_l = vec![0.0_f32; block_size];
    let mut blk_r = vec![0.0_f32; block_size];
    let zero_in = vec![0.0_f32; block_size];

    let mut last_st_lufs = -70.0_f32;
    let mut rendered = 0usize;

    while rendered < total_frames {
        let n = (total_frames - rendered).min(block_size);
        blk_l[..n].fill(0.0);
        blk_r[..n].fill(0.0);
        core.render_block(
            &render_proj,
            &zero_in[..n],
            &zero_in[..n],
            &[],
            &mut blk_l[..n],
            &mut blk_r[..n],
        );
        let snap = analyzer.analyze_block(&blk_l[..n], &blk_r[..n]);
        last_st_lufs = snap.short_term_lufs;
        out_l.extend_from_slice(&blk_l[..n]);
        out_r.extend_from_slice(&blk_r[..n]);
        rendered += n;
    }

    // Apply Mastering BS.1770-4 Integrated Loudness Normalization + True-Peak Lookahead Ceiling!
    let (integrated_lufs, true_peak_dbtp) = if opts.normalize_target_lufs.is_some()
        || opts.true_peak_ceiling_dbtp < -0.05
    {
        master_normalize_and_limit_true_peak(
            opts.sample_rate,
            &mut out_l,
            &mut out_r,
            opts.normalize_target_lufs,
            opts.true_peak_ceiling_dbtp,
        )
    } else {
        (
            measure_integrated_lufs(opts.sample_rate, &out_l, &out_r),
            measure_true_peak_dbtp(&out_l, &out_r),
        )
    };

    let sample_peak = out_l
        .iter()
        .chain(out_r.iter())
        .fold(0.0_f32, |a, &s| a.max(s.abs()));
    let peak_db = ferric_dsp::lin_to_db(sample_peak);

    write_exported_audio_file(output_path, opts, &out_l, &out_r)?;
    Ok(BounceReport {
        output_files: vec![output_path.to_path_buf()],
        duration_secs: dur_secs,
        peak_db,
        true_peak_dbtp,
        integrated_lufs,
        short_term_lufs: last_st_lufs,
    })
}

pub fn render_all_stems_offline(
    project: &Project,
    output_dir: &Path,
    sample_rate: u32,
    bit_depth: BitDepth,
    start_beat: f64,
    end_beat: f64,
) -> Result<BounceReport, WavIoError> {
    let opts = MasteringOptions {
        format: ExportFormat::Wav,
        sample_rate,
        bit_depth,
        start_beat,
        end_beat,
        normalize_target_lufs: None,
        true_peak_ceiling_dbtp: -0.1,
        tpdf_dither: bit_depth != BitDepth::Float32,
    };
    render_all_stems_with_options(project, output_dir, &opts)
}

pub fn render_all_stems_with_options(
    project: &Project,
    output_dir: &Path,
    opts: &MasteringOptions,
) -> Result<BounceReport, WavIoError> {
    std::fs::create_dir_all(output_dir)?;
    let mut files = Vec::new();
    let mut max_peak_db = -96.0_f32;
    let mut max_tp_dbtp = -96.0_f32;
    let mut last_int_lufs = -70.0_f32;
    let mut last_lufs = -70.0_f32;
    let mut dur_secs = 0.0;

    // Note: Stems should preserve relative mix balance unless user explicitly normalized stems;
    // keep True-Peak protection & TPDF dithering + chosen WAV/FLAC container!
    let mut stem_opts = *opts;
    stem_opts.normalize_target_lufs = None;

    let track_ids: Vec<(TrackId, String)> = project
        .tracks
        .iter()
        .map(|t| (t.id, t.name.clone()))
        .collect();

    for (idx, (tid, name)) in track_ids.into_iter().enumerate() {
        let mut solo_proj = project.clone();
        for t in &mut solo_proj.tracks {
            t.solo = t.id == tid || t.parent_folder_id == Some(tid);
            t.mute = false;
        }
        let safe_name: String = name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let path = output_dir.join(format!(
            "{:02}_{}.{}",
            idx + 1,
            safe_name,
            stem_opts.format.extension()
        ));
        let rep = render_master_with_options(&solo_proj, &path, &stem_opts)?;
        max_peak_db = max_peak_db.max(rep.peak_db);
        max_tp_dbtp = max_tp_dbtp.max(rep.true_peak_dbtp);
        last_int_lufs = rep.integrated_lufs;
        last_lufs = rep.short_term_lufs;
        dur_secs = rep.duration_secs;
        files.extend(rep.output_files);
    }

    Ok(BounceReport {
        output_files: files,
        duration_secs: dur_secs,
        peak_db: max_peak_db,
        true_peak_dbtp: max_tp_dbtp,
        integrated_lufs: last_int_lufs,
        short_term_lufs: last_lufs,
    })
}

/// Saves a complete FerricDAW Session Bundle: writes any recorded or generated long audio takes
/// losslessly (32-bit float WAV) into `<ProjectStem>_Audio/` beside the `.ferric`/`.json` project file!
pub fn save_project_bundle(project: &mut Project, project_file: &Path) -> Result<(), String> {
    let parent = project_file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let stem = project_file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("session");
    let audio_dir = parent.join(format!("{}_Audio", stem));

    let persist_buf = |buf: &mut AudioBuffer, label_id: &str| -> Result<(), String> {
        if buf.len_frames() == 0 {
            return Ok(());
        }
        let needs_write = match &buf.source_path {
            None => true,
            Some(existing) => !existing.exists(),
        };
        if needs_write {
            std::fs::create_dir_all(&audio_dir).map_err(|e| e.to_string())?;
            let wav_path = audio_dir.join(format!("{}.wav", label_id));
            write_wav_file(
                &wav_path,
                buf.sample_rate,
                BitDepth::Float32,
                &buf.left,
                &buf.right,
            )
            .map_err(|e| e.to_string())?;
            buf.source_path = Some(wav_path);
        }
        Ok(())
    };

    for track in &mut project.tracks {
        for clip in &mut track.clips {
            for take in &mut clip.takes {
                let tid = take.id.to_string();
                if let ferric_model::TakeContent::Audio { buffer } = &mut take.content {
                    persist_buf(buffer, &format!("take_{}", tid))?;
                }
            }
        }
        for slot in track.session_slots.iter_mut().flatten() {
            let sid = slot.id.to_string();
            if let ferric_model::TakeContent::Audio { buffer } = &mut slot.content {
                persist_buf(buffer, &format!("slot_{}", sid))?;
            }
        }
        for fx in &mut track.fx_chain {
            let fid = fx.id.to_string();
            if let Some(ref mut sbuf) = fx.sampler_buffer {
                persist_buf(sbuf, &format!("sampler_{}", fid))?;
            }
        }
    }

    project
        .save_to_file(project_file)
        .map_err(|e| e.to_string())
}

/// Loads a `.ferric` or `.json` project and automatically re-hydrates all external `AudioBuffer` WAV/FLAC/MP3/OGG files!
pub fn load_project_bundle(
    project_file: &Path,
    target_sample_rate: u32,
) -> Result<Project, String> {
    let mut project = Project::load_from_file(project_file).map_err(|e| e.to_string())?;
    let base_dir = project_file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let stem = project_file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("session");
    let companion_audio_dir = base_dir.join(format!("{}_Audio", stem));

    let resolve_and_load = |buf: &mut AudioBuffer| {
        if buf.len_frames() > 0 {
            return;
        }
        let Some(ref src) = buf.source_path.clone() else {
            return;
        };
        let candidate = if src.exists() {
            Some(src.clone())
        } else if base_dir.join(src).exists() {
            Some(base_dir.join(src))
        } else {
            src.file_name()
                .map(|fname| companion_audio_dir.join(fname))
                .filter(|p| p.exists())
        };
        if let Some(valid_path) = candidate {
            if let Ok(loaded) = import_audio_file(&valid_path, target_sample_rate) {
                *buf = loaded;
            }
        }
    };

    for track in &mut project.tracks {
        for clip in &mut track.clips {
            for take in &mut clip.takes {
                if let ferric_model::TakeContent::Audio { buffer } = &mut take.content {
                    resolve_and_load(buffer);
                }
            }
        }
        for slot in track.session_slots.iter_mut().flatten() {
            if let ferric_model::TakeContent::Audio { buffer } = &mut slot.content {
                resolve_and_load(buffer);
            }
        }
        for fx in &mut track.fx_chain {
            if let Some(ref mut sbuf) = fx.sampler_buffer {
                resolve_and_load(sbuf);
                if fx.sampler_slices.is_empty() && sbuf.len_frames() > 0 {
                    fx.sampler_slices = sbuf.detect_transients(16);
                }
            }
        }
    }

    Ok(project)
}
