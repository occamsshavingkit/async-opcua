// OPCUA for Rust
// SPDX-License-Identifier: MPL-2.0
// Copyright (C) 2017-2024 Adam Lock

//! The secure channel handles security on an OPC-UA connection.

use std::sync::Arc;
use std::{collections::HashMap, ops::Range};

use tracing::{error, trace};

#[cfg(feature = "ecc")]
use opcua_crypto::ecc::EphemeralPrivateKey;
use opcua_crypto::{PrivateKey, PublicKey, SecurityPolicy, X509};
use opcua_types::{
    status_code::StatusCode, ByteString, ContextOwned, DateTime, Error, MessageSecurityMode,
};
#[cfg(feature = "ecc")]
use parking_lot::Mutex;
use parking_lot::RwLock;

use crate::comms::{
    crypto_offload::CryptoOffload, message_chunk::MessageChunk, security_header::SecurityHeader,
};

// Submodules
mod crypto;
mod framing;
mod state;

// Re-exports
pub(crate) use crypto::{asymmetric_decrypt_and_verify_owned, asymmetric_sign_and_encrypt_owned};
use state::RemoteKeys;

/// Reusable scratch storage for secured chunk decryption.
pub type DecryptedChunkStorage = bytes::BytesMut;

#[derive(Debug, PartialEq)]
/// Role of an application in OPC-UA communication.
pub enum Role {
    /// Role is unknown.
    Unknown,
    /// Role is client.
    Client,
    /// Role is server.
    Server,
}

/// Holds all of the security information related to this session
#[derive(Debug)]
pub struct SecureChannel {
    // The side of the secure channel that this role belongs to, client or server
    role: Role,
    /// Whether deprecated (legacy) security policies may be negotiated on
    /// this channel. Servers set this from their runtime configuration.
    allow_deprecated: bool,
    /// The security policy for the connection, None or Encryption/Signing settings
    security_policy: SecurityPolicy,
    /// The security mode for the connection, None, Sign, SignAndEncrypt
    security_mode: MessageSecurityMode,
    /// Secure channel id
    secure_channel_id: u32,
    /// Token creation time.
    token_created_at: DateTime,
    /// Token lifetime
    token_lifetime: u32,
    /// Token identifier
    token_id: u32,
    /// Our certificate
    cert: Option<X509>,
    /// Our private key
    private_key: Option<PrivateKey>,
    /// Their certificate
    remote_cert: Option<X509>,
    /// Their nonce provided by open secure channel
    remote_nonce: Vec<u8>,
    /// Our nonce generated while handling open secure channel
    local_nonce: Vec<u8>,
    /// Our ephemeral private key for ECC secure-channel ECDH.
    #[cfg(feature = "ecc")]
    local_ephemeral_key: Option<EphemeralPrivateKey>,
    /// Signature from the first OpenSecureChannel request for ECC channel thumbprints.
    #[cfg(feature = "ecc")]
    first_request_signature: Mutex<Vec<u8>>,
    /// Whether to apply the ECC ChannelThumbprint calculation to the next response.
    #[cfg(feature = "ecc")]
    apply_channel_thumbprint: bool,
    /// Client (i.e. other end's set of keys) Symmetric Signing Key, Encrypt Key, IV
    ///
    /// This is a map of channel token ids and their respective keys. We need to keep
    /// the old keys around as the client should accept messages secured by an expired
    /// SecurityToken for up to 25 % of the token lifetime.
    ///
    /// See the "OpenSecureChannel" section in the spec for more info:
    /// [Part 4, 5.5.2](https://reference.opcfoundation.org/Core/Part4/v105/docs/5.5.2)
    remote_keys: HashMap<u32, RemoteKeys>,
    /// Server (i.e. our end's set of keys) Symmetric Signing Key, Decrypt Key, IV
    local_keys: Option<opcua_crypto::AesDerivedKeys>,
    /// Peer/client `maxResponseMessageSize` response body limit; `None` means no limit.
    #[allow(dead_code)]
    peer_max_response_body_size: Option<usize>,
    /// Decoding options
    encoding_context: Arc<RwLock<ContextOwned>>,
    /// Whether the security policy has been pre-validated.
    security_policy_valid: bool,
    /// Optional dedicated crypto offload executor for asymmetric OSC crypto
    /// (T010A). When `Some`, handshake decrypt/verify and sign/encrypt run
    /// on this executor's workers instead of the shared `spawn_blocking`
    /// pool, giving the deployment a scheduling-priority seam.
    crypto_offload: Option<Arc<dyn CryptoOffload>>,
}

