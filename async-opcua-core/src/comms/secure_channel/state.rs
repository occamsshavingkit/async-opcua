// OPCUA for Rust
// SPDX-License-Identifier: MPL-2.0
// Copyright (C) 2017-2024 Adam Lock

use std::ops::Deref;
use std::sync::Arc;
use std::time::Instant;

use chrono::Duration;
use tracing::{error, trace};

#[cfg(feature = "ecc")]
use opcua_crypto::ecc::{self, EphemeralPrivateKey};
use opcua_crypto::{random, AesDerivedKeys, PrivateKey, SecurityPolicy, X509};
use opcua_types::{
    status_code::StatusCode, ByteString, ChannelSecurityToken, ContextOwned, DateTime,
    DecodingOptions, Error, MessageSecurityMode, NamespaceMap,
};
use parking_lot::RwLock;

use super::{Role, SecureChannel};

#[derive(Debug)]
pub(super) struct RemoteKeys {
    pub(super) keys: AesDerivedKeys,
    pub(super) expires_at: DateTime,
}

impl SecureChannel {
    /// Return `true` if this channel is for a client.
    pub fn is_client_role(&self) -> bool {
        self.role == Role::Client
    }

    /// Set the channel role.
    pub fn set_role(&mut self, role: Role) {
        self.role = role;
    }

    /// Set the local ephemeral private key used for ECC secure-channel ECDH.
    #[cfg(feature = "ecc")]
    pub fn set_local_ephemeral_key(&mut self, key: EphemeralPrivateKey) {
        self.local_ephemeral_key = Some(key);
    }

    /// Set whether ECC ChannelThumbprint signing/verification is applied.
    #[cfg(feature = "ecc")]
    pub fn set_apply_channel_thumbprint(&mut self, value: bool) {
        self.apply_channel_thumbprint = value;
    }

    /// Set the application certificate.
    pub fn set_cert(&mut self, cert: Option<X509>) {
        self.cert = cert;
    }

    /// Get the application certificate.
    pub fn cert(&self) -> Option<X509> {
        self.cert.clone()
    }

    /// Set the remote certificate.
    pub fn set_remote_cert(&mut self, remote_cert: Option<X509>) {
        self.remote_cert = remote_cert;
    }

    /// Get the remote certificate.
    pub fn remote_cert(&self) -> Option<X509> {
        self.remote_cert.clone()
    }

    /// Set the application private key.
    pub fn set_private_key(&mut self, private_key: Option<PrivateKey>) {
        self.private_key = private_key;
    }

    /// Get the application security mode.
    pub fn security_mode(&self) -> MessageSecurityMode {
        self.security_mode
    }

    /// Set the application security mode.
    pub fn set_security_mode(&mut self, security_mode: MessageSecurityMode) {
        self.security_mode = security_mode;
    }

    /// Set whether deprecated (legacy) security policies may be negotiated
    /// on this channel.
    pub fn set_allow_deprecated(&mut self, allow: bool) {
        self.allow_deprecated = allow;
    }

    /// Set the peer/client `maxResponseMessageSize` response body limit; zero clears it.
    pub fn set_client_response_body_limit(&mut self, max_response_message_size: u32) {
        self.peer_max_response_body_size = match max_response_message_size {
            0 => None,
            max_response_message_size => Some(max_response_message_size as usize),
        };
    }

    /// Get the peer/client response body limit; `None` means no peer/client response body limit.
    pub fn client_response_body_limit(&self) -> Option<usize> {
        self.peer_max_response_body_size
    }

    /// Get the application security policy.
    pub fn security_policy(&self) -> SecurityPolicy {
        self.security_policy
    }

    /// Return the known security policy if already validated, `None` otherwise.
    pub fn known_policy(&self) -> Option<SecurityPolicy> {
        if self.security_policy_valid {
            Some(self.security_policy)
        } else {
            None
        }
    }

    /// Set the application security policy.
    pub fn set_security_policy(&mut self, security_policy: SecurityPolicy) {
        self.security_policy_valid = matches!(
            security_policy,
            SecurityPolicy::Basic128Rsa15
                | SecurityPolicy::Basic256
                | SecurityPolicy::Basic256Sha256
                | SecurityPolicy::Aes128Sha256RsaOaep
                | SecurityPolicy::Aes256Sha256RsaPss
                | SecurityPolicy::EccNistP256
                | SecurityPolicy::EccNistP384
        );
        self.security_policy = security_policy;
    }

