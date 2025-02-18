#![allow(non_camel_case_types)]

#[repr(C)]
#[derive(Clone, Copy)]
pub struct io_uring_zcrx_rqe {
    pub off: u64,
    pub len: u32,
    pub __pad: u32,
}

/// The bit from which area id is encoded into offsets.
pub const IORING_ZCRX_AREA_SHIFT: u64 = 48;

pub const IORING_ZCRX_AREA_MASK: u64 = !((1 << IORING_ZCRX_AREA_SHIFT) - 1);

#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
pub struct io_uring_zcrx_cqe {
    pub off: u64,
    pub __pad: u32,
}
