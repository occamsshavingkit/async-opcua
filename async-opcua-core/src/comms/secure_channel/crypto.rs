// OPCUA for Rust
// SPDX-License-Identifier: MPL-2.0
// Copyright (C) 2017-2024 Adam Lock

use std::ops::Range;

use tracing::trace;

use opcua_crypto::{KeySize, PrivateKey, PublicKey, SecurityPolicy, X509};
use opcua_types::{status_code::StatusCode, ByteString, Error, MessageSecurityMode};

use super::framing::{
    security_mut_slice, security_mut_slice_from, security_slice, security_slice_to,
};
use super::SecureChannel;

thread_local! {
    pub(super) static PADDING_AND_SIGNATURE_SCRATCH: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
    pub(super) static SYMMETRIC_DECRYPT_SCRATCH: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn asymmetric_sign_and_encrypt_owned(
    security_policy: SecurityPolicy,
    signing_key: PrivateKey,
    encryption_key: Option<PublicKey>,
    is_client_role: bool,
    apply_channel_thumbprint: bool,
    first_request_signature: Vec<u8>,
    mut src: Vec<u8>,
    encrypted_range: Range<usize>,
) -> Result<(Vec<u8>, Option<Vec<u8>>), StatusCode> {
    #[cfg(not(feature = "ecc"))]
    let _ = (
        is_client_role,
        apply_channel_thumbprint,
        &first_request_signature,
    );

    let header_size = encrypted_range.start;

    let signing_key_size = signing_key.size();
    let is_ecc = security_policy.is_ecc();

    let signed_range_end = encrypted_range
        .end
        .checked_sub(signing_key_size)
        .ok_or(StatusCode::BadSecurityChecksFailed)?;
    let signed_range = 0..signed_range_end;
    let signature_range = signed_range.end..encrypted_range.end;

    trace!(
        "Header size = {}, Encrypted range = {:?}, Signed range = {:?}, Signature range = {:?}, signature size = {}",
        header_size, encrypted_range, signed_range, signature_range, signing_key_size
    );

    // Encryption will change the size of the chunk. Since we sign before encrypting, we need to
    // compute that size and change the message header to be that new size.
    let cipher_text_size = {
        let plain_text_size = encrypted_range
            .end
            .checked_sub(encrypted_range.start)
            .ok_or(StatusCode::BadSecurityChecksFailed)?;
        let cipher_text_size = if let Some(encryption_key) = &encryption_key {
            security_policy.calculate_cipher_text_size(plain_text_size, encryption_key)
        } else {
            plain_text_size
        };
        trace!(
            "plain_text_size = {}, encrypted_text_size = {}",
            plain_text_size,
            cipher_text_size
        );
        cipher_text_size
    };
    let output_size = header_size
        .checked_add(cipher_text_size)
        .ok_or(StatusCode::BadSecurityChecksFailed)?;
    let mut dst = vec![0; output_size];

    SecureChannel::update_message_size(&mut src, output_size)?;
    security_mut_slice(&mut dst, 0..encrypted_range.start)?
        .copy_from_slice(security_slice(&src, 0..encrypted_range.start)?);

    #[cfg(feature = "ecc")]
    let mut first_request_signature_to_store = None;
    #[cfg(not(feature = "ecc"))]
    let first_request_signature_to_store = None;
    {
        // Sign the message header, security header, sequence header, body, padding.
        let signed_and_signature = security_mut_slice(&mut src, 0..encrypted_range.end)?;
        let (l, r) = signed_and_signature.split_at_mut(signed_range.end);
        let signature = r
            .get_mut(..signing_key_size)
            .ok_or(StatusCode::BadSecurityChecksFailed)?;
        if is_ecc {
            #[cfg(feature = "ecc")]
            {
                if is_client_role {
                    security_policy.asymmetric_sign(&signing_key, l, signature)?;
                    first_request_signature_to_store = Some(signature.to_vec());
                } else if apply_channel_thumbprint {
                    let mut signed_data =
                        Vec::with_capacity(l.len() + first_request_signature.len());
                    signed_data.extend_from_slice(l);
                    signed_data.extend_from_slice(&first_request_signature);
                    security_policy.asymmetric_sign(&signing_key, &signed_data, signature)?;
                } else {
                    security_policy.asymmetric_sign(&signing_key, l, signature)?;
                }
            }
        } else {
            security_policy.asymmetric_sign(&signing_key, l, signature)?;
        }
    }

    if encrypted_range.end != signature_range.end {
        return Err(StatusCode::BadSecurityChecksFailed);
    }

    if is_ecc {
        security_mut_slice(&mut dst, encrypted_range.clone())?
            .copy_from_slice(security_slice(&src, encrypted_range.clone())?);
        SecureChannel::log_crypto_data(
            "Chunk after signing",
            security_slice_to(&dst, signature_range.end)?,
        );
        return Ok((dst, first_request_signature_to_store));
    }

    SecureChannel::log_crypto_data(
        "Chunk after signing",
        security_slice_to(&dst, signature_range.end)?,
    );

    // Encrypt the sequence header, payload, signature portion into dst.
    let encrypted_size = security_policy.asymmetric_encrypt(
        encryption_key
            .as_ref()
            .ok_or(StatusCode::BadSecurityChecksFailed)?,
        security_slice(&src, encrypted_range.clone())?,
        security_mut_slice_from(&mut dst, encrypted_range.start)?,
    )?;

    // Validate encrypted size is right.
    if encrypted_size != cipher_text_size {
        return Err(StatusCode::BadSecurityChecksFailed);
    }

    // {
    //    debug!("Encrypted size in bytes = {} compared to encrypted range {:?}", encrypted_size, encrypted_range);
    //    SecureChannel::log_crypto_data("Decrypted data", &src);
    //    SecureChannel::log_crypto_data("Encrypted data", &dst[0..encrypted_size]);
    // }

    Ok((dst, first_request_signature_to_store))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn asymmetric_decrypt_and_verify_owned(
    security_policy: SecurityPolicy,
    our_private_key: PrivateKey,
    our_cert: X509,
    verification_key: PublicKey,
    receiver_thumbprint: ByteString,
    is_client_role: bool,
    apply_channel_thumbprint: bool,
    first_request_signature: Vec<u8>,
    src: Vec<u8>,
    encrypted_range: Range<usize>,
) -> Result<Vec<u8>, Error> {
    #[cfg(not(feature = "ecc"))]
    let _ = (
        is_client_role,
        apply_channel_thumbprint,
        &first_request_signature,
    );

    // Asymmetric encrypt requires the caller supply the security policy
    if !security_policy.is_supported() {
        return Err(Error::new(
            StatusCode::BadSecurityPolicyRejected,
            format!(
                "Security policy {security_policy} is not supported by asymmetric_decrypt_and_verify and has been rejected"
            ),
        ));
    }

    // Unlike the symmetric_decrypt_and_verify, this code will ALWAYS decrypt and verify regardless
    // of security mode. This is part of the OpenSecureChannel request on a sign / signencrypt
    // mode connection.

    // The sender_certificate is is the cert used to sign the message, i.e. the client's cert
    //
    // The receiver certificate thumbprint identifies which of our certs was used by the client
    // to encrypt the message. We have to work out from the thumbprint which cert to use

    let our_thumbprint = our_cert.thumbprint();
    if our_thumbprint.value() != receiver_thumbprint.as_ref() {
        return Err(Error::new(
            StatusCode::BadNoValidCertificates,
            "Supplied thumbprint does not match application certificate's thumbprint",
        ));
    }

    let mut dst = vec![0u8; encrypted_range.end];

    // Copy message, security header
    dst.get_mut(..encrypted_range.start)
        .ok_or_else(|| {
            Error::new(
                StatusCode::BadSecurityChecksFailed,
                "invalid encrypted range",
            )
        })?
        .copy_from_slice(src.get(..encrypted_range.start).ok_or_else(|| {
            Error::new(
                StatusCode::BadSecurityChecksFailed,
                "invalid encrypted range",
            )
        })?);

    let encrypted_size = encrypted_range
        .end
        .checked_sub(encrypted_range.start)
        .ok_or_else(|| {
            Error::new(
                StatusCode::BadSecurityChecksFailed,
                "invalid encrypted range",
            )
        })?;
    let decrypted_size = if security_policy.is_ecc() {
        dst.get_mut(encrypted_range.clone())
            .ok_or_else(|| {
                Error::new(
                    StatusCode::BadSecurityChecksFailed,
                    "invalid encrypted range",
                )
            })?
            .copy_from_slice(src.get(encrypted_range.clone()).ok_or_else(|| {
                Error::new(
                    StatusCode::BadSecurityChecksFailed,
                    "invalid encrypted range",
                )
            })?);
        encrypted_size
    } else {
        // Note that the unencrypted size can be less than the encrypted size due to removal
        // of padding, so the ranges that were supplied to this function must be offset to compensate.
        trace!("Decrypting message range {:?}", encrypted_range);
        let mut decrypted_tmp = vec![0u8; encrypted_size];

        let decrypted_size = security_policy.asymmetric_decrypt(
            &our_private_key,
            src.get(encrypted_range.clone()).ok_or_else(|| {
                Error::new(
                    StatusCode::BadSecurityChecksFailed,
                    "invalid encrypted range",
                )
            })?,
            &mut decrypted_tmp,
        )?;
        let decrypted_end = encrypted_range
            .start
            .checked_add(decrypted_size)
            .filter(|end| *end <= dst.len())
            .ok_or_else(|| {
                Error::new(
                    StatusCode::BadSecurityChecksFailed,
                    "decrypted chunk exceeds message buffer",
                )
            })?;

        dst.get_mut(encrypted_range.start..decrypted_end)
            .ok_or_else(|| {
                Error::new(
                    StatusCode::BadSecurityChecksFailed,
                    "invalid decrypted range",
                )
            })?
            .copy_from_slice(decrypted_tmp.get(..decrypted_size).ok_or_else(|| {
                Error::new(
                    StatusCode::BadSecurityChecksFailed,
                    "invalid decrypted range",
                )
            })?);
        decrypted_size
    };
    trace!(
        "Decrypted bytes = {} compared to encrypted range {}",
        decrypted_size,
        encrypted_size
    );
    // SecureChannel::log_crypto_data("Decrypted Bytes = ", &decrypted_tmp[..decrypted_size]);

    let verification_key_signature_size =
        security_policy.asymmetric_signature_size(&verification_key);
    trace!(
        "Verification key size = {}",
        verification_key_signature_size
    );

    let decrypted_end = encrypted_range
        .start
        .checked_add(decrypted_size)
        .filter(|end| *end <= dst.len())
        .ok_or_else(|| {
            Error::new(
                StatusCode::BadSecurityChecksFailed,
                "decrypted chunk exceeds message buffer",
            )
        })?;

    // The signature range is at the end of the decrypted block for the verification key's signature
    let signature_dst_offset = decrypted_end
        .checked_sub(verification_key_signature_size)
        .ok_or_else(|| {
            Error::new(
                StatusCode::BadSecurityChecksFailed,
                "decrypted chunk is smaller than the signature",
            )
        })?;
    let signature_range_dst = signature_dst_offset..decrypted_end;

    // The signed range is from 0 to the end of the plaintext except for key size
    let signed_range_dst = 0..signature_dst_offset;

    // SecureChannel::log_crypto_data("Decrypted data = ", &dst[..signature_range_dst.end]);

    // Verify signature (contained encrypted portion) using verification key
    trace!(
        "Verifying signature range {:?} with signature at {:?}",
        signed_range_dst,
        signature_range_dst
    );
    // Keysize for padding is publickey length if avaiable
    let key_size = if let Ok(cert) = our_cert.public_key() {
        cert.size()
    } else {
        verification_key.size()
    };
    let signed_data = dst
        .get(signed_range_dst)
        .ok_or_else(|| Error::new(StatusCode::BadSecurityChecksFailed, "invalid signed range"))?;
    let signature = dst.get(signature_range_dst.clone()).ok_or_else(|| {
        Error::new(
            StatusCode::BadSecurityChecksFailed,
            "invalid signature range",
        )
    })?;
    if security_policy.is_ecc() {
        #[cfg(feature = "ecc")]
        {
            if is_client_role && apply_channel_thumbprint {
                let mut signed_data_with_thumbprint =
                    Vec::with_capacity(signed_data.len() + first_request_signature.len());
                signed_data_with_thumbprint.extend_from_slice(signed_data);
                signed_data_with_thumbprint.extend_from_slice(&first_request_signature);
                security_policy.asymmetric_verify_signature(
                    &verification_key,
                    &signed_data_with_thumbprint,
                    signature,
                )?;
            } else {
                security_policy.asymmetric_verify_signature(
                    &verification_key,
                    signed_data,
                    signature,
                )?;
            }
        }
    } else {
        security_policy.asymmetric_verify_signature(&verification_key, signed_data, signature)?;
    }

    let decrypted_verified_size = if security_policy.is_ecc() {
        signature_range_dst.start
    } else {
        // Verify that the padding is correct
        let padding_range =
            SecureChannel::verify_padding(&dst, key_size, signature_range_dst.start)?;

        // Decrypted and verified into dst
        padding_range.start
    };
    dst.truncate(decrypted_verified_size);
    Ok(dst)
}

impl SecureChannel {
    /// Use the security policy to asymmetric encrypt and sign the specified chunk of data.
    /// Keeps the borrowed-buffer API while delegating crypto to the owned core.
    pub(super) fn asymmetric_sign_and_encrypt(
        &self,
        security_policy: SecurityPolicy,
        src: &mut [u8],
        encrypted_range: Range<usize>,
        dst: &mut [u8],
    ) -> Result<usize, StatusCode> {
        let signing_key = self
            .private_key
            .as_ref()
            .ok_or(StatusCode::BadSecurityChecksFailed)?
            .clone();
        let is_ecc = security_policy.is_ecc();

        let encryption_key = if is_ecc {
            None
        } else {
            Some(
                self.remote_cert
                    .as_ref()
                    .ok_or(StatusCode::BadSecurityChecksFailed)?
                    .public_key()?,
            )
        };

        let is_client_role = self.is_client_role();
        #[cfg(feature = "ecc")]
        let apply_channel_thumbprint = self.apply_channel_thumbprint;
        #[cfg(not(feature = "ecc"))]
        let apply_channel_thumbprint = false;
        #[cfg(feature = "ecc")]
        let first_request_signature = self.first_request_signature.lock().clone();
        #[cfg(not(feature = "ecc"))]
        let first_request_signature = Vec::new();

        let (secured_chunk, first_request_signature_to_store) = asymmetric_sign_and_encrypt_owned(
            security_policy,
            signing_key,
            encryption_key,
            is_client_role,
            apply_channel_thumbprint,
            first_request_signature,
            src.to_vec(),
            encrypted_range,
        )?;

        let secured_size = secured_chunk.len();
        security_mut_slice(dst, 0..secured_size)?.copy_from_slice(&secured_chunk);

        #[cfg(feature = "ecc")]
        if is_client_role {
            if let Some(first_request_signature_to_store) = first_request_signature_to_store {
                let mut first_request_signature = self.first_request_signature.lock();
                first_request_signature.clear();
                first_request_signature.extend_from_slice(&first_request_signature_to_store);
            }
        }
        #[cfg(not(feature = "ecc"))]
        let _ = first_request_signature_to_store;

        Ok(secured_size)
    }

    /// Encode data using security. Destination buffer is expected to be same size as src and expected
    /// to have space for for a signature if a signature is to be appended
    ///
    /// Signing is done first and then encryption
    ///
    /// S - Message Header
    /// S - Security Header
    /// S - Sequence Header - E
    /// S - Body            - E
    /// S - Padding         - E
    ///     Signature       - E
    pub fn symmetric_sign_and_encrypt(
        &self,
        src: &mut [u8],
        signed_range: Range<usize>,
        encrypted_range: Range<usize>,
        dst: &mut [u8],
    ) -> Result<usize, StatusCode> {
        let encrypted_size = match self.security_mode {
            MessageSecurityMode::None => {
                trace!("encrypt_and_sign is doing nothing because security mode == None");
                // Just copy data to out
                dst.copy_from_slice(src);

                src.len()
            }
            MessageSecurityMode::Sign => {
                trace!("encrypt_and_sign security mode == Sign");
                self.expect_supported_security_policy().map_err(|code| {
                    Error::new(code, "Unsupported security policy for symmetric verify")
                })?;
                let size = self.symmetric_sign_in_place(src, signed_range)?;
                security_mut_slice(dst, 0..size)?.copy_from_slice(security_slice(src, 0..size)?);
                size
            }
            MessageSecurityMode::SignAndEncrypt => {
                trace!(
                    "encrypt_and_sign security mode == SignAndEncrypt, signed_range = {:?}, encrypted_range = {:?}",
                    signed_range, encrypted_range
                );
                self.expect_supported_security_policy().map_err(|code| {
                    Error::new(code, "Unsupported security policy for symmetric decrypt")
                })?;

                // Sign the block
                self.symmetric_sign_in_place(src, signed_range)?;

                // Encrypt the sequence header, payload, signature
                let keys = self.local_keys()?;
                let encrypted_size = self.security_policy.symmetric_encrypt(
                    keys,
                    security_slice(src, encrypted_range.clone())?,
                    security_mut_slice_from(dst, encrypted_range.start)?,
                )?;
                // Copy the message header / security header
                security_mut_slice(dst, 0..encrypted_range.start)?
                    .copy_from_slice(security_slice(src, 0..encrypted_range.start)?);

                encrypted_range.start + encrypted_size
            }
            MessageSecurityMode::Invalid => {
                return Err(StatusCode::BadSecurityModeRejected);
            }
        };
        Ok(encrypted_size)
    }

    fn symmetric_sign_in_place(
        &self,
        buf: &mut [u8],
        signed_range: Range<usize>,
    ) -> Result<usize, StatusCode> {
        let signature_size = self.security_policy.symmetric_signature_size();
        trace!(
            "signed_range = {:?}, signature len = {}",
            signed_range,
            signature_size
        );

        // Sign the message header, security header, sequence header, body, padding
        if signed_range.end > buf.len() {
            return Err(StatusCode::BadSecurityChecksFailed);
        }
        let (l, r) = buf.split_at_mut(signed_range.end);
        let signature = r
            .get_mut(..signature_size)
            .ok_or(StatusCode::BadSecurityChecksFailed)?;
        self.security_policy
            .symmetric_sign(self.local_keys()?, l, signature)?;

        Ok(signed_range.end + signature_size)
    }

    /// Decrypts and verifies data.
    ///
    /// Returns the size of the decrypted data
    ///
    /// S - Message Header
    /// S - Security Header
    /// S - Sequence Header - E
    /// S - Body            - E
    /// S - Padding         - E
    ///     Signature       - E
    pub fn symmetric_decrypt_and_verify(
        &self,
        src: &[u8],
        signed_range: Range<usize>,
        encrypted_range: Range<usize>,
        token_id: u32,
        dst: &mut [u8],
    ) -> Result<usize, Error> {
        match self.security_mode {
            MessageSecurityMode::None => {
                // Just copy everything from src to dst
                dst[..].copy_from_slice(src);
                Ok(src.len())
            }
            MessageSecurityMode::Sign => {
                self.expect_supported_security_policy().map_err(|code| {
                    Error::new(code, "Unsupported security policy for symmetric verify")
                })?;
                dst.copy_from_slice(src);
                // Copy everything
                let signature_range = signed_range.end..src.len();
                trace!(
                    "signed range = {:?}, signature range = {:?}",
                    signed_range,
                    signature_range
                );
                let verification_key = self.get_remote_keys(token_id).ok_or_else(|| {
                    Error::new(
                        StatusCode::BadSecureChannelClosed,
                        "Missing verification key",
                    )
                })?;
                self.security_policy.symmetric_verify_signature(
                    verification_key,
                    dst.get(signed_range.clone()).ok_or_else(|| {
                        Error::new(StatusCode::BadSecurityChecksFailed, "invalid signed range")
                    })?,
                    dst.get(signature_range).ok_or_else(|| {
                        Error::new(
                            StatusCode::BadSecurityChecksFailed,
                            "invalid signature range",
                        )
                    })?,
                )?;

                Ok(signed_range.end)
            }
            MessageSecurityMode::SignAndEncrypt => {
                self.expect_supported_security_policy().map_err(|code| {
                    Error::new(code, "Unsupported security policy for symmetric decrypt")
                })?;

                // There is an expectation that the block is padded so, this is a quick test
                let ciphertext_size = encrypted_range.end - encrypted_range.start;
                //                if ciphertext_size % 16 != 0 {
                //                    error!("The cipher text size is not padded properly, size = {}", ciphertext_size);
                //                    return Err(StatusCode::BadUnexpectedError);
                //                }

                // Copy security header
                dst.get_mut(..encrypted_range.start)
                    .ok_or_else(|| {
                        Error::new(
                            StatusCode::BadSecurityChecksFailed,
                            "invalid encrypted range",
                        )
                    })?
                    .copy_from_slice(src.get(..encrypted_range.start).ok_or_else(|| {
                        Error::new(
                            StatusCode::BadSecurityChecksFailed,
                            "invalid encrypted range",
                        )
                    })?);

                let keys = self.get_remote_keys(token_id).ok_or_else(|| {
                    Error::new(
                        StatusCode::BadSecureChannelClosed,
                        "Missing decryption keys",
                    )
                })?;

                trace!(
                    "Secure decrypt called with encrypted range {:?}",
                    encrypted_range
                );
                let decrypted_size = SYMMETRIC_DECRYPT_SCRATCH.with(|scratch| {
                    let mut decrypted_tmp = scratch.borrow_mut();
                    decrypted_tmp.clear();
                    decrypted_tmp.resize(ciphertext_size + 16, 0);
                    let decrypted_size = self.security_policy.symmetric_decrypt(
                        keys,
                        src.get(encrypted_range.clone()).ok_or_else(|| {
                            Error::new(
                                StatusCode::BadSecurityChecksFailed,
                                "invalid encrypted range",
                            )
                        })?,
                        decrypted_tmp.as_mut_slice(),
                    )?;

                    // Self::log_crypto_data("Encrypted buffer", &src[..encrypted_range.end]);
                    let decrypted_end = encrypted_range
                        .start
                        .checked_add(decrypted_size)
                        .filter(|end| *end <= dst.len())
                        .ok_or_else(|| {
                            Error::new(
                                StatusCode::BadSecurityChecksFailed,
                                "decrypted chunk exceeds message buffer",
                            )
                        })?;
                    let decrypted_range = encrypted_range.start..decrypted_end;
                    dst.get_mut(decrypted_range)
                        .ok_or_else(|| {
                            Error::new(
                                StatusCode::BadSecurityChecksFailed,
                                "invalid decrypted range",
                            )
                        })?
                        .copy_from_slice(decrypted_tmp.get(..decrypted_size).ok_or_else(|| {
                            Error::new(
                                StatusCode::BadSecurityChecksFailed,
                                "invalid decrypted range",
                            )
                        })?);
                    Ok::<usize, Error>(decrypted_size)
                })?;

                let decrypted_end = encrypted_range
                    .start
                    .checked_add(decrypted_size)
                    .filter(|end| *end <= dst.len())
                    .ok_or_else(|| {
                        Error::new(
                            StatusCode::BadSecurityChecksFailed,
                            "decrypted chunk exceeds message buffer",
                        )
                    })?;
                let encrypted_range = encrypted_range.start..decrypted_end;
                Self::log_crypto_data(
                    "Decrypted buffer",
                    dst.get(..encrypted_range.end).ok_or_else(|| {
                        Error::new(
                            StatusCode::BadSecurityChecksFailed,
                            "invalid decrypted range",
                        )
                    })?,
                );

                // Verify signature (after encrypted portion)
                let signature_start = encrypted_range
                    .end
                    .checked_sub(self.security_policy.symmetric_signature_size())
                    .ok_or_else(|| {
                        Error::new(
                            StatusCode::BadSecurityChecksFailed,
                            "decrypted chunk is smaller than the signature",
                        )
                    })?;
                let signature_range = signature_start..encrypted_range.end;
                trace!(
                    "signed range = {:?}, signature range = {:?}",
                    signed_range,
                    signature_range
                );
                self.security_policy.symmetric_verify_signature(
                    keys,
                    dst.get(signed_range).ok_or_else(|| {
                        Error::new(StatusCode::BadSecurityChecksFailed, "invalid signed range")
                    })?,
                    dst.get(signature_range).ok_or_else(|| {
                        Error::new(
                            StatusCode::BadSecurityChecksFailed,
                            "invalid signature range",
                        )
                    })?,
                )?;

                let key_size = self.security_policy.encrypting_key_length();

                // Verify that the padding is correct and get the padded range.
                let padding_range = SecureChannel::verify_padding(dst, key_size, signature_start)?;

                // Decrypted range minus padding and signature.
                Ok(padding_range.start)
            }
            MessageSecurityMode::Invalid => {
                // Use the security policy to decrypt the block using the token
                Err(Error::new(
                    StatusCode::BadSecurityModeRejected,
                    "Message security mode is invalid",
                ))
            }
        }
    }
}

#[cfg(test)]
mod crypto_offload_tests {
    use std::time::Duration;

    use tokio::time::{sleep, timeout, Instant};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn blocking_crypto_work_does_not_starve_runtime_timers() {
        let started = Instant::now();
        let timer = tokio::spawn(async move {
            sleep(Duration::from_millis(25)).await;
            started.elapsed()
        });

        let crypto = tokio::task::spawn_blocking(|| {
            let deadline = std::time::Instant::now() + Duration::from_millis(250);
            let mut accumulator = 0_u64;
            while std::time::Instant::now() < deadline {
                accumulator = accumulator.wrapping_add(1);
            }
            accumulator
        });

        let timer_elapsed = timeout(Duration::from_millis(150), timer)
            .await
            .expect("runtime timer should not be starved by blocking crypto work")
            .unwrap();
        assert!(
            timer_elapsed < Duration::from_millis(150),
            "runtime timer was delayed by blocking crypto work: {timer_elapsed:?}"
        );

        assert!(crypto.await.unwrap() > 0);
    }
}
