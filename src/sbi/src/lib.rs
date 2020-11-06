#![feature(asm)]
#![no_std]

pub mod base;

/// Error codes returned by SBI calls
///
/// note: `SBI_SUCCESS` is not represented here since this is to be used as the
/// error type in a `Result`, therefore building one with the value of
/// `SBI_SUCCESS` will result in an `UnknownErrCode` (which is not defined by
/// the specification but exists to allow ease of construction of `SbiError`)
pub enum SbiError {
    Failed,
    NotSupported,
    InvalidParam,
    Denied,
    InvalidAddress,
    AlreadyAvailable,
    UnknownErrCode(isize),
}

impl SbiError {
    pub fn new(n: isize) -> Self {
        match n {
            -1 => SbiError::Failed,
            -2 => SbiError::NotSupported,
            -3 => SbiError::InvalidParam,
            -4 => SbiError::Denied,
            -5 => SbiError::InvalidAddress,
            -6 => SbiError::AlreadyAvailable,
            n => SbiError::UnknownErrCode(n),
        }
    }
}

pub type SbiResult = Result<usize, SbiError>;