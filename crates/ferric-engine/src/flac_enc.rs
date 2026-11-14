//! Pure-Rust Zero-Dependency RFC 9639 Lossless Stereo FLAC Encoder!
//! Supports 16-bit and 24-bit PCM at arbitrary sample rates, Mid-Side decorrelation,
//! Fixed Linear Prediction (orders 0..=4), Rice entropy coding, CRC-8, CRC-16, and RFC 1321 MD5 audio signature.

use ferric_dsp::TpdfDither;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

const FLAC_BLOCK_SIZE: usize = 4096;

pub fn write_flac_stereo_file(
    path: &Path,
    sample_rate: u32,
    bits_per_sample: u8, // 16 or 24
    enable_tpdf_dither: bool,
    left: &[f32],
    right: &[f32],
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let bps = if bits_per_sample >= 24 { 24u8 } else { 16u8 };
    let total_frames = left.len().min(right.len());

    let mut dither = TpdfDither::default();
    let mut pcm_l = Vec::with_capacity(total_frames);
    let mut pcm_r = Vec::with_capacity(total_frames);
    let mut md5 = Md5Hasher::new();

    for i in 0..total_frames {
        let ql = dither.quantize_sample(left[i], bps, enable_tpdf_dither);
        let qr = dither.quantize_sample(right[i], bps, enable_tpdf_dither);
        pcm_l.push(ql);
        pcm_r.push(qr);

        // FLAC MD5 hashes little-endian signed interleaved PCM bytes!
        if bps == 16 {
            md5.update(&(ql as i16).to_le_bytes());
            md5.update(&(qr as i16).to_le_bytes());
        } else {
            let bl = ql.to_le_bytes();
            let br = qr.to_le_bytes();
            md5.update(&bl[0..3]);
            md5.update(&br[0..3]);
        }
    }
    let md5_digest = md5.finalize();

    let file = File::create(path)?;
    let mut out = BufWriter::new(file);

    // 1. Magic marker "fLaC"
    out.write_all(b"fLaC")?;

    // 2. Last-Metadata-Block Flag (1) | Block Type 0 (STREAMINFO), Length = 34 bytes
    out.write_all(&[0x80, 0x00, 0x00, 34])?;

    let min_blk = FLAC_BLOCK_SIZE as u16;
    let max_blk = FLAC_BLOCK_SIZE as u16;
    let mut si = [0u8; 34];
    si[0..2].copy_from_slice(&min_blk.to_be_bytes());
    si[2..4].copy_from_slice(&max_blk.to_be_bytes());
    // min_framesize (24-bit = 0 unknown), max_framesize (24-bit = 0 unknown) -> si[4..10] = 0
    // Packed 64-bit: [sample_rate: 20b][channels-1: 3b][bps-1: 5b][total_samples: 36b]
    let sr20 = (sample_rate.clamp(1, 655_350) as u64) & 0xF_FFFF;
    let ch3 = 1u64; // 2 channels -> (2 - 1) = 1
    let bps5 = ((bps - 1) as u64) & 0x1F;
    let tot36 = (total_frames as u64) & 0xF_FFFF_FFFF;
    let packed = (sr20 << 44) | (ch3 << 41) | (bps5 << 36) | tot36;
    si[10..18].copy_from_slice(&packed.to_be_bytes());
    si[18..34].copy_from_slice(&md5_digest);
    out.write_all(&si)?;

    // 3. Encode Audio Frames
    let mut frame_buf = Vec::with_capacity(FLAC_BLOCK_SIZE * 8);
    let mut offset = 0usize;
    let mut frame_number = 0u64;

    while offset < total_frames {
        let n = (total_frames - offset).min(FLAC_BLOCK_SIZE);
        encode_flac_frame(
            &pcm_l[offset..offset + n],
            &pcm_r[offset..offset + n],
            sample_rate,
            bps,
            frame_number,
            &mut frame_buf,
        );
        out.write_all(&frame_buf)?;
        offset += n;
        frame_number += 1;
    }

    out.flush()?;
    Ok(())
}

