//! Extensions to the `bytesize` crate.

use bytesize::ByteSize;

/// Parse a user-provided number of bytes.
pub fn parse_bytes(input: u64) -> ByteSize {
    ByteSize::b(input)
}
