// OPCUA for Rust
// SPDX-License-Identifier: MPL-2.0
// Copyright (C) 2017-2024 Adam Lock

use std::io::{Cursor, Write};
use std::ops::Range;

use bytes::Buf;
use tracing::{error, trace};

use opcua_crypto::{KeySize, SecurityPolicy, X509};
use opcua_types::{
    status_code::StatusCode, write_bytes, write_u32, write_u8, ByteString, Error,
    MessageSecurityMode, SimpleBinaryDecodable,
};

use crate::comms::{
    message_chunk::{MessageChunk, MessageChunkHeader, MessageChunkType, MESSAGE_SIZE_OFFSET},
    security_header::{AsymmetricSecurityHeader, SecurityHeader, SymmetricSecurityHeader},
};

use super::{DecryptedChunkStorage, SecureChannel};

pub(super) fn security_slice(buf: &[u8], range: Range<usize>) -> Result<&[u8], StatusCode> {
    buf.get(range).ok_or(StatusCode::BadSecurityChecksFailed)
}

pub(super) fn security_slice_to(buf: &[u8], end: usize) -> Result<&[u8], StatusCode> {
    buf.get(..end).ok_or(StatusCode::BadSecurityChecksFailed)
}

pub(super) fn security_mut_slice(
    buf: &mut [u8],
    range: Range<usize>,
) -> Result<&mut [u8], StatusCode> {
    buf.get_mut(range)
        .ok_or(StatusCode::BadSecurityChecksFailed)
}

pub(super) fn security_mut_slice_from(
    buf: &mut [u8],
    start: usize,
) -> Result<&mut [u8], StatusCode> {
    buf.get_mut(start..)
        .ok_or(StatusCode::BadSecurityChecksFailed)
}

pub(super) fn security_byte(buf: &[u8], index: usize) -> Result<u8, Error> {
    buf.get(index)
        .copied()
        .ok_or_else(|| Error::new(StatusCode::BadSecurityChecksFailed, "invalid byte index"))
}

impl SecureChannel {
    fn decode_message_header(
        &self,
        src: &[u8],
    ) -> Result<(MessageChunkHeader, SecurityHeader, usize), Error> {
        let decoding_options = self.decoding_options();
        let mut stream = Cursor::new(&src);
        let message_header = MessageChunkHeader::decode(&mut stream, &decoding_options)?;
        let security_header = SecurityHeader::decode_with_known_policy(
            &mut stream,
            message_header.message_type.is_open_secure_channel(),
            &decoding_options,
            self.known_policy(),
        )?;
        let encrypted_data_offset = stream.position() as usize;
        if message_header.message_size as usize != src.len() {
            return Err(Error::new(
                StatusCode::BadUnexpectedError,
                format!(
                    "The message size {} is not the same as the supplied buffer {}",
                    message_header.message_size,
                    src.len()
                ),
            ));
        }

        Ok((message_header, security_header, encrypted_data_offset))
    }

