//! The scripted server's view of Bedrock packet encryption.

use aes::Aes256;
use bytes::{Bytes, BytesMut};
use ctr::cipher::{KeyIvInit, StreamCipher};
use sha2::{Digest, Sha256};

type Aes256Ctr = ctr::Ctr32BE<Aes256>;

pub(super) struct ScriptCrypto {
    key: [u8; 32],
    decrypt_client: Aes256Ctr,
    encrypt_server: Aes256Ctr,
    client_counter: u64,
    server_counter: u64,
}

impl ScriptCrypto {
    pub(super) fn new(key: [u8; 32]) -> Self {
        let mut iv = [0u8; 16];
        iv[..12].copy_from_slice(&key[..12]);
        iv[15] = 2;
        Self {
            key,
            decrypt_client: Aes256Ctr::new_from_slices(&key, &iv).expect("fixed key and IV"),
            encrypt_server: Aes256Ctr::new_from_slices(&key, &iv).expect("fixed key and IV"),
            client_counter: 0,
            server_counter: 0,
        }
    }

    pub(super) fn decrypt_client(&mut self, frame: Bytes) -> Bytes {
        let mut frame = BytesMut::from(frame.as_ref());
        assert_eq!(frame.first().copied(), Some(0xfe));
        self.decrypt_client.apply_keystream(&mut frame[1..]);
        assert!(frame.len() >= 9);
        let checksum_at = frame.len() - 8;
        let expected = checksum(self.client_counter, &frame[1..checksum_at], &self.key);
        assert_eq!(&frame[checksum_at..], &expected);
        self.client_counter += 1;
        frame.truncate(checksum_at);
        frame.freeze()
    }

    pub(super) fn encrypt_server(&mut self, frame: Bytes) -> Bytes {
        let mut frame = BytesMut::from(frame.as_ref());
        assert_eq!(frame.first().copied(), Some(0xfe));
        let sum = checksum(self.server_counter, &frame[1..], &self.key);
        self.server_counter += 1;
        frame.extend_from_slice(&sum);
        self.encrypt_server.apply_keystream(&mut frame[1..]);
        frame.freeze()
    }
}

fn checksum(counter: u64, data: &[u8], key: &[u8; 32]) -> [u8; 8] {
    let mut digest = Sha256::new();
    digest.update(counter.to_le_bytes());
    digest.update(data);
    digest.update(key);
    let digest = digest.finalize();
    digest[..8].try_into().expect("eight bytes")
}
