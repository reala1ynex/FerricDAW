use std::io;
use std::ptr;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, HANDLE,
    INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Pipes::ConnectNamedPipe;
use windows_sys::Win32::System::Threading::{CreateEventW, ResetEvent, WaitForSingleObject};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Memory::{
    MapViewOfFile, UnmapViewOfFile, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE,
};
use windows_sys::Win32::System::Pipes::WaitNamedPipeW;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};

pub struct NamedPipeEndpoint {
    handle: HANDLE,
    read_buf: Vec<u8>,
    /// Manual-reset event for overlapped I/O (server side only).
    io_event: HANDLE,
}

/// Error kind used when a pipe operation hits its deadline.
pub fn is_timeout(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::TimedOut
}

impl NamedPipeEndpoint {
    pub fn from_connected_server(handle: HANDLE) -> Self {
        Self {
            handle,
            read_buf: vec![0u8; 65536],
            io_event: ptr::null_mut(),
        }
    }

    /// Wrap a server pipe created with `FILE_FLAG_OVERLAPPED`. All `*_timeout`
    /// operations on it can be abandoned at a deadline (audit H5).
    pub fn from_overlapped_server(handle: HANDLE) -> io::Result<Self> {
        let io_event = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
        if io_event.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            handle,
            read_buf: vec![0u8; 65536],
            io_event,
        })
    }

    /// Run one overlapped operation, waiting at most until `deadline`.
    /// `alive` is polled every 50 ms so a dead peer fails fast.
    fn overlapped(
        &mut self,
        deadline: Instant,
        alive: &mut dyn FnMut() -> bool,
        start: impl FnOnce(HANDLE, *mut OVERLAPPED) -> i32,
    ) -> io::Result<u32> {
        if self.io_event.is_null() {
            return Err(io::Error::other("pipe was not opened for overlapped I/O"));
        }
        let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
        ov.hEvent = self.io_event;
        unsafe {
            ResetEvent(self.io_event);
        }
        let ok = start(self.handle, &mut ov);
        if ok == 0 {
            let err = unsafe { GetLastError() };
            if err == ERROR_PIPE_CONNECTED {
                return Ok(0);
            }
            if err != ERROR_IO_PENDING {
                return Err(io::Error::from_raw_os_error(err as i32));
            }
            loop {
                let now = Instant::now();
                let remaining = deadline.saturating_duration_since(now);
                let slice = remaining.min(Duration::from_millis(50));
                let w = unsafe { WaitForSingleObject(self.io_event, slice.as_millis() as u32) };
                if w == WAIT_OBJECT_0 {
                    break;
                }
                let expired = Instant::now() >= deadline;
                if expired || !alive() {
                    unsafe {
                        CancelIoEx(self.handle, &ov);
                        // Wait for the cancellation so `ov` is not used after return.
                        let mut n = 0u32;
                        GetOverlappedResult(self.handle, &ov, &mut n, 1);
                    }
                    return Err(if expired {
                        io::Error::new(io::ErrorKind::TimedOut, "plugin bridge pipe timed out")
                    } else {
                        io::Error::new(io::ErrorKind::BrokenPipe, "plugin bridge worker exited")
                    });
                }
            }
        }
        let mut n = 0u32;
        if unsafe { GetOverlappedResult(self.handle, &ov, &mut n, 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(n)
    }

    /// Wait for the worker to connect to this server pipe.
    pub fn accept_timeout(
        &mut self,
        timeout: Duration,
        alive: &mut dyn FnMut() -> bool,
    ) -> io::Result<()> {
        let deadline = Instant::now() + timeout;
        self.overlapped(deadline, alive, |h, ov| unsafe { ConnectNamedPipe(h, ov) })
            .map(|_| ())
    }

    fn read_exact_timeout(
        &mut self,
        range: std::ops::Range<usize>,
        deadline: Instant,
        alive: &mut dyn FnMut() -> bool,
    ) -> io::Result<()> {
        let mut offset = range.start;
        while offset < range.end {
            let buf_ptr = self.read_buf[offset..range.end].as_mut_ptr();
            let len = (range.end - offset) as u32;
            let n = self.overlapped(deadline, alive, |h, ov| unsafe {
                ReadFile(h, buf_ptr, len, ptr::null_mut(), ov)
            })?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "plugin bridge pipe closed",
                ));
            }
            offset += n as usize;
        }
        Ok(())
    }

    /// Read one length-prefixed frame into the endpoint's reusable buffer and
    /// return the payload (without the length prefix). Does not allocate once
    /// the buffer has grown to the largest frame seen.
    pub fn read_payload_timeout(
        &mut self,
        timeout: Duration,
        alive: &mut dyn FnMut() -> bool,
    ) -> io::Result<&[u8]> {
        let deadline = Instant::now() + timeout;
        self.read_exact_timeout(0..4, deadline, alive)?;
        let len = u32::from_le_bytes([
            self.read_buf[0],
            self.read_buf[1],
            self.read_buf[2],
            self.read_buf[3],
        ]) as usize;
        if len > 64 * 1024 * 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "plugin bridge frame too large",
            ));
        }
        if 4 + len > self.read_buf.len() {
            self.read_buf.resize(4 + len, 0);
        }
        self.read_exact_timeout(4..4 + len, deadline, alive)?;
        Ok(&self.read_buf[4..4 + len])
    }

    /// Write a whole frame, giving up at the deadline.
    pub fn write_frame_timeout(
        &mut self,
        frame: &[u8],
        timeout: Duration,
        alive: &mut dyn FnMut() -> bool,
    ) -> io::Result<()> {
        let deadline = Instant::now() + timeout;
        let mut rest = frame;
        while !rest.is_empty() {
            let buf_ptr = rest.as_ptr();
            let len = rest.len() as u32;
            let n = self.overlapped(deadline, alive, |h, ov| unsafe {
                WriteFile(h, buf_ptr, len, ptr::null_mut(), ov)
            })?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "plugin bridge pipe write stalled",
                ));
            }
            rest = &rest[n as usize..];
        }
        Ok(())
    }

    pub fn connect_client(pipe_name: &str) -> io::Result<Self> {
        let wide: Vec<u16> = pipe_name
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            WaitNamedPipeW(wide.as_ptr(), 30_000);
        }
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                FILE_GENERIC_READ | FILE_GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            handle,
            read_buf: vec![0u8; 65536],
            io_event: ptr::null_mut(),
        })
    }

    pub fn read_frame(&mut self) -> io::Result<Vec<u8>> {
        let mut len_bytes = [0u8; 4];
        read_exact(self.handle, &mut len_bytes)?;
        let len = u32::from_le_bytes(len_bytes) as usize;
        if len > self.read_buf.len() {
            self.read_buf.resize(len, 0);
        }
        let buf = &mut self.read_buf[..len];
        read_exact(self.handle, buf)?;
        let mut out = Vec::with_capacity(4 + len);
        out.extend_from_slice(&len_bytes);
        out.extend_from_slice(buf);
        Ok(out)
    }

    pub fn write_frame(&mut self, frame: &[u8]) -> io::Result<()> {
        write_all(self.handle, frame)
    }

    pub fn handle(&self) -> HANDLE {
        self.handle
    }
}

