use std::{error::Error, ffi, fmt::{self, Display, Formatter, Debug}, sync::atomic::{AtomicU32, Ordering}};

use crate::sys;

pub(crate) struct Inner {
    head: *const AtomicU32,
    tail: *const AtomicU32,
    ring_entries: u32,
    ring_mask: u32,
    rqes: *mut Entry,
}

impl Inner {
    pub(crate) unsafe fn new(
        region: *mut ffi::c_void,
        ring_entries: u32,
        head_offset: u32,
        tail_offset: u32,
        rqes_offset: u32,
    ) -> Inner {
        debug_assert!(ring_entries.is_power_of_two());
        let ring_mask = ring_entries - 1;

        Self {
            head: region.offset(head_offset as isize).cast(),
            tail: region.offset(tail_offset as isize).cast(),
            ring_entries,
            ring_mask,
            rqes: region.offset(rqes_offset as isize).cast(),
        }
    }

    #[inline]
    pub(crate) unsafe fn borrow_shared(&self) -> RefillQueue<'_> {
        RefillQueue {
            head: (*self.head).load(Ordering::Acquire),
            tail: unsync_load(self.tail),
            queue: self,
        }
    }

    #[inline]
    pub(crate) fn borrow(&mut self) -> RefillQueue<'_> {
        unsafe { self.borrow_shared() }
    }
}

#[inline(always)]
unsafe fn unsync_load(u: *const AtomicU32) -> u32 {
    *u.cast::<u32>()
}

pub struct RefillQueue<'a> {
    head: u32,
    tail: u32,
    queue: &'a Inner,
}

impl<'a> RefillQueue<'a> {
    pub fn sync(&mut self) {
        unsafe { &*self.queue.tail }.store(self.tail, Ordering::Release);
        unsafe { &*self.queue.head }.load(Ordering::Acquire);
    }

    /// Get the total number of entries in the refill queue ring buffer.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.queue.ring_entries as usize
    }

    /// Get the number of refill queue events in the ring buffer.
    #[inline]
    pub fn len(&self) -> usize {
        self.tail.wrapping_sub(self.head) as usize
    }

    /// Returns `true` if the refill queue ring buffer is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns `true` if the refill queue ring buffer has reached capacity, and no more buffers can
    /// be added before the kernel consumes some.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }

    /// Attempts to push an entry into the queue.
    /// If the queue is full, an error is returned.
    ///
    /// # Safety
    ///
    /// Developers must ensure that parameters of the entry are valid and will be valid for the
    /// entire duration of the zero-copy receive operations, otherwise it may cause memory problems.
    #[inline]
    pub unsafe fn push(&mut self, entry: &Entry) -> Result<(), PushError> {
        if !self.is_full() {
            self.push_unchecked(entry);
            Ok(())
        } else {
            Err(PushError)
        }
    }

    /// Attempts to push several entries into the queue.
    /// If the queue does not have space for all of the entries, an error is returned.
    ///
    /// # Safety
    ///
    /// Developers must ensure that parameters of all the entries (such as buffer) are valid and
    /// will be valid for the entire duration of the zero-copy receive operations, otherwise it may
    /// cause memory problems.
    #[inline]
    pub unsafe fn push_multiple(&mut self, entries: &[Entry]) -> Result<(), PushError> {
        if self.capacity() - self.len() < entries.len() {
            return Err(PushError);
        }

        for entry in entries {
            self.push_unchecked(entry);
        }

        Ok(())
    }

    #[inline]
    unsafe fn push_unchecked(&mut self, entry: &Entry) {
        *self
            .queue
            .rqes
            .add((self.tail & self.queue.ring_mask) as usize) = *entry;
        self.tail = self.tail.wrapping_add(1);
    }
}

impl<'a> Drop for RefillQueue<'a> {
    fn drop(&mut self) {
        unsafe { &*self.queue.tail }.store(self.tail, Ordering::Release);
    }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Entry(pub(crate) sys::io_uring_zcrx_rqe);

impl Entry {
    fn buffer_offset(&self) -> u64 {
        self.0.off & !sys::IORING_ZCRX_AREA_MASK
    }

    fn area_token(&self) -> u64 {
        self.0.off & sys::IORING_ZCRX_AREA_MASK
    }
}

impl Debug for Entry {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Entry")
            .field("buffer_offset", &self.buffer_offset())
            .field("area_token", &self.area_token())
            .field("len", &self.0.len)
            .finish()
    }
}

/// An error pushing to the refill queue due to it being full.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PushError;

impl Display for PushError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("refill queue is full")
    }
}

impl Error for PushError {}

impl Debug for RefillQueue<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_list();
        let mut pos = self.head;
        while pos != self.tail {
            let entry: &Entry = unsafe { &*self.queue.rqes.add((pos & self.queue.ring_mask) as usize) };
            d.entry(&entry);
            pos = pos.wrapping_add(1);
        }
        d.finish()
    }
}
