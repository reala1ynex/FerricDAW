use std::io::{self, BufWriter, Seek, Write};
use std::num::{NonZeroU32, NonZeroU8};
use std::path::Path;

use rusty_mp3::{Error as Mp3Error, Mp3Encoder, Mp3EncoderConfig};

use tempfile::NamedTempFile;
use vorbis_rs::{VorbisBitrateManagementStrategy, VorbisEncoderBuilder};

fn output_temp(path: &Path) -> io::Result<NamedTempFile> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    NamedTempFile::new_in(parent)
}

fn check_input(sample_rate: u32, left: &[f32], right: &[f32]) -> io::Result<NonZeroU32> {
    if left.is_empty() || left.len() != right.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Expected nonempty stereo audio with equal channel lengths",
        ));
    }
    NonZeroU32::new(sample_rate)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid sample rate"))
}

/// Export standard stereo MPEG Layer III with the bundled encoder.
pub fn write_mp3_stereo_file(
    path: &Path,
    sample_rate: u32,
    left: &[f32],
    right: &[f32],
) -> io::Result<()> {
    check_input(sample_rate, left, right)?;
    if !matches!(sample_rate, 44_100 | 48_000) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Choose 44,100 or 48,000 Hz for MP3 export.",
        ));
    }

    let mut encoded = output_temp(path)?;
    {
        let mut writer = BufWriter::new(encoded.as_file_mut());
        let mut encoder = Mp3Encoder::new(Mp3EncoderConfig {
            bitrate_kbps: 192,
            vbr_quality: None,
        });
        let mut interleaved = Vec::with_capacity(2304);
        for (l, r) in left.chunks(1152).zip(right.chunks(1152)) {
            interleaved.clear();
            interleaved.extend(l.iter().zip(r).flat_map(|(&l, &r)| [l, r]));
            encoder
                .push_pcm_f32(&interleaved, 2, sample_rate)
                .map_err(io::Error::other)?;
            write_mp3_packets(&mut encoder, &mut writer)?;
        }
        encoder.finish();
        write_mp3_packets(&mut encoder, &mut writer)?;

        // The Info frame starts as a placeholder. Patch in the final duration
        // and padding so players can discard encoder delay and tail samples.
        let info = encoder
            .lame_tag_frame()
            .ok_or_else(|| io::Error::other("MP3 encoder did not produce stream metadata"))?;
        writer.rewind()?;
        writer.write_all(info)?;
        writer.flush()?;
    }
    encoded.persist(path).map_err(|e| e.error)?;
    Ok(())
}

fn write_mp3_packets(encoder: &mut Mp3Encoder, writer: &mut impl Write) -> io::Result<()> {
    loop {
        match encoder.next_packet() {
            Ok(packet) => writer.write_all(&packet)?,
            Err(Mp3Error::Again | Mp3Error::Eof) => return Ok(()),
            Err(error) => return Err(io::Error::other(error)),
        }
    }
}

/// Export interoperable Ogg Vorbis, using a bundled encoder at quality 7.
pub fn write_ogg_vorbis_stereo_file(
    path: &Path,
    sample_rate: u32,
    left: &[f32],
    right: &[f32],
) -> io::Result<()> {
    let rate = check_input(sample_rate, left, right)?;
    let mut encoded = output_temp(path)?;
    {
        let mut encoder = VorbisEncoderBuilder::new_with_serial(
            rate,
            NonZeroU8::new(2).unwrap(),
            BufWriter::new(encoded.as_file_mut()),
            1,
        )
        .bitrate_management_strategy(VorbisBitrateManagementStrategy::QualityVbr {
            target_quality: 0.7,
        })
        .build()
        .map_err(io::Error::other)?;
        for (l, r) in left.chunks(1024).zip(right.chunks(1024)) {
            encoder
                .encode_audio_block([l, r])
                .map_err(io::Error::other)?;
        }
        encoder.finish().map_err(io::Error::other)?.flush()?;
    }
    encoded.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo_tones() -> (Vec<f32>, Vec<f32>) {
        let tone = |f: f32| {
            (0..48_000)
                .map(|i| (i as f32 * f * std::f32::consts::TAU / 48_000.0_f32).sin() * 0.2)
                .collect()
        };
        (tone(440.0_f32), tone(660.0_f32))
    }

    fn check_independent_decode(path: &Path) {
        let bytes = std::fs::read(path).unwrap();
        assert!(!bytes.windows(9).any(|b| b == b"FERRICWAV"));
        let decoded = crate::wav_io::import_audio_file(path, 48_000).unwrap();
        assert!(decoded.len_frames().abs_diff(48_000) < 2500);

        // The two channels must preserve their own tone. This detects silence,
        // truncated audio, swapped/duplicated channels and wrong sample rate.
        let amplitude = |samples: &[f32], hz: f32| -> f32 {
            let band = &samples[4000..44_000];
            let (mut re, mut im) = (0.0, 0.0);
            for (idx, &sample) in band.iter().enumerate() {
                let phase = idx as f32 * hz * std::f32::consts::TAU / 48_000.0;
                re += sample * phase.cos();
                im += sample * phase.sin();
            }
            re.hypot(im) * 2.0 / band.len() as f32
        };
        for (samples, expected, other) in [
            (&decoded.left, 440.0, 660.0),
            (&decoded.right, 660.0, 440.0),
        ] {
            assert!(amplitude(samples, expected) > 0.15);
            assert!(amplitude(samples, other) < 0.03);
        }
    }

    #[test]
    fn ogg_vorbis_export_decodes_as_standard_stereo() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("stereo.ogg");
        let (left, right) = stereo_tones();
        write_ogg_vorbis_stereo_file(&path, 48_000, &left, &right).unwrap();
        check_independent_decode(&path);
    }

    #[test]
    fn bundled_mp3_export_decodes_as_standard_stereo() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("stereo.mp3");
        let (left, right) = stereo_tones();
        write_mp3_stereo_file(&path, 48_000, &left, &right).unwrap();
        check_independent_decode(&path);
    }

    #[test]
    fn unsupported_mp3_rate_preserves_existing_destination() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("existing.mp3");
        std::fs::write(&path, b"existing file").unwrap();
        let error = write_mp3_stereo_file(&path, 96_000, &[0.0], &[0.0]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(std::fs::read(&path).unwrap(), b"existing file");
    }
}
