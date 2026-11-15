use std::io;
use std::ptr;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
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
}

impl NamedPipeEndpoint {
    pub fn from_connected_server(handle: HANDLE) -> Self {
        Self {
            handle,
            read_buf: vec![0u8; 65536],
        }
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