#[allow(clippy::large_enum_variant)]
enum PreparedOscDecrypt {
    Plain {
        message_chunk: MessageChunk,
        security_policy: SecurityPolicy,
    },
    Crypto(Box<PreparedOscDecryptCrypto>),
}

struct PreparedOscDecryptCrypto {
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
    #[cfg(feature = "ecc")]
    first_request_signature_to_store: Option<Result<Vec<u8>, Error>>,
    /// Dedicated crypto executor (T010A). `None` falls back to spawn_blocking.
    crypto_offload: Option<Arc<dyn CryptoOffload>>,
}

struct CompletedOscDecrypt {
    security_policy: SecurityPolicy,
    decrypted_chunk: Vec<u8>,
    #[cfg(feature = "ecc")]
    first_request_signature_to_store: Option<Result<Vec<u8>, Error>>,
}

impl PreparedOscDecryptCrypto {
    fn decrypt_inline(self) -> Result<CompletedOscDecrypt, Error> {
        let PreparedOscDecryptCrypto {
            security_policy,
            our_private_key,
            our_cert,
            verification_key,
            receiver_thumbprint,
            is_client_role,
            apply_channel_thumbprint,
            first_request_signature,
            src,
            encrypted_range,
            #[cfg(feature = "ecc")]
            first_request_signature_to_store,
            crypto_offload,
        } = self;
        // crypto_offload is unused in the inline path — only the async
        // (blocking) path consults it. Drop to avoid clippy warnings.
        drop(crypto_offload);

        let decrypted_chunk = asymmetric_decrypt_and_verify_owned(
            security_policy,
            our_private_key,
            our_cert,
            verification_key,
            receiver_thumbprint,
            is_client_role,
            apply_channel_thumbprint,
            first_request_signature,
            src,
            encrypted_range,
        )?;

        Ok(CompletedOscDecrypt {
            security_policy,
            decrypted_chunk,
            #[cfg(feature = "ecc")]
            first_request_signature_to_store,
        })
    }

    async fn decrypt_blocking(self) -> Result<CompletedOscDecrypt, Error> {
        let PreparedOscDecryptCrypto {
            security_policy,
            our_private_key,
            our_cert,
            verification_key,
            receiver_thumbprint,
            is_client_role,
            apply_channel_thumbprint,
            first_request_signature,
            src,
            encrypted_range,
            #[cfg(feature = "ecc")]
            first_request_signature_to_store,
            crypto_offload,
        } = self;

        let decrypted_chunk = match crate::comms::crypto_offload::execute_offloaded(
            crypto_offload.as_deref(),
            move || {
                asymmetric_decrypt_and_verify_owned(
                    security_policy,
                    our_private_key,
                    our_cert,
                    verification_key,
                    receiver_thumbprint,
                    is_client_role,
                    apply_channel_thumbprint,
                    first_request_signature,
                    src,
                    encrypted_range,
                )
            },
        )
        .await
        {
            Ok(inner) => inner?,
            Err(_offload_err) => {
                return Err(Error::new(
                    StatusCode::BadInternalError,
                    "asymmetric decrypt task failed",
                ));
            }
        };

        Ok(CompletedOscDecrypt {
            security_policy,
            decrypted_chunk,
            #[cfg(feature = "ecc")]
            first_request_signature_to_store,
        })
    }
}

/// Owned inputs for an offloaded outbound OpenSecureChannel sign+encrypt.
///
/// All material is cloned from the channel during preparation (before any
/// `.await`), so the struct is `Send + 'static` and safe to move into
/// `spawn_blocking`. OPC-10000-6 §6.7.2.
struct PreparedOscSignCrypto {
    security_policy: SecurityPolicy,
    signing_key: PrivateKey,
    encryption_key: Option<PublicKey>,
    is_client_role: bool,
    apply_channel_thumbprint: bool,
    first_request_signature: Vec<u8>,
    src: Vec<u8>,
    encrypted_range: Range<usize>,
    /// Dedicated crypto executor (T010A). `None` falls back to spawn_blocking.
    crypto_offload: Option<Arc<dyn CryptoOffload>>,
}