    /// Clear the configured security token.
    pub fn clear_security_token(&mut self) {
        self.secure_channel_id = 0;
        self.token_id = 0;
        self.token_created_at = DateTime::now();
        self.token_lifetime = 0;
    }

    /// Attach a dedicated crypto offload executor (T010A). When set,
    /// asymmetric OSC crypto (decrypt/verify, sign/encrypt) runs on this
    /// executor's workers instead of the shared `spawn_blocking` pool,
    /// giving the deployment a scheduling-priority seam for handshake
    /// crypto.
    pub fn set_crypto_offload(
        &mut self,
        executor: Arc<dyn crate::comms::crypto_offload::CryptoOffload>,
    ) {
        self.crypto_offload = Some(executor);
    }

    /// Set the channel security token.
    pub fn set_security_token(&mut self, channel_token: ChannelSecurityToken) {
        self.secure_channel_id = channel_token.channel_id;
        self.token_id = channel_token.token_id;
        self.token_created_at = channel_token.created_at;
        self.token_lifetime = channel_token.revised_lifetime;
    }

    /// Set the ID of the secure channel, this is chosen by the server.
    pub fn set_secure_channel_id(&mut self, secure_channel_id: u32) {
        self.secure_channel_id = secure_channel_id;
    }

    /// Get the ID of the secure channel on the server.
    pub fn secure_channel_id(&self) -> u32 {
        self.secure_channel_id
    }

    /// Get the time the currently active token was created.
    pub fn token_created_at(&self) -> DateTime {
        self.token_created_at
    }

    /// Get the lifetime of the active token.
    pub fn token_lifetime(&self) -> u32 {
        self.token_lifetime
    }

    /// Set the ID of the active token.
    pub fn set_token_id(&mut self, token_id: u32) {
        self.token_id = token_id;
    }

    /// Get the ID of the active token.
    pub fn token_id(&self) -> u32 {
        self.token_id
    }

    /// Set the offset in time between the clock of the server and client.
    pub fn set_client_offset(&mut self, client_offset: chrono::Duration) {
        self.encoding_context.write().options_mut().client_offset = client_offset;
    }

    /// Set the decoding options, will not change the client offset.
    pub fn set_decoding_options(&mut self, decoding_options: DecodingOptions) {
        let mut context = self.encoding_context.write();
        let offset = context.options().client_offset;
        (*context.options_mut()) = DecodingOptions {
            client_offset: offset,
            ..decoding_options
        };
    }