    /// Applies security to a message chunk and yields a encrypted/signed block to be streamed
    pub fn apply_security(
        &self,
        message_chunk: &MessageChunk,
        dst: &mut [u8],
    ) -> Result<usize, StatusCode> {
        let size = if self.security_policy != SecurityPolicy::None
            && (self.security_mode == MessageSecurityMode::Sign
                || self.security_mode == MessageSecurityMode::SignAndEncrypt)
        {
            let encrypted_data_offset =
                message_chunk.encrypted_data_offset(&self.decoding_options())?;

            // S - Message Header
            // S - Security Header
            // S - Sequence Header - E
            // S - Body            - E
            // S - Padding         - E
            //     Signature       - E

            let encrypted_size = super::crypto::PADDING_AND_SIGNATURE_SCRATCH.with(|scratch| {
                let mut data = scratch.borrow_mut();
                let message_size =
                    self.add_space_for_padding_and_signature_into(message_chunk, &mut data)?;
                data.truncate(message_size);
                Self::log_crypto_data("Chunk before padding", &message_chunk.data[..]);
                Self::log_crypto_data("Chunk after padding", &data[..]);

                // Encrypted range is from the sequence header to the end
                let encrypted_range = encrypted_data_offset..data.len();

                // Encrypt and sign - open secure channel
                if message_chunk.is_open_secure_channel(&self.decoding_options()) {
                    self.asymmetric_sign_and_encrypt(
                        self.security_policy,
                        &mut data,
                        encrypted_range,
                        dst,
                    )
                } else {
                    // Symmetric encrypt and sign
                    let signed_range =
                        0..(data.len() - self.security_policy.symmetric_signature_size());
                    self.symmetric_sign_and_encrypt(&mut data, signed_range, encrypted_range, dst)
                }
            })?;

            Self::log_crypto_data(
                "Chunk after encryption",
                security_slice_to(dst, encrypted_size)?,
            );

            encrypted_size
        } else {
            let size = message_chunk.data.len();
            if size > dst.len() {
                error!(
                    "The size of the message chunk {} exceeds the size of the destination buffer {}",
                    size,
                    dst.len()
                );
                return Err(StatusCode::BadEncodingLimitsExceeded);
            }
            security_mut_slice(dst, 0..size)?
                .copy_from_slice(security_slice_to(&message_chunk.data, size)?);
            size
        };
        Ok(size)
    }

    fn decrypt_chunk(
        &self,
        src: bytes::Bytes,
        security_header: SecurityHeader,
        signed_range: Range<usize>,
        encrypted_range: Range<usize>,
        decrypted_data: &mut DecryptedChunkStorage,
    ) -> Result<MessageChunk, Error> {
        // Symmetric decrypt and verify
        trace!(
            "Decrypting block with signature info {:?} and encrypt info {:?}",
            signed_range,
            encrypted_range
        );

        let SecurityHeader::Symmetric(security_header) = security_header else {
            return Err(Error::new(
                StatusCode::BadUnexpectedError,
                format!("Expected symmetric security header, got {security_header:?}"),
            ));
        };

        decrypted_data.clear();
        decrypted_data.resize(encrypted_range.end, 0);
        let decrypted_size = self.symmetric_decrypt_and_verify(
            &src,
            signed_range,
            encrypted_range,
            security_header.token_id,
            &mut decrypted_data[..],
        )?;

        // Value returned from symmetric_decrypt_and_verify is the end of the actual decrypted data.
        Self::update_message_size(&mut decrypted_data[..], decrypted_size)?;
        let data = decrypted_data.split_to(decrypted_size).freeze();
        decrypted_data.reserve(src.len());
        Ok(MessageChunk {
            data,
            cached_chunk_info: std::sync::OnceLock::new(),
        })
    }

    /// Decrypts and verifies the body data if the mode / policy requires it
    pub fn verify_and_remove_security(
        &self,
        src: bytes::Bytes,
        decrypted_data: &mut DecryptedChunkStorage,
    ) -> Result<MessageChunk, Error> {
        // Get message & security header from data
        let (message_header, security_header, encrypted_data_offset) =
            self.decode_message_header(&src)?;
        let message_size = message_header.message_size as usize;

        // S - Message Header
        // S - Security Header
        // S - Sequence Header - E
        // S - Body            - E
        // S - Padding         - E
        //     Signature       - E
        if message_header.message_type.is_open_secure_channel() {
            let (_, encrypted_range) =
                Self::secure_message_ranges(message_size, encrypted_data_offset, 0)?;
            let (decrypted_chunk, _) =
                self.decrypt_open_secure_channel(src, security_header, encrypted_range)?;
            Ok(decrypted_chunk)
        } else if self.is_secure_connection() {
            let signature_size = self.security_policy.symmetric_signature_size();
            let (signed_range, encrypted_range) =
                Self::secure_message_ranges(message_size, encrypted_data_offset, signature_size)?;
            self.decrypt_chunk(
                src,
                security_header,
                signed_range,
                encrypted_range,
                decrypted_data,
            )
        } else {
            Ok(MessageChunk {
                data: src,
                cached_chunk_info: std::sync::OnceLock::new(),
            })
        }
    }

