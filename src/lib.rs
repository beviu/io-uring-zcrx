use std::{
    io,
    mem::{self, ManuallyDrop}, ops::{Deref, DerefMut}, slice,
};

use io_uring::{cqueue, squeue, IoUring};
use mmap::Mmap;

mod mmap;
pub mod rqueue;
mod sys;

pub struct IoUringZcrxIfq {
    area: ManuallyDrop<Mmap>,
    region: ManuallyDrop<Mmap>,
    rq: rqueue::Inner,
    area_token: u64,
}

impl IoUringZcrxIfq {
    pub fn register<S: squeue::EntryMarker>(
        ring: &IoUring<S, cqueue::Entry32>,
        interface_index: u32,
        rx_queue_index: u32,
        refill_ring_entries: u32,
        area_size: usize,
    ) -> io::Result<Self> {
        let area = Mmap::new_anon(area_size)?;

        let page_size = page_size()?;
        let refill_ring_size = page_size
            + mem::size_of::<rqueue::Entry>() * usize::try_from(refill_ring_entries).unwrap();
        let page_mask = !(page_size - 1);
        let region = Mmap::new_anon((refill_ring_size + page_size - 1) & page_mask)?;

        let params = unsafe {
            ring.submitter().register_zcrx_ifq(
                interface_index,
                rx_queue_index,
                refill_ring_entries,
                area.as_mut_ptr() as u64,
                u64::try_from(area.len()).unwrap(),
                region.as_mut_ptr() as u64,
                u64::try_from(region.len()).unwrap(),
            )
        }?;

        let region_ptr = region.as_mut_ptr();
        Ok(Self {
            area: ManuallyDrop::new(area),
            region: ManuallyDrop::new(region),
            rq: unsafe {
                rqueue::Inner::new(
                    region_ptr,
                    params.rq_entries,
                    params.offset_head,
                    params.offset_tail,
                    params.offset_rqes,
                )
            },
            area_token: params.rq_area_token,
        })
    }

    pub unsafe fn get_buf(&self, offset: u64, len: usize) -> Option<BorrowedBuffer> {
        let data = self
            .area
            .as_mut_ptr()
            .cast::<u8>()
            .offset(offset as isize);
        Some(BorrowedBuffer {
            slice: slice::from_raw_parts_mut(data, len),
            off: offset | self.area_token,
        })
    }

    /// Release the memory used by the zero-copy interface queue registration without unregistering
    /// it from [`IoUring`].
    ///
    /// # Safety
    ///
    /// Caller must make sure there is no pending zero-copy receive on the [`IoUring`], or the
    /// [`IoUring`] is dropped.
    pub unsafe fn drop(mut self) {
        ManuallyDrop::drop(&mut self.area);
        ManuallyDrop::drop(&mut self.region);
    }

    /// Get the refill queue. This is used to recycle buffers that were
    /// used for zero-copy receive operations.
    #[inline]
    pub fn refill(&mut self) -> rqueue::RefillQueue<'_> {
        self.rq.borrow()
    }

    /// Get the refill queue from a shared reference.
    ///
    /// # Safety
    ///
    /// No other [`RefillQueue`](rqueue::RefillQueue)s may exist when calling this function.
    #[inline]
    pub unsafe fn refill_shared(&self) -> rqueue::RefillQueue<'_> {
        self.rq.borrow_shared()
    }

    #[inline]
    pub fn area_token(&self) -> u64 {
        self.area_token
    }
}

fn page_size() -> io::Result<usize> {
    let ret = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if ret < 0 {
        return Err(io::Error::from_raw_os_error(-ret as i32));
    }
    Ok(ret as usize)
}

pub struct ZcrxCqe {
    off: u64,
}

impl From<cqueue::Entry32> for ZcrxCqe {
    fn from(value: cqueue::Entry32) -> Self {
        let rcqe: &sys::io_uring_zcrx_cqe = unsafe { mem::transmute(value.big_cqe()) };
        Self { off: rcqe.off }
    }
}

impl ZcrxCqe {
    pub fn buffer_offset(&self) -> u64 {
        self.off & !sys::IORING_ZCRX_AREA_MASK
    }
    
    pub fn area_token(&self) -> u64 {
        self.off & sys::IORING_ZCRX_AREA_MASK
    }
}

pub struct BorrowedBuffer<'a> {
    slice: &'a mut [u8],
    off: u64,
}

impl<'a> BorrowedBuffer<'a> {
    pub fn into_refill_entry(self) -> rqueue::Entry {
        rqueue::Entry(sys::io_uring_zcrx_rqe {
            off: self.off,
            len: self.slice.len() as u32,
            __pad: 0,
        })
    }
}

impl<'a> Deref for BorrowedBuffer<'a> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.slice
    }
}

impl<'a> DerefMut for BorrowedBuffer<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.slice
    }
}
