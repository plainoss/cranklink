//! Motor authentication for supported legacy and modern firmware.
//! Legacy keys use the six DCAS serial bytes in wire order; modern keys also
//! use the final six ASCII product-serial characters. Gateway passkey
//! authentication is separate. Never log these keys or proofs.
//!
//! Legacy exchanges are 16/D8 -> DA challenge, E0 proof -> E2 receipt;
//! modern exchanges are 33/68 -> 6A encrypted challenge, 70 proof -> 72 receipt.
//! The session splits each 16-byte block into ordered selector/data records:
//! 16 carries 6 bytes, 26 carries 6, and 34 carries 4 plus two FF filler bytes.
//! Those selectors are hexadecimal; they are not byte counts or GATT fragment
//! counters. Sessions reject reordered fragments and make one attempt only.
use aes::cipher::{generic_array::GenericArray, BlockDecrypt, BlockEncrypt, KeyInit};
use anyhow::{ensure, Result};
use zeroize::{Zeroize, Zeroizing};

/// Accept only the expected addressed success opcode and four FF filler bytes.
pub(crate) fn validate_receipt(reply: &[u8], pc: u8, group: u8, opcode: u8) -> Result<()> {
    ensure!(
        reply.len() == 7 && reply[..3] == [pc, group, opcode] && reply[3..] == [0xff; 4],
        "drive-unit authentication reply rejected, misrouted or malformed; no retry"
    );
    Ok(())
}

/// Short GATT notifications may include three stale trailing buffer bytes.
/// Validate the complete seven-byte stock receipt; ignore only the known
/// fixed-size transport suffix, never a status or opcode byte.
pub(super) fn validate_ble_receipt(reply: &[u8], pc: u8, group: u8, opcode: u8) -> Result<()> {
    ensure!(
        matches!(reply.len(), 7 | 10),
        "invalid BLE authentication receipt length"
    );
    validate_receipt(&reply[..7], pc, group, opcode)
}

pub(crate) struct LegacyKeys {
    pub(crate) auth_key: [u8; 7],
    pub(super) update_key: [u8; 16],
}
impl Drop for LegacyKeys {
    fn drop(&mut self) {
        self.auth_key.zeroize();
        self.update_key.zeroize();
    }
}

pub(crate) fn derive(serial: &[u8; 6]) -> LegacyKeys {
    let a = u16::from_le_bytes([serial[0], serial[1]]) as u32;
    let b = u16::from_le_bytes([serial[2], serial[3]]) as u32;
    let c = u16::from_le_bytes([serial[4], serial[5]]) as u32;
    let mut state = [
        123456789 ^ ((a << 16) | b),
        362436069,
        521288629,
        88675123 ^ ((b << 16) | c),
    ];
    fn step(s: &mut [u32; 4]) -> u32 {
        let t = s[0] ^ (s[0] << 11);
        *s = [s[1], s[2], s[3], s[3] ^ (s[3] >> 19) ^ t ^ (t >> 8)];
        s[3]
    }
    for _ in 0..(a >> 8) {
        step(&mut state);
    }
    let mut auth_key = [0; 7];
    let first = step(&mut state).to_le_bytes();
    let second = step(&mut state).to_le_bytes();
    auth_key[..4].copy_from_slice(&first);
    auth_key[4..].copy_from_slice(&second[..3]);
    // The unused eighth authentication byte is discarded; the next output
    // array starts with a fresh PRNG word.
    let mut update_key = [0; 16];
    for chunk in update_key.chunks_exact_mut(4) {
        chunk.copy_from_slice(&step(&mut state).to_le_bytes());
    }
    state.zeroize();
    LegacyKeys {
        auth_key,
        update_key,
    }
}

pub(crate) fn encrypt_challenge(keys: &LegacyKeys, challenge: &[u8; 16]) -> [u8; 16] {
    let aes = aes::Aes128::new(GenericArray::from_slice(&keys.update_key));
    let mut proof = *challenge;
    aes.encrypt_block(GenericArray::from_mut_slice(&mut proof));
    proof
}

/// Keys for supported motors using the modern authentication scheme.
/// These keys are distinct from the legacy, DCAS-serial-only keys above.
/// No Debug implementation: key material must never enter diagnostic logs.
pub(crate) struct ModernKeys {
    pub(super) auth_key: [u8; 16],
    pub(super) common_key: [u8; 16],
    pub(super) encrypt_key: [u8; 16],
    pub(super) update_key: [u8; 16],
}

impl Drop for ModernKeys {
    fn drop(&mut self) {
        self.auth_key.zeroize();
        self.common_key.zeroize();
        self.encrypt_key.zeroize();
        self.update_key.zeroize();
    }
}

impl ModernKeys {
    /// The 33/68 authentication block: AES-ECB(encrypt_key, auth7 || FF*9).
    pub(crate) fn auth_ciphertext(&self) -> [u8; 16] {
        let cipher = aes::Aes128::new(GenericArray::from_slice(&self.encrypt_key));
        let mut block = self.auth_key;
        cipher.encrypt_block(GenericArray::from_mut_slice(&mut block));
        block
    }