    /// Decrypts and verifies the body data if the mode / policy requires it.
    ///
    /// This is called on the server, and will also set the security policy of the channel
    /// if the message is an OpenSecureChannel request.
    pub fn verify_and_remove_security_server(
        &mut self,
        src: bytes::Bytes,
        decrypted_data: &mut DecryptedChunkStorage,
    ) -> Result<MessageChunk, Error> {
        // Get message & security header from data
        let (message_header, security_header, encrypted_data_offset) =
            self.decode_message_header(&src)?;
        let message_size = message_header.message_size as usize;

        // S - Message Header
        // S - Security Header
        // S - Sequence Header - E
        // S - Body            - E
        // S - Padding         - E
        //     Signature       - E
        if message_header.message_type.is_open_secure_channel() {
            // The OpenSecureChannel is the first thing we receive so we must examine
            // the security policy and use it to determine if the packet must be decrypted.

            let (_, encrypted_range) =
                Self::secure_message_ranges(message_size, encrypted_data_offset, 0)?;
            let (decrypted_chunk, security_policy) =
                self.decrypt_open_secure_channel(src, security_header, encrypted_range)?;
            self.set_security_policy(security_policy);
            Ok(decrypted_chunk)
        } else if self.is_secure_connection() {
            let signature_size = self.security_policy.symmetric_signature_size();
            let (signed_range, encrypted_range) =
                Self::secure_message_ranges(message_size, encrypted_data_offset, signature_size)?;
            self.decrypt_chunk(
                src,
                security_header,
                signed_range,
                encrypted_range,
                decrypted_data,
            )
        } else {
            Ok(MessageChunk {
                data: src,
                cached_chunk_info: std::sync::OnceLock::new(),
            })
        }
    }

    /// Decrypts and verifies the body data if the mode / policy requires it.
    ///
    /// Server-side async variant that offloads only the OpenSecureChannel
    /// asymmetric decrypt/verify leaf to Tokio's blocking pool.
    pub async fn verify_and_remove_security_server_async(
        &mut self,
        src: bytes::Bytes,
        decrypted_data: &mut DecryptedChunkStorage,
    ) -> Result<MessageChunk, Error> {
        // Get message & security header from data
        let (message_header, security_header, encrypted_data_offset) =
            self.decode_message_header(&src)?;
        let message_size = message_header.message_size as usize;

        // S - Message Header
        // S - Security Header
        // S - Sequence Header - E
        // S - Body            - E
        // S - Padding         - E
        //     Signature       - E
        if message_header.message_type.is_open_secure_channel() {
            // The OpenSecureChannel is the first thing we receive so we must examine
            // the security policy and use it to determine if the packet must be decrypted.

            let (_, encrypted_range) =
                Self::secure_message_ranges(message_size, encrypted_data_offset, 0)?;
            let (decrypted_chunk, security_policy) = self
                .decrypt_open_secure_channel_async(src, security_header, encrypted_range)
                .await?;
            self.set_security_policy(security_policy);
            Ok(decrypted_chunk)
        } else if self.is_secure_connection() {
            let signature_size = self.security_policy.symmetric_signature_size();
            let (signed_range, encrypted_range) =
                Self::secure_message_ranges(message_size, encrypted_data_offset, signature_size)?;
            self.decrypt_chunk(
                src,
                security_header,
                signed_range,
                encrypted_range,
                decrypted_data,
            )
        } else {
            Ok(MessageChunk {
                data: src,
                cached_chunk_info: std::sync::OnceLock::new(),
            })
        }
    }

    fn is_secure_connection(&self) -> bool {
        !matches!(self.security_policy, SecurityPolicy::None)
            && matches!(
                self.security_mode,
                MessageSecurityMode::Sign | MessageSecurityMode::SignAndEncrypt
            )
    }