struct CompletedOscSign {
    secured_chunk: Vec<u8>,
    first_request_signature_to_store: Option<Vec<u8>>,
}

impl PreparedOscSignCrypto {
    /// Run the asymmetric sign+encrypt core on the blocking pool or
    /// dedicated crypto executor (T010A).
    ///
    /// Offload failure (worker panic/cancellation, executor closed) maps to
    /// `BadInternalError`; the inner crypto `Err(status)` is returned
    /// verbatim (C2/R6).
    async fn sign_blocking(self) -> Result<CompletedOscSign, StatusCode> {
        let PreparedOscSignCrypto {
            security_policy,
            signing_key,
            encryption_key,
            is_client_role,
            apply_channel_thumbprint,
            first_request_signature,
            src,
            encrypted_range,
            crypto_offload,
        } = self;

        let (secured_chunk, first_request_signature_to_store) =
            match crate::comms::crypto_offload::execute_offloaded(
                crypto_offload.as_deref(),
                move || {
                    asymmetric_sign_and_encrypt_owned(
                        security_policy,
                        signing_key,
                        encryption_key,
                        is_client_role,
                        apply_channel_thumbprint,
                        first_request_signature,
                        src,
                        encrypted_range,
                    )
                },
            )
            .await
            {
                Ok(inner) => inner?,
                Err(_offload_err) => {
                    return Err(StatusCode::BadInternalError);
                }
            };

        Ok(CompletedOscSign {
            secured_chunk,
            first_request_signature_to_store,
        })
    }
}

impl SecureChannel {
    /// For testing purposes only
    #[cfg(test)]
    pub fn new_no_certificate_store() -> SecureChannel {
        SecureChannel {
            role: Role::Unknown,
            allow_deprecated: false,
            security_policy: SecurityPolicy::None,
            security_mode: MessageSecurityMode::None,
            secure_channel_id: 0,
            token_id: 0,
            token_created_at: DateTime::now(),
            token_lifetime: 0,
            local_nonce: Vec::new(),
            remote_nonce: Vec::new(),
            #[cfg(feature = "ecc")]
            local_ephemeral_key: None,
            #[cfg(feature = "ecc")]
            first_request_signature: Mutex::new(Vec::new()),
            #[cfg(feature = "ecc")]
            apply_channel_thumbprint: false,
            cert: None,
            private_key: None,
            remote_cert: None,
            local_keys: None,
            peer_max_response_body_size: None,
            encoding_context: Default::default(),
            remote_keys: HashMap::new(),
            security_policy_valid: false,
            crypto_offload: None,
        }
    }

    /// Create a new secure channel with the given certificate store
    /// and role.
    pub fn new(
        certificate_store: Arc<RwLock<opcua_crypto::CertificateStore>>,
        role: Role,
        encoding_context: Arc<RwLock<ContextOwned>>,
    ) -> SecureChannel {
        let (cert, private_key) = {
            let certificate_store = certificate_store.read();
            let cert = match certificate_store.read_own_cert() {
                Err(e) => {
                    error!("Failed to read own certificate: {e}. Check paths, crypto won't work");
                    None
                }
                Ok(r) => Some(r),
            };
            let pkey = match certificate_store.read_own_pkey() {
                Err(e) => {
                    error!("Failed to read own private key: {e}. Check paths, crypto won't work");
                    None
                }
                Ok(r) => Some(r),
            };
            (cert, pkey)
        };
        SecureChannel {
            role,
            allow_deprecated: false,
            security_mode: MessageSecurityMode::None,
            security_policy: SecurityPolicy::None,
            secure_channel_id: 0,
            token_id: 0,
            token_created_at: DateTime::now(),
            token_lifetime: 0,
            local_nonce: Vec::new(),
            remote_nonce: Vec::new(),
            #[cfg(feature = "ecc")]
            local_ephemeral_key: None,
            #[cfg(feature = "ecc")]
            first_request_signature: Mutex::new(Vec::new()),
            #[cfg(feature = "ecc")]
            apply_channel_thumbprint: false,
            cert,
            private_key,
            remote_cert: None,
            local_keys: None,
            peer_max_response_body_size: None,
            encoding_context,
            remote_keys: HashMap::new(),
            security_policy_valid: false,
            crypto_offload: None,
        }
    }

