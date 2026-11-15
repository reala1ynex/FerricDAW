use crate::biquad::StereoBiquad;
use ferric_model::AudioBuffer;
use rustfft::{num_complex::Complex, FftPlanner};
use std::f32::consts::PI;

pub struct FourStemSplit {
    pub drums_percussion: AudioBuffer,
    pub sub_bass: AudioBuffer,
    pub mids_keys_vocals: AudioBuffer,
    pub highs_air: AudioBuffer,
}

/// Offline 4-Stem Harmonic-Percussive STFT + Linkwitz-Riley Audio Splitter!
///
/// Uses Fitzgerald's Median-Filter Harmonic-Percussive Source Separation (HPSS) in the 2048-bin
/// Hann-windowed STFT domain (`75%` overlap, exact COLA synthesis) to isolate **Drums / Transients**
/// from **Sustained Harmonic Content**, then splits the harmonic stem through phase-coherent
/// 4th-order Linkwitz-Riley crossovers (`185 Hz` and `2800 Hz`) into **Sub/Bass**, **Mids/Keys/Vocals**,
/// and **Highs/Air**!
pub fn split_audio_to_4_stems(source: &AudioBuffer) -> FourStemSplit {
    let sr = source.sample_rate.max(8_000);
    let total_frames = source.len_frames();
    if total_frames == 0 {
        let empty = AudioBuffer::from_stereo(sr, Vec::new(), Vec::new(), None);
        return FourStemSplit {
            drums_percussion: empty.clone(),
            sub_bass: empty.clone(),
            mids_keys_vocals: empty.clone(),
            highs_air: empty,
        };
    }

    const N_FFT: usize = 2048;
    const HOP: usize = 512; // 75% overlap -> sum(Hann^2) = 1.5 constant overlap-add!
    const BINS: usize = N_FFT / 2 + 1;
    const MED_TIME_HALF: isize = 4; // 9-frame (~96 ms) temporal median for sustained harmonics
    const MED_FREQ_HALF: isize = 6; // 13-bin (~300 Hz) spectral median for broadband percussion

    let pad = N_FFT;
    let padded_len = total_frames + pad * 2;
    let mut pad_l = vec![0.0_f32; padded_len];
    let mut pad_r = vec![0.0_f32; padded_len];
    pad_l[pad..pad + total_frames].copy_from_slice(&source.left[..total_frames]);
    pad_r[pad..pad + total_frames].copy_from_slice(&source.right[..total_frames]);

    let num_stft_frames = (padded_len.saturating_sub(N_FFT)) / HOP + 1;

    let window: Vec<f32> = (0..N_FFT)
        .map(|n| 0.5 * (1.0 - (2.0 * PI * n as f32 / N_FFT as f32).cos()))
        .collect();

    let mut planner = FftPlanner::<f32>::new();
    let fft_fwd = planner.plan_fft_forward(N_FFT);
    let fft_inv = planner.plan_fft_inverse(N_FFT);
    let mut scratch = vec![
        Complex::new(0.0_f32, 0.0_f32);
        fft_fwd
            .get_inplace_scratch_len()
            .max(fft_inv.get_inplace_scratch_len())
    ];

    let mut spec_l = vec![Complex::new(0.0_f32, 0.0_f32); num_stft_frames * BINS];
    let mut spec_r = vec![Complex::new(0.0_f32, 0.0_f32); num_stft_frames * BINS];
    let mut mag_mid = vec![0.0_f32; num_stft_frames * BINS];

    let mut buf_l = vec![Complex::new(0.0_f32, 0.0_f32); N_FFT];
    let mut buf_r = vec![Complex::new(0.0_f32, 0.0_f32); N_FFT];

    // 1. Forward STFT
    for t in 0..num_stft_frames {
        let base = t * HOP;
        for n in 0..N_FFT {
            let w = window[n];
            buf_l[n] = Complex::new(pad_l[base + n] * w, 0.0);
            buf_r[n] = Complex::new(pad_r[base + n] * w, 0.0);
        }
        fft_fwd.process_with_scratch(&mut buf_l, &mut scratch);
        fft_fwd.process_with_scratch(&mut buf_r, &mut scratch);

        let row = t * BINS;
        for k in 0..BINS {
            spec_l[row + k] = buf_l[k];
            spec_r[row + k] = buf_r[k];
            let ml = buf_l[k].norm();
            let mr = buf_r[k].norm();
            mag_mid[row + k] = 0.5 * (ml + mr);
        }
    }

    // 2. Median Filtering across Time (Harmonic) and Frequency (Percussive) + Wiener Soft Mask
    let mut harm_out_l = vec![0.0_f32; padded_len];
    let mut harm_out_r = vec![0.0_f32; padded_len];
    let mut perc_out_l = vec![0.0_f32; padded_len];
    let mut perc_out_r = vec![0.0_f32; padded_len];
    let mut win_norm = vec![0.0_f32; padded_len];

    let mut h_buf_l = vec![Complex::new(0.0_f32, 0.0_f32); N_FFT];
    let mut h_buf_r = vec![Complex::new(0.0_f32, 0.0_f32); N_FFT];
    let mut p_buf_l = vec![Complex::new(0.0_f32, 0.0_f32); N_FFT];
    let mut p_buf_r = vec![Complex::new(0.0_f32, 0.0_f32); N_FFT];

    let mut t_scratch = [0.0_f32; (MED_TIME_HALF as usize) * 2 + 1];
    let mut f_scratch = [0.0_f32; (MED_FREQ_HALF as usize) * 2 + 1];
    let inv_n = 1.0 / N_FFT as f32;

    for t in 0..num_stft_frames {
        let row = t * BINS;
        for k in 0..BINS {
            // Temporal median -> Harmonic estimate H(t, k)
            let mut tc = 0usize;
            for dt in -MED_TIME_HALF..=MED_TIME_HALF {
                let ti = (t as isize + dt).clamp(0, num_stft_frames as isize - 1) as usize;
                t_scratch[tc] = mag_mid[ti * BINS + k];
                tc += 1;
            }
            t_scratch[..tc].sort_unstable_by(|a, b| a.total_cmp(b));
            let h_est = t_scratch[tc / 2];

            // Spectral median -> Percussive estimate P(t, k)
            let mut fc = 0usize;
            for dk in -MED_FREQ_HALF..=MED_FREQ_HALF {
                let ki = (k as isize + dk).clamp(0, BINS as isize - 1) as usize;
                f_scratch[fc] = mag_mid[row + ki];
                fc += 1;
            }
            f_scratch[..fc].sort_unstable_by(|a, b| a.total_cmp(b));
            let p_est = f_scratch[fc / 2];

            // Wiener power-2 soft mask: M_H + M_P == 1.0 (lossless reconstruction!)
            let h2 = h_est * h_est;
            let p2 = p_est * p_est;
            let denom = h2 + p2 + 1e-16;
            let mask_h = h2 / denom;
            let mask_p = 1.0 - mask_h;

            let sl = spec_l[row + k];
            let sr_c = spec_r[row + k];
            h_buf_l[k] = sl * mask_h;
            h_buf_r[k] = sr_c * mask_h;
            p_buf_l[k] = sl * mask_p;
            p_buf_r[k] = sr_c * mask_p;
        }

        // Conjugate symmetry for real inverse FFT (`k = 1 .. N_FFT/2 - 1`)
        for k in 1..(N_FFT / 2) {
            let mirror = N_FFT - k;
            h_buf_l[mirror] = h_buf_l[k].conj();
            h_buf_r[mirror] = h_buf_r[k].conj();
            p_buf_l[mirror] = p_buf_l[k].conj();
            p_buf_r[mirror] = p_buf_r[k].conj();
        }

        fft_inv.process_with_scratch(&mut h_buf_l, &mut scratch);
        fft_inv.process_with_scratch(&mut h_buf_r, &mut scratch);
        fft_inv.process_with_scratch(&mut p_buf_l, &mut scratch);
        fft_inv.process_with_scratch(&mut p_buf_r, &mut scratch);

        let base = t * HOP;
        for n in 0..N_FFT {
            let w = window[n];
            let scale = w * inv_n;
            harm_out_l[base + n] += h_buf_l[n].re * scale;
            harm_out_r[base + n] += h_buf_r[n].re * scale;
            perc_out_l[base + n] += p_buf_l[n].re * scale;
            perc_out_r[base + n] += p_buf_r[n].re * scale;
            win_norm[base + n] += w * w;
        }
    }

    // Normalize COLA window & strip padding
    let mut drums_l = Vec::with_capacity(total_frames);
    let mut drums_r = Vec::with_capacity(total_frames);
    let mut harm_l = Vec::with_capacity(total_frames);
    let mut harm_r = Vec::with_capacity(total_frames);

    for i in 0..total_frames {
        let idx = pad + i;
        let inv_w = if win_norm[idx] > 1e-6 {
            1.0 / win_norm[idx]
        } else {
            1.0
        };
        drums_l.push(perc_out_l[idx] * inv_w);
        drums_r.push(perc_out_r[idx] * inv_w);
        harm_l.push(harm_out_l[idx] * inv_w);
        harm_r.push(harm_out_r[idx] * inv_w);
    }

    // 3. Split Harmonic Stem via 4th-order Linkwitz-Riley (24 dB/oct) into Sub/Bass, Mids/Keys, Highs/Air
    let sr_f = sr as f32;
    let mut lp_bass_1 = StereoBiquad::default();
    let mut lp_bass_2 = StereoBiquad::default();
    let mut hp_bass_1 = StereoBiquad::default();
    let mut hp_bass_2 = StereoBiquad::default();
    lp_bass_1.set_lowpass(sr_f, 185.0, 0.7071);
    lp_bass_2.set_lowpass(sr_f, 185.0, 0.7071);
    hp_bass_1.set_highpass(sr_f, 185.0, 0.7071);
    hp_bass_2.set_highpass(sr_f, 185.0, 0.7071);

    let mut lp_hi_1 = StereoBiquad::default();
    let mut lp_hi_2 = StereoBiquad::default();
    let mut hp_hi_1 = StereoBiquad::default();
    let mut hp_hi_2 = StereoBiquad::default();
    lp_hi_1.set_lowpass(sr_f, 2800.0, 0.7071);
    lp_hi_2.set_lowpass(sr_f, 2800.0, 0.7071);
    hp_hi_1.set_highpass(sr_f, 2800.0, 0.7071);
    hp_hi_2.set_highpass(sr_f, 2800.0, 0.7071);

    let mut bass_l = Vec::with_capacity(total_frames);
    let mut bass_r = Vec::with_capacity(total_frames);
    let mut mids_l = Vec::with_capacity(total_frames);
    let mut mids_r = Vec::with_capacity(total_frames);
    let mut highs_l = Vec::with_capacity(total_frames);
    let mut highs_r = Vec::with_capacity(total_frames);

    for i in 0..total_frames {
        let (hl, hr) = (harm_l[i], harm_r[i]);
        let (b1_l, b1_r) = lp_bass_1.process_stereo(hl, hr);
        let (bl, br) = lp_bass_2.process_stereo(b1_l, b1_r);

        let (mh1_l, mh1_r) = hp_bass_1.process_stereo(hl, hr);
        let (mh_l, mh_r) = hp_bass_2.process_stereo(mh1_l, mh1_r);

        let (m1_l, m1_r) = lp_hi_1.process_stereo(mh_l, mh_r);
        let (ml, mr) = lp_hi_2.process_stereo(m1_l, m1_r);

        let (h1_l, h1_r) = hp_hi_1.process_stereo(mh_l, mh_r);
        let (al, ar) = hp_hi_2.process_stereo(h1_l, h1_r);

        bass_l.push(bl);
        bass_r.push(br);
        mids_l.push(ml);
        mids_r.push(mr);
        highs_l.push(al);
        highs_r.push(ar);
    }

    FourStemSplit {
        drums_percussion: AudioBuffer::from_stereo(sr, drums_l, drums_r, None),
        sub_bass: AudioBuffer::from_stereo(sr, bass_l, bass_r, None),
        mids_keys_vocals: AudioBuffer::from_stereo(sr, mids_l, mids_r, None),
        highs_air: AudioBuffer::from_stereo(sr, highs_l, highs_r, None),
    }
}