fn encode_flac_frame(
    left: &[i32],
    right: &[i32],
    sample_rate: u32,
    bps: u8,
    frame_number: u64,
    out_bytes: &mut Vec<u8>,
) {
    out_bytes.clear();
    let n = left.len();
    let mut bw = BitWriter::with_capacity(n * 8 + 64);

    // Decide Independent Stereo (0b0001) vs Left-Side (0b1000) vs Mid-Side (0b1010)
    // Mid-Side (`mid = (L+R)>>1`, `side = L-R`, side needs `bps + 1` bits!) compresses stereo mixes ~25% tighter!
    let mut sum_lr = 0u64;
    let mut sum_ms = 0u64;
    for i in 0..n {
        let l = left[i] as i64;
        let r = right[i] as i64;
        let side = l - r;
        let mid = (l + r) >> 1;
        sum_lr += l.unsigned_abs() + r.unsigned_abs();
        sum_ms += mid.unsigned_abs() + side.unsigned_abs();
    }
    let use_mid_side = sum_ms * 10 < sum_lr * 9;

    // --- Frame Header ---
    // Sync code (14 bits = 0b11111111111110) + Reserved (0) + Blocking strategy (0 = fixed blocksize)
    bw.write_bits(0x3FFE, 14);
    bw.write_bits(0, 1);
    bw.write_bits(0, 1);

    // Block size code: 0b1100 = 4096 if n == 4096, else 0b0111 = 16-bit (n - 1) at end of header
    let (bs_code, explicit_bs16) = if n == 4096 {
        (0b1100_u64, false)
    } else {
        (0b0111_u64, true)
    };
    bw.write_bits(bs_code, 4);

    // Sample rate code: 0b1001 = 44.1kHz, 0b1010 = 48kHz, 0b1011 = 96kHz, else 0b1101 = 16-bit Hz at end of header
    let (sr_code, explicit_sr16) = match sample_rate {
        44_100 => (0b1001_u64, false),
        48_000 => (0b1010_u64, false),
        96_000 => (0b1011_u64, false),
        _ => (0b1101_u64, true),
    };
    bw.write_bits(sr_code, 4);

    // Channel assignment: 0b0001 = L/R independent (2 ch), 0b1010 = Mid/Side stereo
    let ch_code = if use_mid_side { 0b1010_u64 } else { 0b0001_u64 };
    bw.write_bits(ch_code, 4);

    // Sample size code: 0b100 = 16-bit, 0b110 = 24-bit
    let bps_code = if bps == 24 { 0b110_u64 } else { 0b100_u64 };
    bw.write_bits(bps_code, 3);
    bw.write_bits(0, 1); // Reserved 0

    // UTF-8 coded frame number
    write_utf8_u64(&mut bw, frame_number);

    if explicit_bs16 {
        bw.write_bits((n - 1) as u64, 16);
    }
    if explicit_sr16 {
        bw.write_bits(sample_rate.min(65535) as u64, 16);
    }

    // Header CRC-8 (polynomial 0x07)
    bw.align_to_byte();
    let crc8 = compute_crc8(bw.bytes());
    bw.write_bits(crc8 as u64, 8);

    // --- Subframes ---
    if use_mid_side {
        let mut mid = Vec::with_capacity(n);
        let mut side = Vec::with_capacity(n);
        for i in 0..n {
            let l = left[i] as i64;
            let r = right[i] as i64;
            let s = l - r;
            // RFC 9639 §4.2: mid = ((L + R) >> 1) in integer math; decoder reconstructs:
            // mid2 = (mid << 1) | (side & 1); L = (mid2 + side) >> 1; R = (mid2 - side) >> 1
            let m = (l + r) >> 1;
            mid.push(m);
            side.push(s);
        }
        encode_subframe(&mut bw, &mid, bps);
        encode_subframe(&mut bw, &side, bps + 1); // Side channel has +1 bit!
    } else {
        let ch0: Vec<i64> = left.iter().map(|&x| x as i64).collect();
        let ch1: Vec<i64> = right.iter().map(|&x| x as i64).collect();
        encode_subframe(&mut bw, &ch0, bps);
        encode_subframe(&mut bw, &ch1, bps);
    }

    // Zero-pad to byte boundary + Frame Footer CRC-16 (polynomial 0x8005)
    bw.align_to_byte();
    let crc16 = compute_crc16(bw.bytes());
    bw.write_bits(crc16 as u64, 16);

    out_bytes.extend_from_slice(bw.bytes());
}

