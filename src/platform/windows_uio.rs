//! Native EAPOL transport through the inbox NDIS Usermode I/O protocol driver.
//! This interface is restricted to 0x888e by Windows; it is not a general packet
//! capture backend. Every asynchronous request is joined before its buffers die.
use super::{Adapter, Cancel, Handle, wide};
use anyhow::{Result, ensure};
use std::{io, mem, ptr, sync::Mutex};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::*,
    System::{IO::*, Threading::*},
};

const OPEN_DEVICE: u32 = 0x12c800;
const SET_ETHER_TYPE: u32 = 0x12c808;
pub struct Capture {
    handle: Handle,
    send_lock: Mutex<()>,
}
struct Request<'a> {
    handle: &'a Handle,
    overlapped: OVERLAPPED,
    _event: Handle,
    pending: bool,
}
impl<'a> Request<'a> {
    fn new(handle: &'a Handle) -> Result<Self> {
        let event = Handle::new(unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) })?;
        Ok(Self {
            handle,
            overlapped: OVERLAPPED {
                hEvent: event.0,
                ..unsafe { mem::zeroed() }
            },
            _event: event,
            pending: false,
        })
    }
    fn started(&mut self, ok: i32) -> Result<()> {
        if ok == 0 {
            let error = unsafe { GetLastError() };
            ensure!(error == ERROR_IO_PENDING, "NDISUIO request failed: {error}");
            self.pending = true;
        }
        Ok(())
    }
    fn finish(&mut self) -> Result<u32> {
        let mut transferred = 0;
        let ok =
            unsafe { GetOverlappedResult(self.handle.0, &self.overlapped, &mut transferred, 1) };
        self.pending = false;
        ensure!(
            ok != 0,
            "NDISUIO completion: {}",
            io::Error::last_os_error()
        );
        Ok(transferred)
    }
    fn bounded(&mut self) -> Result<u32> {
        let wait = unsafe { WaitForSingleObject(self.overlapped.hEvent, 5000) };
        ensure!(
            wait == WAIT_OBJECT_0,
            "NDISUIO operation timed out or wait failed: {wait}"
        );
        self.finish()
    }
}
impl Drop for Request<'_> {
    fn drop(&mut self) {
        if self.pending {
            unsafe {
                CancelIoEx(self.handle.0, &self.overlapped);
                let mut transferred = 0;
                GetOverlappedResult(self.handle.0, &self.overlapped, &mut transferred, 1);
            }
        }
    }
}
impl Capture {
    pub fn open(adapter: &Adapter) -> Result<Self> {
        ensure!(
            adapter.ethernet,
            "NDISUIO authentication requires a physical Ethernet adapter"
        );
        let handle = Handle::new(unsafe {
            CreateFileW(
                wide(r"\\.\Ndisuio").as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                ptr::null_mut(),
            )
        })?;
        let name: Vec<u16> = format!(r"\DEVICE\{}", adapter.id).encode_utf16().collect();
        let mut request = Request::new(&handle)?;
        let ok = unsafe {
            DeviceIoControl(
                handle.0,
                OPEN_DEVICE,
                name.as_ptr().cast(),
                (name.len() * 2) as u32,
                ptr::null_mut(),
                0,
                ptr::null_mut(),
                &mut request.overlapped,
            )
        };
        request.started(ok)?;
        request.bounded()?;
        drop(request);
        // Do not depend on another Windows client having initialized this
        // protocol driver's EtherType since boot. The inbox driver permits
        // only EAPOL here; setting it also leaves Wi-Fi EAPOL traffic intact.
        let ether_type = crate::protocol::ETHERTYPE.to_be_bytes();
        let mut request = Request::new(&handle)?;
        let ok = unsafe {
            DeviceIoControl(
                handle.0,
                SET_ETHER_TYPE,
                ether_type.as_ptr().cast(),
                ether_type.len() as u32,
                ptr::null_mut(),
                0,
                ptr::null_mut(),
                &mut request.overlapped,
            )
        };
        request.started(ok)?;
        request.bounded()?;
        drop(request);
        Ok(Self {
            handle,
            send_lock: Mutex::new(()),
        })
    }
    pub fn send(&self, bytes: &[u8]) -> Result<()> {
        ensure!(
            bytes.len() >= 18
                && bytes.len() <= crate::protocol::MAX_FRAME
                && bytes[12..14] == crate::protocol::ETHERTYPE.to_be_bytes(),
            "invalid native EAPOL frame"
        );
        let _guard = self.send_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut request = Request::new(&self.handle)?;
        let ok = unsafe {
            WriteFile(
                self.handle.0,
                bytes.as_ptr(),
                bytes.len() as u32,
                ptr::null_mut(),
                &mut request.overlapped,
            )
        };
        request.started(ok)?;
        ensure!(
            request.bounded()? as usize == bytes.len(),
            "NDISUIO short send"
        );
        Ok(())
    }
    pub fn receive(&self, stop: &Cancel) -> Result<Option<Vec<u8>>> {
        let mut bytes = vec![0; crate::protocol::MAX_FRAME];
        let mut request = Request::new(&self.handle)?;
        let ok = unsafe {
            ReadFile(
                self.handle.0,
                bytes.as_mut_ptr(),
                bytes.len() as u32,
                ptr::null_mut(),
                &mut request.overlapped,
            )
        };
        request.started(ok)?;
        let handles = [stop.0.0, request.overlapped.hEvent];
        let wait = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) };
        if wait == WAIT_OBJECT_0 {
            return Ok(None);
        }
        ensure!(
            wait == WAIT_OBJECT_0 + 1,
            "NDISUIO receive wait failed: {wait}"
        );
        let size = request.finish()? as usize;
        ensure!(size <= bytes.len(), "NDISUIO oversized receive");
        drop(request);
        bytes.truncate(size);
        Ok(Some(bytes))
    }
}