    /// Makes a security header according to the type of message being sent, symmetric or asymmetric
    pub fn make_security_header(&self, message_type: MessageChunkType) -> SecurityHeader {
        match message_type {
            MessageChunkType::OpenSecureChannel => {
                let asymmetric_security_header = if self.security_policy == SecurityPolicy::None {
                    trace!("AsymmetricSecurityHeader security policy none");
                    AsymmetricSecurityHeader::none()
                } else {
                    let receiver_certificate_thumbprint =
                        if let Some(ref remote_cert) = self.remote_cert {
                            remote_cert.thumbprint().as_byte_string()
                        } else {
                            ByteString::null()
                        };
                    // Non-None policies require a configured local certificate.
                    #[allow(clippy::unwrap_used)]
                    let cert = self.cert.as_ref().unwrap();
                    AsymmetricSecurityHeader::new(
                        self.security_policy,
                        cert,
                        receiver_certificate_thumbprint,
                    )
                };
                trace!(
                    "AsymmetricSecurityHeader = {:?}",
                    asymmetric_security_header
                );
                SecurityHeader::Asymmetric(asymmetric_security_header)
            }
            _ => SecurityHeader::Symmetric(SymmetricSecurityHeader {
                token_id: self.token_id,
            }),
        }
    }

    /// Calculates the signature size for a message depending on the supplied security header
    pub fn signature_size(&self, security_header: &SecurityHeader) -> usize {
        // Signature size in bytes
        match security_header {
            SecurityHeader::Asymmetric(security_header) => {
                if self.security_policy.is_ecc() {
                    return self.private_key.as_ref().map(KeySize::size).unwrap_or(0);
                }

                if !security_header.sender_certificate.is_null() {
                    X509::from_byte_string(&security_header.sender_certificate)
                        .and_then(|x509| x509.public_key())
                        .map(|key| key.size())
                        .unwrap_or(0)
                } else {
                    trace!(
                        "No certificate / public key was supplied in the asymmetric security header"
                    );
                    0
                }
            }
            SecurityHeader::Symmetric(_) => {
                // Signature size comes from policy
                self.security_policy.symmetric_signature_size()
            }
        }
    }

    /// Get the plain text block size and minimum padding for this channel.
    /// Only makes sense if security policy is not None, and security mode is
    /// SignAndEncrypt
    pub fn get_padding_block_sizes(
        &self,
        security_header: &SecurityHeader,
        message_type: MessageChunkType,
    ) -> Result<(usize, usize), Error> {
        if self.security_policy == SecurityPolicy::None
            || self.security_mode != MessageSecurityMode::SignAndEncrypt
                && !message_type.is_open_secure_channel()
        {
            return Ok((0, 0));
        }

        match security_header {
            SecurityHeader::Asymmetric(security_header) => {
                if security_header.sender_certificate.is_null() {
                    error!(
                        "Sender has not supplied a certificate so it is doubtful that this will work"
                    );
                    Ok((self.security_policy.plain_block_size(), 1))
                } else {
                    // Padding requires we look at the remote certificate and security policy
                    let x509 = self.remote_cert().ok_or_else(|| {
                        Error::new(
                            StatusCode::BadCertificateInvalid,
                            "Missing server certificate, this is required for asymmetric encryption"
                        )
                    })?;
                    let pk = x509.public_key()?;
                    let padding = self.security_policy.asymmetric_padding_info(&pk);
                    Ok((padding.block_size, padding.minimum_padding))
                }
            }
            SecurityHeader::Symmetric(_) => {
                // Plain text block size comes from policy
                let padding = self.security_policy.symmetric_padding_info();
                Ok((padding.block_size, padding.minimum_padding))
            }
        }
    }

    /// Calculate the padding size
    ///
    /// Padding adds bytes to the body to make it a multiple of the block size so it can be encrypted.
    fn padding_size(
        &self,
        security_header: &SecurityHeader,
        body_size: usize,
        signature_size: usize,
        message_type: MessageChunkType,
    ) -> Result<(usize, usize), Error> {
        let (plain_text_block_size, minimum_padding) =
            self.get_padding_block_sizes(security_header, message_type)?;

        if plain_text_block_size == 0 {
            return Ok((0, 0));
        }

        // PaddingSize = PlainTextBlockSize – ((BytesToWrite + SignatureSize + 1) % PlainTextBlockSize);
        let encrypt_size = 8 + body_size + signature_size + minimum_padding;
        let padding_size = if !encrypt_size.is_multiple_of(plain_text_block_size) {
            plain_text_block_size - (encrypt_size % plain_text_block_size)
        } else {
            0
        };
        trace!(
            "sequence_header(8) + body({}) + signature ({}) = plain text size = {} / with padding {} = {}, plain_text_block_size = {}",
            body_size,
            signature_size,
            encrypt_size,
            padding_size,
            encrypt_size + padding_size,
            plain_text_block_size
        );
        Ok((minimum_padding + padding_size, minimum_padding))
    }