impl Drop for NamedPipeEndpoint {
    fn drop(&mut self) {
        if !self.handle.is_null() && self.handle != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.handle);
            }
        }
        if !self.io_event.is_null() {
            unsafe {
                CloseHandle(self.io_event);
            }
        }
    }
}

fn read_exact(handle: HANDLE, buf: &mut [u8]) -> io::Result<()> {
    let mut offset = 0usize;
    while offset < buf.len() {
        let mut n = 0u32;
        let ok = unsafe {
            ReadFile(
                handle,
                buf[offset..].as_mut_ptr(),
                (buf.len() - offset) as u32,
                &mut n,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "plugin bridge pipe closed",
            ));
        }
        offset += n as usize;
    }
    Ok(())
}

fn write_all(handle: HANDLE, mut buf: &[u8]) -> io::Result<()> {
    while !buf.is_empty() {
        let mut n = 0u32;
        let ok = unsafe {
            WriteFile(
                handle,
                buf.as_ptr(),
                buf.len() as u32,
                &mut n,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "plugin bridge pipe write stalled",
            ));
        }
        buf = &buf[n as usize..];
    }
    Ok(())
}

pub struct ShmMapping {
    pub handle: HANDLE,
    pub view: *mut u8,
    pub byte_len: usize,
    mapped: MEMORY_MAPPED_VIEW_ADDRESS,
}

impl Drop for ShmMapping {
    fn drop(&mut self) {
        unsafe {
            if !self.view.is_null() {
                UnmapViewOfFile(self.mapped);
            }
            if !self.handle.is_null() && self.handle != INVALID_HANDLE_VALUE {
                CloseHandle(self.handle);
            }
        }
    }
}

fn view_ptr(view: MEMORY_MAPPED_VIEW_ADDRESS) -> *mut u8 {
    view.Value as *mut u8
}

pub fn create_shm_mapping(name: &str, byte_len: usize) -> io::Result<ShmMapping> {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let handle = unsafe {
        windows_sys::Win32::System::Memory::CreateFileMappingW(
            INVALID_HANDLE_VALUE,
            ptr::null(),
            PAGE_READWRITE,
            0,
            byte_len as u32,
            wide.as_ptr(),
        )
    };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    let mapped = unsafe {
        MapViewOfFile(
            handle,
            FILE_MAP_ALL_ACCESS,
            0,
            0,
            byte_len,
        )
    };
    let view = view_ptr(mapped);
    if view.is_null() {
        unsafe {
            CloseHandle(handle);
        }
        return Err(io::Error::last_os_error());
    }
    Ok(ShmMapping {
        handle,
        view,
        byte_len,
        mapped,
    })
}

pub fn open_shm_mapping(name: &str, byte_len: usize) -> io::Result<ShmMapping> {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let handle = unsafe {
        windows_sys::Win32::System::Memory::OpenFileMappingW(FILE_MAP_ALL_ACCESS, 0, wide.as_ptr())
    };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    let mapped = unsafe {
        MapViewOfFile(
            handle,
            FILE_MAP_ALL_ACCESS,
            0,
            0,
            byte_len,
        )
    };
    let view = view_ptr(mapped);
    if view.is_null() {
        unsafe {
            CloseHandle(handle);
        }
        return Err(io::Error::last_os_error());
    }
    Ok(ShmMapping {
        handle,
        view,
        byte_len,
        mapped,
    })
}