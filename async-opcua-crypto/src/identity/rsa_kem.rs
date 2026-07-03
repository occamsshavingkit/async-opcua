//! RSA-KEM helpers for identity-token encrypted secrets.
//! OPC 10000-6 §6.7.3: RSA Key Encapsulation Mechanism.
//!
//! Format: [rsa_oaep_encrypted_aes_key][aes_keywrap_wrapped_token]
//! 1. RSA-OAEP decrypt the first RSA block → AES-256 key
//! 2. AES-256-KeyWrap unwrap the remainder → plaintext token

use opcua_types::{status_code::StatusCode, Error};

use crate::{
    aes::{aes_key_wrap_decrypt, AesKey},
    algorithms::{ENC_RSA_OAEP, ENC_RSA_OAEP_SHA256},
    policy::aes::{OaepSha1, OaepSha256},
    KeySize, PrivateKey,
};

/// Return `Ok(sk)` on successful RSA-KEM decryption:
/// 1. RSA-OAEP decrypt the first RSA block → AES-256 key
/// 2. AES-256-KeyWrap unwrap the remainder → plaintext token
///
/// # Errors
///
/// Returns `BadIdentityTokenRejected` if the payload is malformed, RSA decryption
/// fails, or AES unwrap fails.
pub fn decrypt_rsa_kem_secret(
    encryption_algorithm: &str,
    encrypted_secret: &[u8],
    server_key: &PrivateKey,
) -> Result<Vec<u8>, Error> {
    let block_size = server_key.cipher_text_block_size();
    if block_size == 0 {
        return Err(Error::new(
            StatusCode::BadIdentityTokenRejected,
            "RSA key has zero block size",
        ));
    }
    if encrypted_secret.len() <= block_size {
        return Err(Error::new(
            StatusCode::BadIdentityTokenRejected,
            "RSA-KEM payload too short: expected at least one RSA block + wrapped data",
        ));
    }

    let (rsa_encrypted_key, aes_wrapped_token) = encrypted_secret.split_at(block_size);

    // Step 1: RSA-OAEP decrypt the AES key
    let mut aes_key_bytes = vec![0u8; block_size];
    let aes_key_len = match encryption_algorithm {
        ENC_RSA_OAEP => {
            server_key.private_decrypt::<OaepSha1>(rsa_encrypted_key, &mut aes_key_bytes)
        }
        ENC_RSA_OAEP_SHA256 => {
            server_key.private_decrypt::<OaepSha256>(rsa_encrypted_key, &mut aes_key_bytes)
        }
        algorithm => {
            return Err(Error::new(
                StatusCode::BadIdentityTokenRejected,
                format!("unsupported RSA-KEM encryption algorithm {algorithm}"),
            ))
        }
    }
    .map_err(|_| {
        Error::new(
            StatusCode::BadIdentityTokenRejected,
            "failed to decrypt RSA-KEM symmetric key",
        )
    })?;
    aes_key_bytes.truncate(aes_key_len);

    let aes_key = AesKey::new(aes_key_bytes);

    // Step 2: AES-256-KeyWrap unwrap the token
    let mut plaintext = vec![0u8; aes_wrapped_token.len() + 8];
    let len = aes_key_wrap_decrypt(&aes_key, aes_wrapped_token, &mut plaintext).map_err(|_| {
        Error::new(
            StatusCode::BadIdentityTokenRejected,
            "failed to unwrap RSA-KEM identity token",
        )
    })?;
    plaintext.truncate(len);

    Ok(plaintext)
}
