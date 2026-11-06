//! Credential strings that never appear in `Debug` output, logs or error messages.

use std::fmt;

use serde::{Deserialize, Deserializer};

/// Maximum credential length in Unicode scalar values.
pub const MAX_CREDENTIAL_CHARS: usize = 1024;

#[derive(Clone, PartialEq, Eq, Default)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The raw value, for handshake headers and signatures only.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_blank(&self) -> bool {
        self.0.trim().is_empty()
    }

    pub fn is_within_limit(&self) -> bool {
        self.0.chars().count() <= MAX_CREDENTIAL_CHARS
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self)
    }
}
