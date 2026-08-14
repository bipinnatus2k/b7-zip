// Allow non-standard naming for C/C++ bindings.
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

//! Raw C FFI bindings to the bit7z C++ wrapper (`demo.h`, compiled via
//! `bridge.cc`).
//!
//! * [`ffi_gen`] — generated `extern "C"` declarations for every exported
//!   wrapper function.
//! * [`ffi_ext`] — hand-written declarations for the callback-based
//!   extract/compress variants.

mod ffi_ext;
mod ffi_gen;

pub use ffi_ext::*;
pub use ffi_gen::*;
pub use std::ffi::c_void;