fn encode_subframe(bw: &mut BitWriter, samples: &[i64], bps: u8) {
    let n = samples.len();
    if n == 0 {
        return;
    }

    // Check Constant Subframe (e.g. digital silence or DC)
    let first = samples[0];
    if samples.iter().all(|&s| s == first) {
        // Subframe header: [0][000000 = CONSTANT][0 = no wasted bits]
        bw.write_bits(0, 1);
        bw.write_bits(0b00_0000, 6);
        bw.write_bits(0, 1);
        bw.write_signed_bits(first, bps);
        return;
    }

    // If block is very tiny (< 8 samples), emit Verbatim subframe (`000001`)
    if n < 8 {
        bw.write_bits(0, 1);
        bw.write_bits(0b00_0001, 6);
        bw.write_bits(0, 1);
        for &s in samples {
            bw.write_signed_bits(s, bps);
        }
        return;
    }

    // Evaluate Fixed Linear Prediction Orders 0..=4 and pick lowest L1 residual cost!
    let mut best_order = 0usize;
    let mut best_cost = u64::MAX;
    let mut best_residuals = Vec::new();
    let mut cand_res = Vec::with_capacity(n);

    for order in 0..=4usize {
        cand_res.clear();
        let mut cost = 0u64;
        for i in order..n {
            let pred = match order {
                0 => 0,
                1 => samples[i - 1],
                2 => 2 * samples[i - 1] - samples[i - 2],
                3 => 3 * samples[i - 1] - 3 * samples[i - 2] + samples[i - 3],
                _ => 4 * samples[i - 1] - 6 * samples[i - 2] + 4 * samples[i - 3] - samples[i - 4],
            };
            let r = samples[i] - pred;
            cost = cost.saturating_add(r.unsigned_abs());
            cand_res.push(r);
        }
        if cost < best_cost {
            best_cost = cost;
            best_order = order;
            best_residuals.clone_from(&cand_res);
        }
    }

    // Subframe header: [0][001xxx = FIXED order][0 = no wasted bits]
    bw.write_bits(0, 1);
    bw.write_bits(0b00_1000 | (best_order as u64), 6);
    bw.write_bits(0, 1);

    // Warm-up samples (`best_order` unencoded signed `bps`-bit samples)
    for &s in &samples[..best_order] {
        bw.write_signed_bits(s, bps);
    }

    // Rice Partitioned Residual Coding (Method `00` = 4-bit Rice parameter `0..14`, Partition Order `0` = single partition)
    bw.write_bits(0b00, 2); // Coding method 0 (RICE)
    bw.write_bits(0b0000, 4); // Partition order 0

    // Compute optimal Rice parameter `k` in `0..=14`
    let mut sum_u = 0u64;
    let mut max_u = 0u64;
    for &r in &best_residuals {
        let u = zigzag_encode(r);
        sum_u = sum_u.saturating_add(u);
        max_u = max_u.max(u);
    }
    let mean_u = (sum_u / (best_residuals.len().max(1) as u64)).max(1);
    let mut k = (63 - mean_u.leading_zeros()).min(14) as u8;
    // Prevent giant unary quotients on sudden transients!
    while k < 14 && (max_u >> k) > 2048 {
        k += 1;
    }

    if (max_u >> k) > 16_384 {
        // Escape code `0b1111` -> raw unencoded signed residuals of `esc_bits` width!
        let max_abs = best_residuals
            .iter()
            .map(|r| r.unsigned_abs())
            .max()
            .unwrap_or(1);
        let esc_bits = ((64 - max_abs.leading_zeros()) + 1).clamp(2, 31) as u8;
        bw.write_bits(0b1111, 4);
        bw.write_bits(esc_bits as u64, 5);
        for &r in &best_residuals {
            bw.write_signed_bits(r, esc_bits);
        }
    } else {
        bw.write_bits(k as u64, 4);
        let mask = (1u64 << k) - 1;
        for &r in &best_residuals {
            let u = zigzag_encode(r);
            let q = u >> k;
            let rem = u & mask;
            // Unary quotient: `q` zeros followed by a `1`
            bw.write_zeros(q as usize);
            bw.write_bits(1, 1);
            if k > 0 {
                bw.write_bits(rem, k);
            }
        }
    }
}

#[inline]
fn zigzag_encode(r: i64) -> u64 {
    if r >= 0 {
        (r as u64) << 1
    } else {
        ((-r as u64) << 1) - 1
    }
}

fn write_utf8_u64(bw: &mut BitWriter, v: u64) {
    if v < 0x80 {
        bw.write_bits(v, 8);
    } else if v < 0x800 {
        bw.write_bits(0xC0 | (v >> 6), 8);
        bw.write_bits(0x80 | (v & 0x3F), 8);
    } else if v < 0x1_0000 {
        bw.write_bits(0xE0 | (v >> 12), 8);
        bw.write_bits(0x80 | ((v >> 6) & 0x3F), 8);
        bw.write_bits(0x80 | (v & 0x3F), 8);
    } else if v < 0x20_0000 {
        bw.write_bits(0xF0 | (v >> 18), 8);
        bw.write_bits(0x80 | ((v >> 12) & 0x3F), 8);
        bw.write_bits(0x80 | ((v >> 6) & 0x3F), 8);
        bw.write_bits(0x80 | (v & 0x3F), 8);
    } else {
        bw.write_bits(0xF8 | (v >> 24), 8);
        bw.write_bits(0x80 | ((v >> 18) & 0x3F), 8);
        bw.write_bits(0x80 | ((v >> 12) & 0x3F), 8);
        bw.write_bits(0x80 | ((v >> 6) & 0x3F), 8);
        bw.write_bits(0x80 | (v & 0x3F), 8);
    }
}

struct BitWriter {
    buf: Vec<u8>,
    acc: u64,
    bits_in_acc: u8,
}