    /// The 33/70 proof: decrypt the received challenge with encrypt_key, then
    /// encrypt that nonce with update_key. Both operations use one unpadded block.
    pub(crate) fn challenge_proof(&self, encrypted_challenge: &[u8; 16]) -> [u8; 16] {
        let decryptor = aes::Aes128::new(GenericArray::from_slice(&self.encrypt_key));
        let encryptor = aes::Aes128::new(GenericArray::from_slice(&self.update_key));
        let mut nonce = Zeroizing::new(*encrypted_challenge);
        decryptor.decrypt_block(GenericArray::from_mut_slice(nonce.as_mut()));
        encryptor.encrypt_block(GenericArray::from_mut_slice(nonce.as_mut()));
        *nonce
    }
}

/// Derive modern keys from raw DCAS serial bytes and the product serial text.
/// Use the final six product-serial characters as bytes, without hex
/// parsing or case conversion. Reject ambiguous, truncated or padded text before
/// any caller can send an authentication attempt to the motor.
pub(crate) fn derive_modern(serial: &[u8; 6], product_serial: &str) -> Result<ModernKeys> {
    ensure!(
        product_serial.len() >= 6 && product_serial.bytes().all(|b| b.is_ascii_graphic()),
        "product serial requires at least six printable ASCII characters without spaces"
    );
    let tail = &product_serial.as_bytes()[product_serial.len() - 6..];
    let mut pairs = Zeroizing::new([0u32; 6]);
    for (i, (&dcas, &product)) in serial.iter().zip(tail).enumerate() {
        pairs[i] = u32::from(dcas) | (u32::from(product) << 8);
    }
    let mut state = Zeroizing::new([
        123456789 ^ ((pairs[0] << 16) | pairs[1]),
        362436069 ^ ((pairs[2] << 16) | pairs[3]),
        521288629 ^ ((pairs[4] << 16) | pairs[5]),
        88675123 ^ ((pairs[5] << 16) | pairs[0]),
    ]);
    fn step(state: &mut [u32; 4]) -> u32 {
        let mut t = state[0] ^ (state[0] << 11);
        *state = [
            state[1],
            state[2],
            state[3],
            state[3] ^ (state[3] >> 19) ^ t ^ (t >> 8),
        ];
        t.zeroize();
        state[3]
    }
    fn output(state: &mut [u32; 4], bytes: &mut [u8]) {
        // Each output starts at a fresh PRNG word, discarding an unused tail.
        for chunk in bytes.chunks_mut(4) {
            let word = Zeroizing::new(step(state).to_le_bytes());
            chunk.copy_from_slice(&word[..chunk.len()]);
        }
    }
    let mut skips = Zeroizing::new([0u8; 3]);
    for i in 0..(pairs[5] >> 8) {
        let word = Zeroizing::new(step(&mut state).to_le_bytes());
        if i == 0 {
            skips.copy_from_slice(&word[..3]);
        }
    }
    let mut keys = ModernKeys {
        auth_key: [0xff; 16],
        common_key: [0; 16],
        encrypt_key: [0; 16],
        update_key: [0; 16],
    };
    output(&mut state, &mut keys.auth_key[..7]);
    for (skip, bytes) in skips.iter().zip([
        &mut keys.common_key,
        &mut keys.encrypt_key,
        &mut keys.update_key,
    ]) {
        for _ in 0..*skip {
            step(&mut state);
        }
        output(&mut state, bytes);
    }
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_accepts_success_and_rejects_errors() {
        let captured = hex::decode("0016e2ffffffff").unwrap();
        validate_receipt(&captured, 0, 0x16, 0xe2).unwrap();
        validate_receipt(&[0, 0x33, 0x72, 255, 255, 255, 255], 0, 0x33, 0x72).unwrap();
        for invalid in [
            vec![0, 0x16, 0xe3, 0x45, 0, 255, 0],
            vec![0, 0x16, 0xe3, 0x46, 0, 255, 0],
            vec![0, 0x16, 0xe2, 0],
            vec![0, 0x16, 0xe2, 0, 0, 0, 0],
            vec![0, 0x16, 0xe2, 0x45, 0, 255, 0],
            vec![0, 0x16, 0xe2, 255, 255, 255],
            vec![1, 0x16, 0xe2, 255, 255, 255, 255],
        ] {
            assert!(validate_receipt(&invalid, 0, 0x16, 0xe2).is_err());
        }
    }

    #[test]
    fn modern_keys_and_proofs_match_fixed_vectors() {
        // Synthetic identity inputs with fixed expected keys and proofs.
        let vectors = [
            (
                "000000000000",
                "000000",
                "675f6fbb226602ffffffffffffffffff",
                "a5ffff6b1ccfa06f9e5eec00ff1b2d0b",
                "994096f7d6c061852d4d638e28edd55e",
                "47d8d4f9d4d4ce6edd9cc1e6ff43f02d",
                "147e8a5ce6d94482e8e1887a27678fbe",
                "ec614c572e8e7bffa4578465264e6d7a",
                "87512eca2dded3cfc14cda79726db41e",
            ),
            (
                "010203040506",
                "ABCDEF",
                "172ae363e4a44bffffffffffffffffff",
                "2c6e8d9f2d334ebb9dffbb6497a90dcc",
                "64def4bb7e9c60d8da44768e405c3414",
                "ce11bad46590040ae495641bc3c52a99",
                "718dd37093b36dc5c414917637a03804",
                "63dfcbda01f8b42165be9d869752794f",
                "5501d4b4f69bc199ac0e846be1f85fee",
            ),
            (
                "ffffffffffff",
                "999999",
                "77c811564c2bddffffffffffffffffff",
                "a3de2296e525375c6c15ab8e6816a572",
                "39bbe1a27eaf2ef5abb555a856f8126f",
                "9aa5131dbdcdc1eac6bc98e3b4dcc4de",
                "098076fffa9976edcf70826492873e87",
                "16f365854a816d35b1758d09dcb8affb",
                "7bbf45e8d633be14accc80c5b7bb73d8",
            ),
            (
                "1032547698ba",
                "123456",
                "f8368339153b1fffffffffffffffffff",
                "c404b976af89503fe8d54db954b173ff",
                "c9892de92d7d5990be239367fd1717b3",
                "af2df099911e2948d1156e6e99a9081e",
                "ab364193b06b7a762d318a655e9ec468",
                "29ebd23d86dd251fd35a978adcb15722",
                "f9ec51bdf99566d6ea52bd19ec61a2cf",
            ),
        ];
        for (serial, product, auth, common, encrypt, update, ciphertext, challenge, proof) in
            vectors
        {
            let serial = hex::decode(serial).unwrap().try_into().unwrap();
            let keys = derive_modern(&serial, product).unwrap();
            assert_eq!(hex::encode(keys.auth_key), auth);
            assert_eq!(hex::encode(keys.common_key), common);
            assert_eq!(hex::encode(keys.encrypt_key), encrypt);
            assert_eq!(hex::encode(keys.update_key), update);
            assert_eq!(hex::encode(keys.auth_ciphertext()), ciphertext);
            let challenge = hex::decode(challenge).unwrap().try_into().unwrap();
            assert_eq!(hex::encode(keys.challenge_proof(&challenge)), proof);
        }
    }

    #[test]
    fn modern_product_serial_requires_unambiguous_ascii_tail() {
        for invalid in [
            "",
            "12345",
            "12345\0",
            "12345\n",
            "12345 ",
            " 123456",
            "123é56",
            "éPREFIX123456",
        ] {
            assert!(derive_modern(&[0; 6], invalid).is_err());
        }
        let expected = derive_modern(&[1, 2, 3, 4, 5, 6], "ABCDEF").unwrap();
        let prefixed = derive_modern(&[1, 2, 3, 4, 5, 6], "PREFIX-ABCDEF").unwrap();
        assert_eq!(expected.auth_key, prefixed.auth_key);
        assert_eq!(expected.update_key, prefixed.update_key);
        // Product tail characters are ASCII bytes, not a hexadecimal number,
        // and case must not be normalized when deriving an authentication key.
        let lower = derive_modern(&[1, 2, 3, 4, 5, 6], "abcdef").unwrap();
        assert_ne!(expected.auth_key, lower.auth_key);
    }

    #[test]
    fn keys_match_fixed_vectors_for_synthetic_serials() {
        // Synthetic identity inputs with fixed expected keys.
        let vectors = [
            (
                "000000000000",
                "ea45a3dce61651",
                "aa491095b0008dd85e82c71e4641b28d",
            ),
            (
                "010203040506",
                "3072389bc4278a",
                "1bb87b432948bb9f1c004ac736ab655c",
            ),
            (
                "ffffffffffff",
                "9859c9cc83775b",
                "e2b3804eb4221912f6bc9a9489269a73",
            ),
            (
                "1032547698ba",
                "48c6b2c6a0c6d4",
                "92fc8744494555d2cd598582fb6e57b1",
            ),
        ];
        for (serial, auth, update) in vectors {
            let serial: [u8; 6] = hex::decode(serial).unwrap().try_into().unwrap();
            let keys = derive(&serial);
            assert_eq!(hex::encode(keys.auth_key), auth);
            assert_eq!(hex::encode(keys.update_key), update);
        }
    }

    #[test]
    fn challenge_uses_single_ecb_block_without_padding() {
        // AES-128 FIPS197 known-answer vector, independent of firmware keys.
        let keys = LegacyKeys {
            auth_key: [0; 7],
            update_key: hex::decode("000102030405060708090a0b0c0d0e0f")
                .unwrap()
                .try_into()
                .unwrap(),
        };
        let challenge = hex::decode("00112233445566778899aabbccddeeff")
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(
            hex::encode(encrypt_challenge(&keys, &challenge)),
            "69c4e0d86a7b0430d8cdb78070b4c55a"
        );
    }
}
