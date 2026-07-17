// OPCUA for Rust
// SPDX-License-Identifier: MPL-2.0
// Copyright (C) 2017-2024 Adam Lock

//! Functionality for holding a message digest.
use serde::{Deserialize, Serialize};

use opcua_types::{ByteString, Error, StatusCode};

/// The thumbprint holds a 20 byte representation of a certificate that can be used as a hash,
/// handshake comparison, a filename hint or similar purpose where a shortened representation
/// of a cert is required. Thumbprint size is dictated by the OPC UA spec
#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct Thumbprint {
    /// Thumbprint is relatively small and fixed size, so use array to hold value instead of a vec
    /// just to save heap
    value: [u8; Thumbprint::THUMBPRINT_SIZE],
}

impl From<Thumbprint> for ByteString {
    fn from(value: Thumbprint) -> Self {
        Self::from(&value.value)
    }
}

impl Thumbprint {
    /// Size of thumbprint.
    pub const THUMBPRINT_SIZE: usize = 20;

    /// Constructs a thumbprint from a message digest which is expected to be the proper length.
    pub fn new(digest: &[u8]) -> Result<Thumbprint, Error> {
        if digest.len() != Thumbprint::THUMBPRINT_SIZE {
            return Err(Error::new(
                StatusCode::BadUnexpectedError,
                format!("Thumbprint is the wrong length, {}", digest.len()),
            ));
        }
        let mut value = [0u8; Thumbprint::THUMBPRINT_SIZE];
        value.clone_from_slice(digest);
        Ok(Thumbprint { value })
    }

    /// Create a byte string from this thumbprint.
    pub fn as_byte_string(&self) -> ByteString {
        ByteString::from(&self.value)
    }

    /// Returns the thumbprint as a string using hexadecimal values for each byte
    pub fn as_hex_string(&self) -> String {
        let mut hex_string = String::with_capacity(self.value.len() * 2);
        for b in self.value.iter() {
            hex_string.push_str(&format!("{b:02x}"))
        }
        hex_string
    }

    /// Parses a thumbprint from a hexadecimal string as produced by [`Thumbprint::as_hex_string`]
    /// (e.g. the `Thumbprint` argument of OPC UA Part 12's `RemoveCertificate` Method).
    pub fn parse_hex(s: &str) -> Result<Thumbprint, Error> {
        let s = s.trim();
        if s.len() != Thumbprint::THUMBPRINT_SIZE * 2 {
            return Err(Error::new(
                StatusCode::BadInvalidArgument,
                format!("Thumbprint hex string has the wrong length, {}", s.len()),
            ));
        }
        let mut value = [0u8; Thumbprint::THUMBPRINT_SIZE];
        for (i, byte) in value.iter_mut().enumerate() {
            let hex_byte = s.get(i * 2..i * 2 + 2).ok_or_else(|| {
                Error::new(
                    StatusCode::BadInvalidArgument,
                    "Invalid thumbprint hex string",
                )
            })?;
            *byte = u8::from_str_radix(hex_byte, 16).map_err(|_| {
                Error::new(
                    StatusCode::BadInvalidArgument,
                    "Invalid thumbprint hex string",
                )
            })?;
        }
        Ok(Thumbprint { value })
    }

    /// Returns the thumbprint
    pub fn value(&self) -> &[u8] {
        &self.value[..]
    }
}