impl BitWriter {
    fn with_capacity(cap_bytes: usize) -> Self {
        Self {
            buf: Vec::with_capacity(cap_bytes),
            acc: 0,
            bits_in_acc: 0,
        }
    }

    #[inline]
    fn write_bits(&mut self, val: u64, num_bits: u8) {
        if num_bits == 0 {
            return;
        }
        let mask = if num_bits >= 64 {
            u64::MAX
        } else {
            (1u64 << num_bits) - 1
        };
        let v = val & mask;
        // Split if `bits_in_acc + num_bits > 64`
        if self.bits_in_acc + num_bits > 64 {
            let first = 64 - self.bits_in_acc;
            let rest = num_bits - first;
            self.write_bits(v >> rest, first);
            self.write_bits(v, rest);
            return;
        }
        self.acc = (self.acc << num_bits) | v;
        self.bits_in_acc += num_bits;
        while self.bits_in_acc >= 8 {
            self.bits_in_acc -= 8;
            let b = ((self.acc >> self.bits_in_acc) & 0xFF) as u8;
            self.buf.push(b);
        }
    }

    #[inline]
    fn write_signed_bits(&mut self, val: i64, num_bits: u8) {
        self.write_bits(val as u64, num_bits);
    }

    #[inline]
    fn write_zeros(&mut self, mut count: usize) {
        while count >= 32 {
            self.write_bits(0, 32);
            count -= 32;
        }
        if count > 0 {
            self.write_bits(0, count as u8);
        }
    }

    fn align_to_byte(&mut self) {
        if self.bits_in_acc > 0 {
            let pad = 8 - self.bits_in_acc;
            self.write_bits(0, pad);
        }
    }

    fn bytes(&self) -> &[u8] {
        &self.buf
    }
}

/// FLAC Header CRC-8 (`x^8 + x^2 + x^1 + x^0` = `0x07`, init `0x00`).
fn compute_crc8(bytes: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &b in bytes {
        crc ^= b;
        for _ in 0..8 {
            if (crc & 0x80) != 0 {
                crc = (crc << 1) ^ 0x07;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// FLAC Frame Footer CRC-16 (`x^16 + x^15 + x^2 + x^0` = `0x8005`, init `0x0000`).
fn compute_crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &b in bytes {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x8005;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// Compact RFC 1321 MD5 implementation exclusively for FLAC `STREAMINFO` lossless verification checksum!
struct Md5Hasher {
    state: [u32; 4],
    count_bytes: u64,
    buf: [u8; 64],
    buf_len: usize,
}

impl Md5Hasher {
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];

    fn new() -> Self {
        Self {
            state: [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476],
            count_bytes: 0,
            buf: [0u8; 64],
            buf_len: 0,
        }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.count_bytes += data.len() as u64;
        if self.buf_len > 0 {
            let need = 64 - self.buf_len;
            if data.len() < need {
                self.buf[self.buf_len..self.buf_len + data.len()].copy_from_slice(data);
                self.buf_len += data.len();
                return;
            }
            self.buf[self.buf_len..64].copy_from_slice(&data[..need]);
            let blk = self.buf;
            self.compress(&blk);
            self.buf_len = 0;
            data = &data[need..];
        }
        while data.len() >= 64 {
            let mut blk = [0u8; 64];
            blk.copy_from_slice(&data[..64]);
            self.compress(&blk);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buf_len = data.len();
        }
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut m = [0u32; 16];
        for i in 0..16 {
            m[i] = u32::from_le_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        let (mut a, mut b, mut c, mut d) =
            (self.state[0], self.state[1], self.state[2], self.state[3]);
        for i in 0..64 {
            let (f, g) = if i < 16 {
                ((b & c) | ((!b) & d), i)
            } else if i < 32 {
                ((d & b) | ((!d) & c), (5 * i + 1) % 16)
            } else if i < 48 {
                (b ^ c ^ d, (3 * i + 5) % 16)
            } else {
                (c ^ (b | (!d)), (7 * i) % 16)
            };
            let tmp = d;
            d = c;
            c = b;
            let sum = a
                .wrapping_add(f)
                .wrapping_add(Self::K[i])
                .wrapping_add(m[g]);
            b = b.wrapping_add(sum.rotate_left(Self::S[i]));
            a = tmp;
        }
        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
    }

    fn finalize(mut self) -> [u8; 16] {
        let bit_len = self.count_bytes.wrapping_mul(8);
        self.update(&[0x80]);
        while self.buf_len != 56 {
            self.update(&[0x00]);
        }
        self.update(&bit_len.to_le_bytes());
        let mut out = [0u8; 16];
        for i in 0..4 {
            out[i * 4..i * 4 + 4].copy_from_slice(&self.state[i].to_le_bytes());
        }
        out
    }
}