    pub(super) fn log_crypto_data(message: &str, data: &[u8]) {
        crate::debug::log_buffer(message, data);
    }

    /// Prepare owned inputs for an offloaded outbound OpenSecureChannel
    /// sign+encrypt. The thread-local padding scratch is used during
    /// preparation and the borrow ends before this method returns, so the
    /// resulting `PreparedOscSignCrypto` is `Send + 'static`.
    fn prepare_open_secure_channel_sign(
        &self,
        message_chunk: &MessageChunk,
        encrypted_data_offset: usize,
    ) -> Result<PreparedOscSignCrypto, StatusCode> {
        let signing_key = self
            .private_key
            .as_ref()
            .ok_or(StatusCode::BadSecurityChecksFailed)?
            .clone();

        let is_ecc = self.security_policy.is_ecc();
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

        let src = crypto::PADDING_AND_SIGNATURE_SCRATCH.with(|scratch| {
            let mut data = scratch.borrow_mut();
            let message_size =
                self.add_space_for_padding_and_signature_into(message_chunk, &mut data)?;
            data.truncate(message_size);
            Self::log_crypto_data("Chunk before padding", &message_chunk.data[..]);
            Self::log_crypto_data("Chunk after padding", &data[..]);
            Ok::<Vec<u8>, StatusCode>(data.clone())
        })?;

        let encrypted_range = encrypted_data_offset..src.len();

        Ok(PreparedOscSignCrypto {
            security_policy: self.security_policy,
            signing_key,
            encryption_key,
            is_client_role,
            apply_channel_thumbprint,
            first_request_signature,
            src,
            encrypted_range,
            crypto_offload: self.crypto_offload.clone(),
        })
    }

    /// Async variant of [`apply_security`](Self::apply_security) that offloads
    /// only the outbound OpenSecureChannel asymmetric sign+encrypt to Tokio's
    /// blocking pool (C5: symmetric per-request path stays inline).
    ///
    /// For security policy `None`, symmetric chunks, and Ack/Error payloads,
    /// this delegates to the existing synchronous `apply_security`.
    /// OPC-10000-6 §6.7.2.
    pub(crate) async fn apply_security_async(
        &self,
        message_chunk: &MessageChunk,
        dst: &mut [u8],
    ) -> Result<usize, StatusCode> {
        let should_offload = self.security_policy != SecurityPolicy::None
            && matches!(
                self.security_mode,
                MessageSecurityMode::Sign | MessageSecurityMode::SignAndEncrypt
            )
            && message_chunk.is_open_secure_channel(&self.decoding_options());

        if !should_offload {
            return self.apply_security(message_chunk, dst);
        }

        let encrypted_data_offset =
            message_chunk.encrypted_data_offset(&self.decoding_options())?;

        let prepared =
            self.prepare_open_secure_channel_sign(message_chunk, encrypted_data_offset)?;

        let CompletedOscSign {
            secured_chunk,
            first_request_signature_to_store,
        } = prepared.sign_blocking().await?;

        let secured_size = secured_chunk.len();
        framing::security_mut_slice(dst, 0..secured_size)?.copy_from_slice(&secured_chunk);

        Self::log_crypto_data(
            "Chunk after encryption",
            framing::security_slice_to(dst, secured_size)?,
        );

        // Store ECC client first_request_signature back, matching the sync
        // wrapper semantics. On the server path is_client_role is false so
        // this is a no-op; included for drop-in equivalence.
        #[cfg(feature = "ecc")]
        {
            let is_client_role = self.is_client_role();
            if is_client_role {
                if let Some(sig) = first_request_signature_to_store {
                    let mut first_request_signature = self.first_request_signature.lock();
                    first_request_signature.clear();
                    first_request_signature.extend_from_slice(&sig);
                }
            }
        }
        #[cfg(not(feature = "ecc"))]
        let _ = first_request_signature_to_store;

        Ok(secured_size)
    }