    // Takes an unpadded message chunk and adds padding as well as space to the end to accomodate a signature.
    // Also modifies the message size to include the new padding/signature
    pub(super) fn add_space_for_padding_and_signature_into(
        &self,
        message_chunk: &MessageChunk,
        data: &mut Vec<u8>,
    ) -> Result<usize, Error> {
        let chunk_info = message_chunk.chunk_info(self)?;
        let message_data = &message_chunk.data[..];

        let security_header = chunk_info.security_header;

        // Signature size (if required)
        let signature_size = self.signature_size(&security_header);

        // Write padding
        let body_size = chunk_info.body_length;

        let (padding_size, minimum_padding) = self.padding_size(
            &security_header,
            body_size,
            signature_size,
            chunk_info.message_header.message_type,
        )?;

        let message_size = message_data.len() + padding_size + signature_size;
        data.clear();
        data.reserve(message_size);

        // First off just write out the src to the buffer. The message header, security header, sequence header and payload
        data.write_all(message_data)?;

        if padding_size > 0 {
            // A number of bytes are written out equal to the padding size.
            // Each byte is the padding size. So if padding size is 15 then
            // there will be 15 bytes all with the value 15
            if minimum_padding == 1 {
                let padding_byte = ((padding_size - 1) & 0xff) as u8;
                let _ = write_bytes(data, padding_byte, padding_size)?;
            } else if minimum_padding == 2 {
                // Padding and then extra padding
                let padding_byte = ((padding_size - 2) & 0xff) as u8;
                let extra_padding_byte = ((padding_size - 2) >> 8) as u8;
                trace!(
                    "adding extra padding - padding_byte = {}, extra_padding_byte = {}",
                    padding_byte,
                    extra_padding_byte
                );
                let _ = write_bytes(data, padding_byte, padding_size - 1)?;
                write_u8(data, extra_padding_byte)?;
            }
        }

        // Write zeros for the signature
        let _ = write_bytes(data, 0u8, signature_size)?;

        // Update message header to reflect size with padding + signature
        Self::update_message_size(&mut data[..], message_size)?;
        Ok(message_size)
    }

    pub(super) fn update_message_size(data: &mut [u8], message_size: usize) -> Result<(), Error> {
        // Read and rewrite the message_size in the header
        let mut stream = Cursor::new(data);
        stream.advance(MESSAGE_SIZE_OFFSET);
        write_u32(&mut stream, message_size as u32)
    }

    /// Writes message size and truncates the message to fit.
    pub fn update_message_size_and_truncate(
        mut data: Vec<u8>,
        message_size: usize,
    ) -> Result<Vec<u8>, Error> {
        Self::update_message_size(&mut data[..], message_size)?;
        // Truncate vector to the size
        data.truncate(message_size);
        Ok(data)
    }

    pub(super) fn secure_message_ranges(
        message_size: usize,
        encrypted_data_offset: usize,
        signature_size: usize,
    ) -> Result<(Range<usize>, Range<usize>), Error> {
        if message_size < encrypted_data_offset {
            return Err(Error::new(
                StatusCode::BadSecurityChecksFailed,
                format!(
                    "Chunk message_size {message_size} is smaller than the encrypted data offset {encrypted_data_offset}"
                ),
            ));
        }
        let signed_end = message_size.checked_sub(signature_size).ok_or_else(|| {
            Error::new(
                StatusCode::BadSecurityChecksFailed,
                format!(
                    "Chunk message_size {message_size} is smaller than the {signature_size}-byte signature"
                ),
            )
        })?;
        Ok((0..signed_end, encrypted_data_offset..message_size))
    }