    /// Get a reference to the encoding context.
    pub fn context(&self) -> impl Deref<Target = ContextOwned> + '_ {
        self.encoding_context.read()
    }

    /// Get a reference counted reference to the encoding context.
    pub fn context_arc(&self) -> Arc<RwLock<ContextOwned>> {
        self.encoding_context.clone()
    }

    /// Set the namespace map.
    pub fn set_namespaces(&self, namespaces: NamespaceMap) {
        *self.encoding_context.write().namespaces_mut() = namespaces;
    }

    /// Get the decoding options.
    pub fn decoding_options(&self) -> DecodingOptions {
        self.context().options().clone()
    }

    /// Test if the secure channel token needs to be renewed. The algorithm determines it needs
    /// to be renewed if the issue period has elapsed by 75% or more.
    pub fn should_renew_security_token(&self) -> bool {
        if self.token_id() == 0 {
            false
        } else {
            // Check if secure channel 75% close to expiration in which case send a renew
            let renew_lifetime = (self.token_lifetime * 3) / 4;
            let renew_lifetime = Duration::milliseconds(renew_lifetime as i64);
            // Renew the token?
            DateTime::now() - self.token_created_at > renew_lifetime
        }
    }

    /// Creates a nonce for the connection. The nonce should be the same size as the symmetric key
    pub fn create_random_nonce(&mut self) {
        self.local_nonce
            .resize(self.security_policy.secure_channel_nonce_length(), 0);
        random::bytes(&mut self.local_nonce);
    }

    /// Creates the local nonce for an OpenSecureChannel handshake.
    ///
    /// For ECC policies the nonce is the ephemeral EC public key and the matching private key is
    /// retained for ECDH key derivation. Other policies keep the existing random nonce behavior.
    ///
    /// # Errors
    ///
    /// Returns an error if ECC ephemeral key generation fails.
    pub fn create_local_nonce(&mut self) -> Result<(), Error> {
        #[cfg(feature = "ecc")]
        if matches!(
            self.security_policy,
            SecurityPolicy::EccNistP256 | SecurityPolicy::EccNistP384
        ) {
            let curve = ecc::EccCurve::from_security_policy(self.security_policy)?;
            let keypair = ecc::generate_ephemeral_keypair(curve)?;
            let (private_key, public_key) = keypair.into_parts();
            self.local_nonce = public_key.encoded().to_vec();
            self.set_local_ephemeral_key(private_key);
            return Ok(());
        }

        self.create_random_nonce();
        Ok(())
    }

    /// Sets the remote certificate
    pub fn set_remote_cert_from_byte_string(
        &mut self,
        remote_cert: &ByteString,
    ) -> Result<(), Error> {
        self.remote_cert = if remote_cert.is_null_or_empty() {
            None
        } else {
            Some(X509::from_byte_string(remote_cert)?)
        };
        Ok(())
    }

    /// Obtains the remote certificate as a byte string
    pub fn remote_cert_as_byte_string(&self) -> ByteString {
        if let Some(ref remote_cert) = self.remote_cert {
            remote_cert.as_byte_string()
        } else {
            ByteString::null()
        }
    }

    /// For secure channel requests, validate that the nonce has the correct length.
    pub fn validate_secure_channel_nonce_length(&self, nonce: &ByteString) -> Result<(), Error> {
        if self.security_policy != SecurityPolicy::None
            && nonce.len() != self.security_policy.secure_channel_nonce_length()
        {
            error!(
                "Nonce is invalid length {}, expecting {}. {:?}",
                nonce.len(),
                self.security_policy.secure_channel_nonce_length(),
                nonce
            );
            Err(Error::new(
                StatusCode::BadNonceInvalid,
                format!(
                    "Nonce is invalid length {}, expecting {}. {:?}",
                    nonce.len(),
                    self.security_policy.secure_channel_nonce_length(),
                    nonce
                ),
            ))
        } else {
            Ok(())
        }
    }

    /// Set their nonce which should be the same as the symmetric key
    pub fn set_remote_nonce_from_byte_string(
        &mut self,
        remote_nonce: &ByteString,
    ) -> Result<(), Error> {
        if let Some(ref remote_nonce) = remote_nonce.value {
            self.remote_nonce = remote_nonce.to_vec();
            Ok(())
        } else if self.security_policy != SecurityPolicy::None {
            error!("Remote nonce is invalid {:?}", remote_nonce);
            Err(Error::new(
                StatusCode::BadNonceInvalid,
                "Remote nonce is invalid",
            ))
        } else {
            Ok(())
        }
    }

    /// Part 6
    /// 6.7.5
    /// Deriving keys Once the SecureChannel is established the Messages are signed and encrypted with
    /// keys derived from the Nonces exchanged in the OpenSecureChannel call. These keys are derived by passing the Nonces to a pseudo-random function which produces a sequence of bytes from a set of inputs. A pseudo-random function is represented by the following function declaration:
    ///
    /// ```c++
    /// Byte[] PRF( Byte[] secret,  Byte[] seed,  Int32 length,  Int32 offset)
    /// ```
    ///
    /// Where length is the number of bytes to return and offset is a number of bytes from the beginning of the sequence.
    ///
    /// The lengths of the keys that need to be generated depend on the SecurityPolicy used for the channel.
    /// The following information is specified by the SecurityPolicy:
    ///
    /// a) SigningKeyLength (from the DerivedSignatureKeyLength);
    /// b) EncryptingKeyLength (implied by the SymmetricEncryptionAlgorithm);
    /// c) EncryptingBlockSize (implied by the SymmetricEncryptionAlgorithm).
    ///
    /// The parameters passed to the pseudo random function are specified in Table 33.
    ///
    /// Table 33 – Cryptography key generation parameters
    ///
    /// Key | Secret | Seed | Length | Offset
    /// ClientSigningKey | ServerNonce | ClientNonce | SigningKeyLength | 0
    /// ClientEncryptingKey | ServerNonce | ClientNonce | EncryptingKeyLength | SigningKeyLength
    /// ClientInitializationVector | ServerNonce | ClientNonce | EncryptingBlockSize | SigningKeyLength + EncryptingKeyLength
    /// ServerSigningKey | ClientNonce | ServerNonce | SigningKeyLength | 0
    /// ServerEncryptingKey | ClientNonce | ServerNonce | EncryptingKeyLength | SigningKeyLength
    /// ServerInitializationVector | ClientNonce | ServerNonce | EncryptingBlockSize | SigningKeyLength + EncryptingKeyLength
    ///
    /// The Client keys are used to secure Messages sent by the Client. The Server keys
    /// are used to secure Messages sent by the Server.
    ///
    pub fn derive_keys(&mut self) {
        #[cfg(feature = "ecc")]
        if matches!(
            self.security_policy,
            SecurityPolicy::EccNistP256 | SecurityPolicy::EccNistP384
        ) {
            self.derive_ecc_keys();
            return;
        }

        self.insert_remote_keys(
            self.security_policy
                .make_secure_channel_keys(&self.local_nonce, &self.remote_nonce),
        );
        self.local_keys = Some(
            self.security_policy
                .make_secure_channel_keys(&self.remote_nonce, &self.local_nonce),
        );
        trace!("Remote nonce = {:?}", self.remote_nonce);
        trace!("Local nonce = {:?}", self.local_nonce);
        trace!(
            "Derived remote keys = {:?}",
            self.get_remote_keys(self.token_id)
        );
        trace!("Derived local keys = {:?}", self.local_keys);
    }

    #[cfg(feature = "ecc")]
    fn derive_ecc_keys(&mut self) {
        let Some(local_ephemeral_key) = self.local_ephemeral_key.as_ref() else {
            error!("Cannot derive ECC secure-channel keys without a local ephemeral key");
            return;
        };

        let keys = (|| {
            let curve = ecc::EccCurve::from_security_policy(self.security_policy)?;
            let remote_public_key = ecc::decode_public_key(curve, &self.remote_nonce)?;
            let shared_secret = ecc::ecdh_shared_secret(local_ephemeral_key, &remote_public_key)?;
            let (client_nonce, server_nonce) = if self.is_client_role() {
                (self.local_nonce.as_slice(), self.remote_nonce.as_slice())
            } else {
                (self.remote_nonce.as_slice(), self.local_nonce.as_slice())
            };

            ecc::derive_keys(
                self.security_policy,
                shared_secret.as_ref(),
                client_nonce,
                server_nonce,
            )
        })();

        match keys {
            Ok(keys) => {
                if self.is_client_role() {
                    self.local_keys = Some(keys.client);
                    self.insert_remote_keys(keys.server);
                } else {
                    self.local_keys = Some(keys.server);
                    self.insert_remote_keys(keys.client);
                }
            }
            Err(err) => {
                self.local_keys = None;
                error!("Failed to derive ECC secure-channel keys: {err}");
            }
        }
    }

    /// Get the deadline as an [`Instant`] for token renewal, used
    /// for timeouts on the server.
    pub fn token_renewal_deadline(&self) -> Instant {
        let deadline =
            self.token_created_at + Duration::seconds((self.token_lifetime as i64) * 4 / 3);
        // Convert to instant by getting the time until expiration then adding that to now()
        let until_expiration = (deadline - DateTime::now()).num_milliseconds();
        if until_expiration < 0 {
            Instant::now()
        } else {
            Instant::now() + std::time::Duration::from_millis(until_expiration as u64)
        }
    }

    /// Get the local nonce.
    pub fn local_nonce(&self) -> &[u8] {
        &self.local_nonce
    }

    /// Set the local nonce.
    pub fn set_local_nonce(&mut self, local_nonce: &[u8]) {
        self.local_nonce.clear();
        self.local_nonce.extend_from_slice(local_nonce);
    }

    /// Get the local nonce as a byte string.
    pub fn local_nonce_as_byte_string(&self) -> ByteString {
        if self.local_nonce.is_empty() {
            ByteString::null()
        } else {
            ByteString::from(&self.local_nonce)
        }
    }

    /// Set the remote nonce.
    pub fn set_remote_nonce(&mut self, remote_nonce: &[u8]) {
        self.remote_nonce.clear();
        self.remote_nonce.extend_from_slice(remote_nonce);
    }

    /// Get the remote nonce.
    pub fn remote_nonce(&self) -> &[u8] {
        &self.remote_nonce
    }

    /// Get the remote nonce as a byte string.
    pub fn remote_nonce_as_byte_string(&self) -> ByteString {
        if self.remote_nonce.is_empty() {
            ByteString::null()
        } else {
            ByteString::from(&self.remote_nonce)
        }
    }

    pub(super) fn local_keys(&self) -> Result<&AesDerivedKeys, StatusCode> {
        self.local_keys
            .as_ref()
            .ok_or(StatusCode::BadSecurityChecksFailed)
    }

    pub(super) fn insert_remote_keys(&mut self, keys: AesDerivedKeys) {
        // First remove any expired keys.
        self.remote_keys
            .retain(|_, v| DateTime::now() < v.expires_at);

        let expires_at = (self.token_lifetime as f32 * 1.25).ceil();
        let expires_at = Duration::milliseconds(expires_at as i64);

        // Then insert the new keys to ensure there is
        // always at least one set of keys available.
        self.remote_keys.insert(
            self.token_id,
            RemoteKeys {
                keys,
                expires_at: self.token_created_at + expires_at,
            },
        );
    }

    pub(super) fn get_remote_keys(&self, token_id: u32) -> Option<&AesDerivedKeys> {
        self.remote_keys.get(&token_id).map(|k| &k.keys)
    }

    pub(super) fn expect_supported_security_policy(&self) -> Result<(), StatusCode> {
        if self.security_policy_valid {
            Ok(())
        } else {
            Err(StatusCode::BadSecurityPolicyRejected)
        }
    }

    /// Set the token lifetime.
    pub fn set_token_lifetime(&mut self, token_lifetime: u32) {
        self.token_lifetime = token_lifetime;
    }

    /// Set the token creation time.
    pub fn set_token_created_at(&mut self, created_at: DateTime) {
        self.token_created_at = created_at;
    }
}