    fn prepare_open_secure_channel_decrypt(
        &self,
        src: bytes::Bytes,
        security_header: SecurityHeader,
        encrypted_range: Range<usize>,
    ) -> Result<PreparedOscDecrypt, Error> {
        // The OpenSecureChannel is the first thing we receive so we must examine
        // the security policy and use it to determine if the packet must be decrypted.

        trace!("Decrypting OpenSecureChannel");

        // Asymmetric decrypt and verify

        let security_header = match security_header {
            SecurityHeader::Asymmetric(security_header) => security_header,
            _ => {
                return Err(Error::new(
                    StatusCode::BadUnexpectedError,
                    format!("Expected asymmetric security header, got {security_header:?}"),
                ));
            }
        };

        // The security policy dictates the encryption / signature algorithms used by the request
        let security_policy_uri = security_header.security_policy_uri.as_ref();
        let security_policy = SecurityPolicy::from_uri(security_policy_uri);
        match security_policy {
            SecurityPolicy::Unknown => {
                return Err(Error::new(
                    StatusCode::BadSecurityPolicyRejected,
                    format!(
                        "Security policy \"{security_policy_uri}\" provided by client is unknown so it is has been rejected"
                    ),
                ));
            }
            SecurityPolicy::None => {
                // Nothing to do
                return Ok(PreparedOscDecrypt::Plain {
                    message_chunk: MessageChunk {
                        data: src,
                        cached_chunk_info: std::sync::OnceLock::new(),
                    },
                    security_policy,
                });
            }
            _ => {}
        }

        // Reject policies this build cannot serve (e.g. legacy policies in
        // a build without the legacy-crypto feature) before any crypto is
        // attempted.
        if !security_policy.is_supported() {
            return Err(Error::new(
                StatusCode::BadSecurityPolicyRejected,
                format!(
                    "Security policy \"{security_policy_uri}\" is not supported by this build and has been rejected"
                ),
            ));
        }

        // Reject deprecated policies unless the deployment opted in at
        // runtime (server allow_legacy_crypto).
        if security_policy.is_deprecated() && !self.allow_deprecated {
            return Err(Error::new(
                StatusCode::BadSecurityPolicyRejected,
                format!(
                    "Security policy \"{security_policy_uri}\" is deprecated and disabled. Set allow_legacy_crypto: true in the server configuration to enable it."
                ),
            ));
        }

        // Asymmetric decrypt and verify

        // The OpenSecureChannel Messages are always signed and encrypted if the SecurityMode
        // is not None. Even if the SecurityMode is Sign and not SignAndEncrypt.

        // An OpenSecureChannelRequest uses Asymmetric encryption - decrypt using the server's private
        // key, verify signature with client's public key.

        // This code doesn't *care* if the cert is trusted, merely that it was used to sign the message
        if security_header.sender_certificate.is_null() {
            return Err(Error::new(
                StatusCode::BadCertificateInvalid,
                "Sender certificate is null",
            ));
        }

        let sender_certificate_len = security_header
            .sender_certificate
            .value
            .as_ref()
            .ok_or_else(|| {
                Error::new(
                    StatusCode::BadCertificateInvalid,
                    "Sender certificate is null",
                )
            })?
            .len();
        trace!(
            "Sender certificate byte length = {}",
            sender_certificate_len
        );
        let sender_certificate = X509::from_byte_string(&security_header.sender_certificate)?;

        let verification_key = sender_certificate.public_key()?;
        let receiver_thumbprint = security_header.receiver_certificate_thumbprint;
        trace!("Receiver thumbprint = {:?}", receiver_thumbprint);

        let our_cert = self.cert.as_ref().ok_or_else(|| {
            Error::new(
                StatusCode::BadCertificateInvalid,
                "Missing local application certificate",
            )
        })?;
        let our_private_key = self.private_key.as_ref().ok_or_else(|| {
            Error::new(StatusCode::BadSecurityChecksFailed, "Missing private key")
        })?;
        let is_client_role = self.is_client_role();
        #[cfg(feature = "ecc")]
        let apply_channel_thumbprint = self.apply_channel_thumbprint;
        #[cfg(not(feature = "ecc"))]
        let apply_channel_thumbprint = false;
        #[cfg(feature = "ecc")]
        let first_request_signature = self.first_request_signature.lock().clone();
        #[cfg(not(feature = "ecc"))]
        let first_request_signature = Vec::new();
        #[cfg(feature = "ecc")]
        let first_request_signature_to_store = if security_policy.is_ecc() && !is_client_role {
            let signature_size = security_policy.asymmetric_signature_size(&verification_key);
            let signature_end = encrypted_range.end;
            Some(
                signature_end
                    .checked_sub(signature_size)
                    .ok_or_else(|| {
                        Error::new(
                            StatusCode::BadSecurityChecksFailed,
                            "decrypted chunk is smaller than the signature",
                        )
                    })
                    .and_then(|signature_start| {
                        src.get(signature_start..signature_end)
                            .ok_or_else(|| {
                                Error::new(
                                    StatusCode::BadSecurityChecksFailed,
                                    "invalid signature range",
                                )
                            })
                            .map(|signature| signature.to_vec())
                    }),
            )
        } else {
            None
        };

        Ok(PreparedOscDecrypt::Crypto(Box::new(
            PreparedOscDecryptCrypto {
                security_policy,
                our_private_key: our_private_key.clone(),
                our_cert: our_cert.clone(),
                verification_key,
                receiver_thumbprint,
                is_client_role,
                apply_channel_thumbprint,
                first_request_signature,
                src: src.to_vec(),
                encrypted_range,
                #[cfg(feature = "ecc")]
                first_request_signature_to_store,
                crypto_offload: self.crypto_offload.clone(),
            },
        )))
    }

