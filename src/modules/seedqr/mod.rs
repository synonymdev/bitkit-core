//! SeedQR decoding shared by the platform applications.
//!
//! Camera scanning and normalization of platform-specific barcode payloads
//! remain application concerns. This module accepts the canonical Standard
//! SeedQR text or Compact SeedQR entropy described by the SeedQR format.

mod errors;
mod implementation;
#[cfg(test)]
mod tests;

pub use errors::SeedQrError;
pub use implementation::{decode_compact_seed_qr, decode_standard_seed_qr};