#[cfg(test)]
mod token_grace_tests {
    //! B4 (multi-AI cross-check, `specs/multi-ai-test-suites/UNIFIED-PROTOCOL.md`): on SecureChannel
    //! renewal the previous token's keys must remain usable during the overlap window and be pruned
    //! once it has elapsed (Part 4 §5.6.2 — accept the previous token for up to 125% of its lifetime).
    use super::SecureChannel;
    use chrono::Duration;
    use opcua_crypto::SecurityPolicy;
    use opcua_types::{ChannelSecurityToken, DateTime};

    fn token(id: u32, created_at: DateTime, revised_lifetime: u32) -> ChannelSecurityToken {
        ChannelSecurityToken {
            channel_id: 1,
            token_id: id,
            created_at,
            revised_lifetime,
        }
    }

    /// A SecureChannel on a real (key-deriving) policy with valid-length nonces, so `derive_keys`
    /// actually populates the per-token remote-keys map.
    fn channel() -> SecureChannel {
        let mut sc = SecureChannel::new_no_certificate_store();
        sc.set_security_policy(SecurityPolicy::Basic256Sha256);
        sc.local_nonce = vec![1u8; 32];
        sc.remote_nonce = vec![2u8; 32];
        sc
    }

    #[test]
    fn previous_token_keys_remain_usable_during_renewal_overlap() {
        let mut sc = channel();

        // Token #1, fresh with a long lifetime.
        sc.set_security_token(token(1, DateTime::now(), 60_000));
        sc.derive_keys();
        assert!(sc.get_remote_keys(1).is_some());

        // Renew to #2 while #1 is still well inside its 125% grace window: BOTH must stay available.
        sc.set_security_token(token(2, DateTime::now(), 60_000));
        sc.derive_keys();
        assert!(
            sc.get_remote_keys(1).is_some(),
            "previous token's keys must survive during the renewal overlap"
        );
        assert!(sc.get_remote_keys(2).is_some());
    }

    #[test]
    fn expired_token_keys_are_pruned_on_next_renewal() {
        let mut sc = channel();

        // Token #1 created far enough in the past that its 125% window (here ~75s) has fully elapsed.
        sc.set_security_token(token(1, DateTime::now() + Duration::seconds(-100), 60_000));
        sc.derive_keys();
        assert!(
            sc.get_remote_keys(1).is_some(),
            "current token is always inserted"
        );

        // Renewing inserts #2 and prunes the now-expired #1 — a stale token must no longer verify.
        sc.set_security_token(token(2, DateTime::now(), 60_000));
        sc.derive_keys();
        assert!(
            sc.get_remote_keys(1).is_none(),
            "an expired previous token must be pruned after the grace window"
        );
        assert!(sc.get_remote_keys(2).is_some());
    }
}
