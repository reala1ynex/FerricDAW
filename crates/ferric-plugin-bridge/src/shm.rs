use crate::protocol::{BRIDGE_IO_VERSION, DEFAULT_MAX_FRAMES};

pub const SHM_MAGIC: u32 = 0x4652_5042; // "FRPB"

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ShmHeader {
    pub magic: u32,
    pub version: u32,
    pub max_frames: u32,
    pub _pad: u32,
}

impl ShmHeader {
    pub fn new(max_frames: u32) -> Self {
        Self {
            magic: SHM_MAGIC,
            version: BRIDGE_IO_VERSION,
            max_frames: max_frames.max(1),
            _pad: 0,
        }
    }
}

pub struct AudioShmLayout {
    pub max_frames: u32,
    pub byte_len: usize,
}

impl AudioShmLayout {
    pub fn new(max_frames: u32) -> Self {
        let frames = max_frames.max(1).min(DEFAULT_MAX_FRAMES * 2);
        let samples = frames as usize * 2; // stereo interleaved in + out
        let byte_len = std::mem::size_of::<ShmHeader>() + samples * 2 * std::mem::size_of::<f32>();
        Self {
            max_frames: frames,
            byte_len,
        }
    }

    pub fn input_offset(&self) -> usize {
        std::mem::size_of::<ShmHeader>()
    }

    pub fn output_offset(&self, frames: usize) -> usize {
        self.input_offset() + frames * 2 * std::mem::size_of::<f32>()
    }
}

/// Mapped stereo interleaved in/out scratch (host ↔ worker).
pub struct AudioShmRegion {
    pub layout: AudioShmLayout,
    base: *mut u8,
}

impl AudioShmRegion {
    /// Host side: mapping is fresh — header is written immediately after open.
    pub unsafe fn new_host(layout: AudioShmLayout, base: *mut u8) -> Self {
        Self { layout, base }
    }

    pub unsafe fn from_mapped_ptr(layout: AudioShmLayout, base: *mut u8) -> Self {
        let hdr = &*(base as *const ShmHeader);
        if hdr.magic != SHM_MAGIC {
            panic!("invalid plugin bridge shm magic");
        }
        Self { layout, base }
    }

    pub fn write_header(&self) {
        let hdr = ShmHeader::new(self.layout.max_frames);
        unsafe {
            std::ptr::write(self.base as *mut ShmHeader, hdr);
        }
    }

    pub fn input_stereo_interleaved_mut(&mut self, frames: usize) -> &mut [f32] {
        let samples = frames * 2;
        let offset = self.layout.input_offset();
        unsafe {
            std::slice::from_raw_parts_mut(
                self.base.add(offset) as *mut f32,
                samples,
            )
        }
    }

    pub fn output_stereo_interleaved_mut(&mut self, frames: usize) -> &mut [f32] {
        let samples = frames * 2;
        let offset = self.layout.output_offset(frames);
        unsafe {
            std::slice::from_raw_parts_mut(
                self.base.add(offset) as *mut f32,
                samples,
            )
        }
    }

    pub fn copy_lr_to_input(&mut self, left: &[f32], right: &[f32]) {
        let frames = left.len().min(right.len()).min(self.layout.max_frames as usize);
        if frames == 0 {
            return;
        }
        let buf = self.input_stereo_interleaved_mut(frames);
        for i in 0..frames {
            buf[i * 2] = left[i];
            buf[i * 2 + 1] = right[i];
        }
    }

    pub fn copy_output_to_lr(&self, left: &mut [f32], right: &mut [f32]) {
        let frames = left.len().min(right.len()).min(self.layout.max_frames as usize);
        if frames == 0 {
            return;
        }
        let offset = self.layout.output_offset(frames);
        let out = unsafe {
            std::slice::from_raw_parts(self.base.add(offset) as *const f32, frames * 2)
        };
        for i in 0..frames {
            left[i] = out[i * 2];
            right[i] = out[i * 2 + 1];
        }
    }
}

unsafe impl Send for AudioShmRegion {}