//! Obfuscated intermediate transport. Frame only complete payloads: grammers
//! 0.11's intermediate unpacker checks len rather than len + 4 on partial input.
use crate::random;
use crate::words;
use grammers_crypto::{DequeBuffer, ObfuscatedCipher};
use grammers_mtproto::transport::{Intermediate, Tagged, Transport};

#[derive(Debug)]
pub(crate) enum Failure {
    Status(i32),
    Invalid(String),
}
impl From<String> for Failure {
    fn from(why: String) -> Self {
        Self::Invalid(why)
    }
}
impl From<&str> for Failure {
    fn from(why: &str) -> Self {
        Self::Invalid(why.into())
    }
}

pub(crate) struct Wire {
    cipher: ObfuscatedCipher,
    header: Option<[u8; 64]>,
    framing: Intermediate,
    incoming: Vec<u8>,
}

impl Wire {
    pub(crate) fn new() -> Self {
        let mut init: [u8; 64] = random::bytes();
        while init[0] == 0xef
            || init[4..8] == [0; 4]
            || [
                *b"HEAD",
                *b"POST",
                *b"GET ",
                *b"OPTI",
                [0x16, 3, 1, 2],
                [0xdd; 4],
                [0xee; 4],
            ]
            .contains(&init[..4].try_into().unwrap())
        {
            init = random::bytes();
        }
        let mut framing = Intermediate::new();
        init[56..60].copy_from_slice(&framing.init_tag());
        let mut cipher = ObfuscatedCipher::new(&init);
        let mut encrypted = init;
        cipher.encrypt(&mut encrypted);
        init[56..].copy_from_slice(&encrypted[56..]);
        Self {
            cipher,
            header: Some(init),
            framing,
            incoming: Vec::new(),
        }
    }

    pub(crate) fn pack(&mut self, body: &[u8]) -> Vec<u8> {
        let mut buffer = DequeBuffer::with_capacity(body.len(), 68);
        buffer.extend(body);
        self.framing.pack(&mut buffer);
        self.cipher.encrypt(buffer.as_mut());
        if let Some(header) = self.header.take() {
            buffer.extend_front(&header);
        }
        buffer.as_ref().to_vec()
    }

    pub(crate) fn feed(&mut self, mut bytes: Vec<u8>) -> Result<Vec<Vec<u8>>, Failure> {
        self.cipher.decrypt(&mut bytes);
        self.incoming.extend(bytes);
        let mut packets = Vec::new();
        let mut offset = 0;
        while self.incoming.len() - offset >= 4 {
            let length = i32::from_le_bytes(self.incoming[offset..offset + 4].try_into().unwrap());
            let frame_length_is_invalid = length <= 0 || length % 4 != 0;
            if frame_length_is_invalid {
                return Err(Failure::Invalid(words::invalid_frame(length)));
            }
            let length = length as usize;
            if self.incoming.len() - offset < length + 4 {
                break;
            }
            if length == 4 {
                let status =
                    i32::from_le_bytes(self.incoming[offset + 4..offset + 8].try_into().unwrap());
                return Err(Failure::Status(status));
            }
            packets.push(self.incoming[offset + 4..offset + 4 + length].to_vec());
            offset += 4 + length;
        }
        self.incoming.drain(..offset);
        Ok(packets)
    }
}
