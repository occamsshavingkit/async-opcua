//! AES Key Wrap (RFC 3394) for use in RSA-KEM identity token decryption.
//!
//! Uses 128-bit AES blocks as two 64-bit halves: A (integrity) and R[i] (data).

use crate::aes::AesKey;
use aes::cipher::{generic_array::GenericArray, BlockDecrypt, KeyInit};

const IV: [u8; 8] = [0xa6, 0xa6, 0xa6, 0xa6, 0xa6, 0xa6, 0xa6, 0xa6];

/// RFC 3394 §2.2.2 — AES Key Unwrap.
pub(crate) fn aes_key_wrap_decrypt(
    key: &AesKey,
    ciphertext: &[u8],
    output: &mut [u8],
) -> Result<usize, ()> {
    let n = (ciphertext.len() / 8) - 1;
    if n < 1 || !ciphertext.len().is_multiple_of(8) {
        return Err(());
    }

    match key.value().len() {
        16 => aes_unwrap_128(key, ciphertext, output, n),
        32 => aes_unwrap_256(key, ciphertext, output, n),
        _ => Err(()),
    }
}

fn xor_u64_into_a(a: &mut [u8; 8], t: u64) {
    for k in 0..8 {
        a[7 - k] ^= (t >> (k * 8)) as u8;
    }
}

fn aes_unwrap_128(
    key: &AesKey,
    ciphertext: &[u8],
    output: &mut [u8],
    n: usize,
) -> Result<usize, ()> {
    let aes = aes::Aes128::new(GenericArray::from_slice(key.value()));
    let mut a: [u8; 8] = ciphertext[..8].try_into().unwrap();
    let mut r: Vec<[u8; 8]> = (0..n)
        .map(|i| ciphertext[8 + i * 8..8 + (i + 1) * 8].try_into().unwrap())
        .collect();

    for j in (0..=5).rev() {
        for i in (1..=n).rev() {
            let t = (n as u64) * (j as u64) + (i as u64);
            xor_u64_into_a(&mut a, t);
            let mut combined: [u8; 16] = [0u8; 16];
            combined[..8].copy_from_slice(&a);
            combined[8..].copy_from_slice(&r[i - 1]);
            BlockDecrypt::decrypt_block(&aes, GenericArray::from_mut_slice(&mut combined));
            a.copy_from_slice(&combined[..8]);
            r[i - 1].copy_from_slice(&combined[8..]);
        }
    }

    if a != IV {
        return Err(());
    }
    for i in 0..n {
        output[i * 8..(i + 1) * 8].copy_from_slice(&r[i]);
    }
    Ok(n * 8)
}

fn aes_unwrap_256(
    key: &AesKey,
    ciphertext: &[u8],
    output: &mut [u8],
    n: usize,
) -> Result<usize, ()> {
    let aes = aes::Aes256::new(GenericArray::from_slice(key.value()));
    let mut a: [u8; 8] = ciphertext[..8].try_into().unwrap();
    let mut r: Vec<[u8; 8]> = (0..n)
        .map(|i| ciphertext[8 + i * 8..8 + (i + 1) * 8].try_into().unwrap())
        .collect();

    for j in (0..=5).rev() {
        for i in (1..=n).rev() {
            let t = (n as u64) * (j as u64) + (i as u64);
            xor_u64_into_a(&mut a, t);
            let mut combined: [u8; 16] = [0u8; 16];
            combined[..8].copy_from_slice(&a);
            combined[8..].copy_from_slice(&r[i - 1]);
            BlockDecrypt::decrypt_block(&aes, GenericArray::from_mut_slice(&mut combined));
            a.copy_from_slice(&combined[..8]);
            r[i - 1].copy_from_slice(&combined[8..]);
        }
    }

    if a != IV {
        return Err(());
    }
    for i in 0..n {
        output[i * 8..(i + 1) * 8].copy_from_slice(&r[i]);
    }
    Ok(n * 8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aes_key_wrap_128_known_vector() {
        let kek = crate::AesKey::new(vec![
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D,
            0x0E, 0x0F,
        ]);
        let wrapped = vec![
            0x1F, 0xA6, 0x8B, 0x0A, 0x81, 0x12, 0xB4, 0x47, 0xAE, 0xF3, 0x4B, 0xD8, 0xFB, 0x5A,
            0x7B, 0x82, 0x9D, 0x3E, 0x86, 0x23, 0x71, 0xD2, 0xCF, 0xE5,
        ];
        let expected: Vec<u8> = vec![
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD,
            0xEE, 0xFF,
        ];
        let mut out = vec![0u8; 16];
        let len = aes_key_wrap_decrypt(&kek, &wrapped, &mut out).unwrap();
        assert_eq!(&out[..len], &expected[..]);
    }
}