    /// Verify that the padding is correct. Padding is expected to be before the supplied padding end index.
    ///
    /// Function returns the padding range so caller can strip the range if it so desires.
    pub(super) fn verify_padding(
        src: &[u8],
        key_size: usize,
        padding_end: usize,
    ) -> Result<Range<usize>, Error> {
        let padding_range = if key_size > 256 {
            if padding_end < 2 {
                return Err(Error::new(
                    StatusCode::BadSecurityChecksFailed,
                    "invalid padding",
                ));
            }
            let padding_byte = security_byte(src, padding_end - 2)?;
            let extra_padding_byte = security_byte(src, padding_end - 1)?;
            let padding_size = ((extra_padding_byte as usize) << 8) + (padding_byte as usize);
            let padding_range_start =
                padding_end.checked_sub(padding_size + 2).ok_or_else(|| {
                    Error::new(
                        StatusCode::BadSecurityChecksFailed,
                        "padding size exceeds chunk",
                    )
                })?;
            let padding_range = padding_range_start..padding_end;

            trace!(
                "Extra padding - extra_padding_byte = {}, padding_byte = {}, padding_end = {}, padding_size = {}",
                extra_padding_byte, padding_byte, padding_end, padding_size
            );

            // Check padding bytes and extra padding byte
            Self::check_padding_bytes(
                src.get(padding_range.start..(padding_range.end - 1))
                    .ok_or_else(|| {
                        Error::new(StatusCode::BadSecurityChecksFailed, "invalid padding range")
                    })?,
                padding_byte,
                padding_range.start,
            )?;
            if security_byte(src, padding_range.end - 1)? != extra_padding_byte {
                return Err(Error::new(
                    StatusCode::BadSecurityChecksFailed,
                    format!(
                        "Expected extra padding byte {}, at index {}",
                        extra_padding_byte, padding_range.start
                    ),
                ));
            }
            padding_range
        } else {
            if padding_end == 0 {
                return Err(Error::new(
                    StatusCode::BadSecurityChecksFailed,
                    "invalid padding",
                ));
            }
            let padding_byte = security_byte(src, padding_end - 1)?;
            let padding_size = padding_byte as usize;
            let padding_range_start =
                padding_end.checked_sub(padding_size + 1).ok_or_else(|| {
                    Error::new(
                        StatusCode::BadSecurityChecksFailed,
                        "padding size exceeds chunk",
                    )
                })?;
            let padding_range = padding_range_start..padding_end;
            // Check padding bytes
            Self::check_padding_bytes(
                src.get(padding_range.clone()).ok_or_else(|| {
                    Error::new(StatusCode::BadSecurityChecksFailed, "invalid padding range")
                })?,
                padding_byte,
                padding_range.start,
            )?;
            padding_range
        };
        trace!("padding_range = {:?}", padding_range);
        Ok(padding_range)
    }

    fn check_padding_bytes(
        padding_bytes: &[u8],
        expected_padding_byte: u8,
        padding_range_start: usize,
    ) -> Result<(), Error> {
        for (i, b) in padding_bytes.iter().enumerate() {
            if *b != expected_padding_byte {
                return Err(Error::new(
                    StatusCode::BadSecurityChecksFailed,
                    format!(
                        "Expected padding byte {}, got {} at index {}",
                        expected_padding_byte,
                        *b,
                        padding_range_start + i
                    ),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod secure_message_range_tests {
    use super::SecureChannel;

    #[test]
    fn secure_message_ranges_rejects_undersized_chunk() {
        // Regression (CRITICAL panic audit): a chunk whose message_size is smaller than
        // the symmetric signature (or the encrypted-data offset) must be rejected, not
        // underflow `message_size - signature_size` into an out-of-bounds slice panic.
        assert!(SecureChannel::secure_message_ranges(10, 8, 20).is_err()); // < signature
        assert!(SecureChannel::secure_message_ranges(4, 8, 2).is_err()); // < offset
                                                                         // Correctly-sized chunk still yields the expected ranges.
        let (signed, encrypted) = SecureChannel::secure_message_ranges(100, 8, 20).unwrap();
        assert_eq!(signed, 0..80);
        assert_eq!(encrypted, 8..100);
    }
}
