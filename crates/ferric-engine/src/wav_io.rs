use crate::graph::AudioGraphCore;
use ferric_dsp::LoudnessAndSpectrumAnalyzer;
use ferric_model::{AudioBuffer, Project, TrackId};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WavIoError {
    #[error("WAV error: {0}")]
    Hound(#[from] hound::Error),
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

#[derive(Debug, Clone)]
pub struct BounceReport {
    pub output_files: Vec<PathBuf>,
    pub duration_secs: f64,
    pub peak_db: f32,
    pub short_term_lufs: f32,
}

/// Loads any mono or stereo 8/16/24/32-bit PCM or 32-bit Float WAV file into an `AudioBuffer`,
/// resampling cleanly to `target_sample_rate` if needed.
pub fn import_wav_file(path: &Path, target_sample_rate: u32) -> Result<AudioBuffer, WavIoError> {
    let mut reader = WavReader::open(path)?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let src_sr = spec.sample_rate.max(8_000);

    let interleaved: Vec<f32> = match spec.sample_format {
        SampleFormat::Float => reader
            .samples::<f32>()
            .map(|s| s.unwrap_or(0.0))
            .collect(),
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

    if src_sr == target_sample_rate || frames < 2 {
        return Ok(AudioBuffer::from_stereo(
            target_sample_rate,
            src_l,
            src_r,
            Some(path.to_path_buf()),
        ));
    }

    // Linear resampling to engine sample rate
    let ratio = src_sr as f64 / target_sample_rate as f64;
    let out_frames = ((frames as f64) / ratio).ceil() as usize;
    let tmp = AudioBuffer::from_stereo(src_sr, src_l, src_r, None);
    let mut out_l = Vec::with_capacity(out_frames);
    let mut out_r = Vec::with_capacity(out_frames);
    for i in 0..out_frames {
        let (l, r) = tmp.sample_interpolated(i as f64 * ratio);
        out_l.push(l);
        out_r.push(r);
    }

    Ok(AudioBuffer::from_stereo(
        target_sample_rate,
        out_l,
        out_r,
        Some(path.to_path_buf()),
    ))
}

pub fn write_wav_file(
    path: &Path,
    sample_rate: u32,
    bit_depth: BitDepth,
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
    match bit_depth {
        BitDepth::Pcm16 => {
            for i in 0..frames {
                let l = (left[i].clamp(-1.0, 1.0) * 32767.0) as i16;
                let r = (right[i].clamp(-1.0, 1.0) * 32767.0) as i16;
                writer.write_sample(l)?;
                writer.write_sample(r)?;
            }
        }
        BitDepth::Pcm24 => {
            let scale = 8_388_607.0_f32;
            for i in 0..frames {
                let l = (left[i].clamp(-1.0, 1.0) * scale) as i32;
                let r = (right[i].clamp(-1.0, 1.0) * scale) as i32;
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

/// Faster-than-realtime offline Master Mix bounce!
pub fn render_master_offline(
    project: &Project,
    output_path: &Path,
    sample_rate: u32,
    bit_depth: BitDepth,
    start_beat: f64,
    end_beat: f64,
) -> Result<BounceReport, WavIoError> {
    let mut render_proj = project.clone();
    render_proj.loop_enabled = false;
    render_proj.metronome_enabled = false;

    let dur_beats = (end_beat - start_beat).max(1.0);
    let dur_secs = render_proj.beats_to_seconds(dur_beats) + 0.75; // +750ms reverb tail
    let total_frames = (dur_secs * sample_rate as f64).ceil() as usize;

    let block_size = 512usize;
    let mut core = AudioGraphCore::new(sample_rate as f32, block_size);
    core.sync_processors_from_project(&render_proj);
    core.playhead_beat = start_beat;
    core.is_playing = true;

    let mut analyzer = LoudnessAndSpectrumAnalyzer::new(sample_rate as f32);
    let mut out_l = Vec::with_capacity(total_frames);
    let mut out_r = Vec::with_capacity(total_frames);
    let mut blk_l = vec![0.0_f32; block_size];
    let mut blk_r = vec![0.0_f32; block_size];
    let zero_in = vec![0.0_f32; block_size];

    let mut max_peak_db = -96.0_f32;
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
        max_peak_db = max_peak_db.max(snap.peak_l_db).max(snap.peak_r_db);
        last_st_lufs = snap.short_term_lufs;
        out_l.extend_from_slice(&blk_l[..n]);
        out_r.extend_from_slice(&blk_r[..n]);
        rendered += n;
    }

    write_wav_file(output_path, sample_rate, bit_depth, &out_l, &out_r)?;
    Ok(BounceReport {
        output_files: vec![output_path.to_path_buf()],
        duration_secs: dur_secs,
        peak_db: max_peak_db,
        short_term_lufs: last_st_lufs,
    })
}

/// Faster-than-realtime Multi-Track Stem Export (one WAV per track in `output_dir`).
pub fn render_all_stems_offline(
    project: &Project,
    output_dir: &Path,
    sample_rate: u32,
    bit_depth: BitDepth,
    start_beat: f64,
    end_beat: f64,
) -> Result<BounceReport, WavIoError> {
    std::fs::create_dir_all(output_dir)?;
    let mut files = Vec::new();
    let mut max_peak_db = -96.0_f32;
    let mut last_lufs = -70.0_f32;
    let mut dur_secs = 0.0;

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
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        let path = output_dir.join(format!("{:02}_{}.wav", idx + 1, safe_name));
        let rep = render_master_offline(&solo_proj, &path, sample_rate, bit_depth, start_beat, end_beat)?;
        max_peak_db = max_peak_db.max(rep.peak_db);
        last_lufs = rep.short_term_lufs;
        dur_secs = rep.duration_secs;
        files.extend(rep.output_files);
    }

    Ok(BounceReport {
        output_files: files,
        duration_secs: dur_secs,
        peak_db: max_peak_db,
        short_term_lufs: last_lufs,
    })
}
