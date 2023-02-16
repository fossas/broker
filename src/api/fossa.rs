//! Interactions and data types for the FOSSA API live here.

use nutype::nutype;

/// The FOSSA API key.
#[nutype(validate(not_empty))]
#[derive(*)]
pub struct ApiKey(String);
