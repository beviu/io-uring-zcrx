use std::{ffi, io, ptr};

pub(crate) struct Mmap {
    addr: *mut ffi::c_void,
    len: usize,
}

impl Mmap {
    pub(crate) fn new_anon(len: usize) -> io::Result<Self> {
        let addr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_ANONYMOUS | libc::MAP_PRIVATE,
                -1,
                0,
            )
        };
        if addr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { addr, len })
    }

    #[inline]
    pub(crate) fn as_mut_ptr(&self) -> *mut ffi::c_void {
        self.addr
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.len
    }
}

impl Drop for Mmap {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.addr, self.len);
        }
    }
}
