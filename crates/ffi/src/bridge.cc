// Compilation unit for the C++ wrapper layer (demo.h).
//
// demo.h declares the exported functions as `extern "C" inline`, which
// produces linkable COMDAT symbols in this TU. Rust declares the same
// functions in ffi_gen.rs and links against them.

#include "demo.h"
