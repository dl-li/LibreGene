//! Melting temperature computation — delegates to the full
//! SantaLucia 1998 nearest-neighbour model in [`thermodynamics`].
//!
//! This module is kept as a thin re-export so existing callers don't break.

pub use super::thermodynamics::{compute_tm, compute_tm_nn, gc_content};