    fn finish_open_secure_channel_decrypt(
        &self,
        completed: CompletedOscDecrypt,
    ) -> Result<(MessageChunk, SecurityPolicy), Error> {
        let CompletedOscDecrypt {
            security_policy,
            decrypted_chunk,
            #[cfg(feature = "ecc")]
            first_request_signature_to_store,
        } = completed;

        #[cfg(feature = "ecc")]
        if let Some(first_request_signature_to_store) = first_request_signature_to_store {
            let first_request_signature_to_store = first_request_signature_to_store?;
            let mut first_request_signature = self.first_request_signature.lock();
            first_request_signature.clear();
            first_request_signature.extend_from_slice(&first_request_signature_to_store);
        }

        let decrypted_size = decrypted_chunk.len();
        let msg = Self::update_message_size_and_truncate(decrypted_chunk, decrypted_size)?;
        Ok((
            MessageChunk {
                data: msg.into(),
                cached_chunk_info: std::sync::OnceLock::new(),
            },
            security_policy,
        ))
    }

    fn decrypt_open_secure_channel(
        &self,
        src: bytes::Bytes,
        security_header: SecurityHeader,
        encrypted_range: Range<usize>,
    ) -> Result<(MessageChunk, SecurityPolicy), Error> {
        match self.prepare_open_secure_channel_decrypt(src, security_header, encrypted_range)? {
            PreparedOscDecrypt::Plain {
                message_chunk,
                security_policy,
            } => Ok((message_chunk, security_policy)),
            PreparedOscDecrypt::Crypto(prepared) => {
                self.finish_open_secure_channel_decrypt((*prepared).decrypt_inline()?)
            }
        }
    }

    async fn decrypt_open_secure_channel_async(
        &self,
        src: bytes::Bytes,
        security_header: SecurityHeader,
        encrypted_range: Range<usize>,
    ) -> Result<(MessageChunk, SecurityPolicy), Error> {
        match self.prepare_open_secure_channel_decrypt(src, security_header, encrypted_range)? {
            PreparedOscDecrypt::Plain {
                message_chunk,
                security_policy,
            } => Ok((message_chunk, security_policy)),
            PreparedOscDecrypt::Crypto(prepared) => {
                self.finish_open_secure_channel_decrypt((*prepared).decrypt_blocking().await?)
            }
        }
    }
}
