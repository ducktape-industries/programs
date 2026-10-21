//! DH with the host's time. grammers' high-level step3 reads SystemTime,
//! unavailable on wasm32-unknown-unknown; the crypto here is grammers' own.
use crate::words;
use grammers_crypto::{AuthKey, aes, generate_key_data_from_nonce, rsa};
use grammers_tl_types::{self as tl, Deserializable, Serializable};
use num_bigint::BigUint;
use sha1::{Digest, Sha1};

use crate::random;

const GRAMMERS_MTPROTO_0_11_0_TEST_RSA_MODULUS: &str = "25342889448840415564971689590713473206898847759084779052582026594546022463853940585885215951168491965708222649399180603818074200620463776135424884632162512403163793083921641631564740959529419359595852941166848940585952337613333022396096584117954892216031229237302943701877588456738335398602461675225081791820393153757504952636234951323237820036543581047826906120927972487366805292115792231423684261262330394324750785450942589751755390156647751460719351439969059949569615302809050721500330239005077889855323917509948255722081644689442127297605422579707142646660768825302832201908302295573257427896031830742328565032949";

pub(crate) fn server_key(fingerprint: i64) -> Option<rsa::Key> {
    let modulus = match fingerprint {
        -5595554452916591101 => GRAMMERS_MTPROTO_0_11_0_TEST_RSA_MODULUS,
        #[cfg(test)]
        0x54455354 => include_str!("tests/rsa_n.txt").trim(),
        _ => return None,
    };
    rsa::Key::new(modulus, "65537")
}

pub(crate) struct Handshake {
    nonce: [u8; 16],
    server_nonce: [u8; 16],
    new_nonce: [u8; 32],
    key: Option<[u8; 256]>,
    pub(crate) offset: i64,
}

impl Handshake {
    pub(crate) fn new() -> (Self, Vec<u8>) {
        let nonce = random::bytes();
        (
            Self {
                nonce,
                server_nonce: [0; 16],
                new_nonce: [0; 32],
                key: None,
                offset: 0,
            },
            tl::functions::ReqPqMulti { nonce }.to_bytes(),
        )
    }

    pub(crate) fn pq(&mut self, body: &[u8]) -> Result<Vec<u8>, String> {
        let tl::enums::ResPq::Pq(pq) = decode(body)?;
        let pq_nonce_or_length_is_invalid = pq.nonce != self.nonce || pq.pq.len() != 8;
        if pq_nonce_or_length_is_invalid {
            return Err(words::BAD_PQ_NONCE.into());
        }
        let fingerprint = pq
            .server_public_key_fingerprints
            .iter()
            .copied()
            .find(|fingerprint| server_key(*fingerprint).is_some())
            .ok_or(words::UNKNOWN_RSA)?;
        let product = u64::from_be_bytes(pq.pq.as_slice().try_into().unwrap());
        if product < 4 {
            return Err(words::BAD_PQ.into());
        }
        let (p, q) = fallible_bounded_pollard_rho_factors(product).ok_or(words::UNFACTORED_PQ)?;
        let pq_factors_are_invalid = p.checked_mul(q) != Some(product) || p < 2 || q < 2;
        if pq_factors_are_invalid {
            return Err(words::BAD_PQ_FACTORS.into());
        }
        let minimal = |n: u64| {
            let bytes = n.to_be_bytes();
            bytes[bytes.iter().position(|b| *b != 0).unwrap_or(7)..].to_vec()
        };
        let (p, q) = (minimal(p), minimal(q));
        self.server_nonce = pq.server_nonce;
        self.new_nonce = random::bytes();
        let inner = tl::enums::PQInnerData::Data(tl::types::PQInnerData {
            pq: pq.pq,
            p: p.clone(),
            q: q.clone(),
            nonce: self.nonce,
            server_nonce: self.server_nonce,
            new_nonce: self.new_nonce,
        })
        .to_bytes();
        Ok(tl::functions::ReqDhParams {
            nonce: self.nonce,
            server_nonce: self.server_nonce,
            p,
            q,
            public_key_fingerprint: fingerprint,
            encrypted_data: rsa::encrypt_hashed(
                &inner,
                &server_key(fingerprint).unwrap(),
                &random::bytes(),
            ),
        }
        .to_bytes())
    }

    pub(crate) fn dh(&mut self, body: &[u8], unix: i64) -> Result<Vec<u8>, String> {
        let params: tl::enums::ServerDhParams = decode(body)?;
        let tl::enums::ServerDhParams::Ok(mut params) = params else {
            return Err(words::DH_DECLINED.into());
        };
        self.check_nonces(params.nonce, params.server_nonce)?;
        let dh_encrypted_answer_length_is_invalid =
            params.encrypted_answer.len() < 32 || !params.encrypted_answer.len().is_multiple_of(16);
        if dh_encrypted_answer_length_is_invalid {
            return Err(words::BAD_DH_LENGTH.into());
        }
        let (key, iv) = generate_key_data_from_nonce(&self.server_nonce, &self.new_nonce);
        aes::ige_decrypt(&mut params.encrypted_answer, &key, &iv);
        let mut cursor = tl::Cursor::from_slice(&params.encrypted_answer[20..]);
        let tl::enums::ServerDhInnerData::Data(inner) =
            tl::enums::ServerDhInnerData::deserialize(&mut cursor).map_err(|e| e.to_string())?;
        if Sha1::digest(&params.encrypted_answer[20..20 + cursor.pos()])[..]
            != params.encrypted_answer[..20]
        {
            return Err(words::BAD_DH_HASH.into());
        }
        self.check_nonces(inner.nonce, inner.server_nonce)?;
        let dh_prime_or_generator_is_invalid = !(2..=7).contains(&inner.g)
            || !grammers_crypto::two_factor_auth::check_p_and_g(&inner.dh_prime, &inner.g);
        if dh_prime_or_generator_is_invalid {
            return Err(words::BAD_PRIME.into());
        }
        let prime = BigUint::from_bytes_be(&inner.dh_prime);
        let ga = BigUint::from_bytes_be(&inner.g_a);
        check_public(&ga, &prime)?;
        let secret = BigUint::from_bytes_be(&random::bytes::<256>());
        let gb = BigUint::from(inner.g as u32).modpow(&secret, &prime);
        check_public(&gb, &prime)?;
        self.key = Some(padded_key(&ga.modpow(&secret, &prime)));
        self.offset = i64::from(inner.server_time) - unix;
        let inner = tl::enums::ClientDhInnerData::Data(tl::types::ClientDhInnerData {
            nonce: self.nonce,
            server_nonce: self.server_nonce,
            retry_id: 0,
            g_b: gb.to_bytes_be(),
        })
        .to_bytes();
        let mut hashed = Sha1::digest(&inner).to_vec();
        hashed.extend(inner);
        let padding = (16 - hashed.len() % 16) % 16;
        hashed.extend_from_slice(&random::bytes::<16>()[..padding]);
        aes::ige_encrypt(&mut hashed, &key, &iv);
        Ok(tl::functions::SetClientDhParams {
            nonce: self.nonce,
            server_nonce: self.server_nonce,
            encrypted_data: hashed,
        }
        .to_bytes())
    }

    pub(crate) fn finish(&self, body: &[u8]) -> Result<([u8; 256], i64), String> {
        let reply: tl::enums::SetClientDhParamsAnswer = decode(body)?;
        let tl::enums::SetClientDhParamsAnswer::DhGenOk(reply) = reply else {
            return Err(words::KEY_DECLINED.into());
        };
        self.check_nonces(reply.nonce, reply.server_nonce)?;
        let key = self.key.ok_or(words::INCOMPLETE_DH)?;
        if AuthKey::from_bytes(key).calc_new_nonce_hash(&self.new_nonce, 1) != reply.new_nonce_hash1
        {
            return Err(words::BAD_KEY_CONFIRMATION.into());
        }
        let salt = std::array::from_fn(|i| self.new_nonce[i] ^ self.server_nonce[i]);
        Ok((key, i64::from_le_bytes(salt)))
    }

    fn check_nonces(&self, nonce: [u8; 16], server: [u8; 16]) -> Result<(), String> {
        let client_or_server_nonce_mismatches = nonce != self.nonce || server != self.server_nonce;
        if client_or_server_nonce_mismatches {
            Err(words::BAD_NONCES.into())
        } else {
            Ok(())
        }
    }
}

pub(crate) fn decode<T: Deserializable>(bytes: &[u8]) -> Result<T, String> {
    T::from_bytes(bytes).map_err(|error| error.to_string())
}

pub(crate) fn padded_key(number: &BigUint) -> [u8; 256] {
    let bytes = number.to_bytes_be();
    let mut padded = [0; 256];
    padded[256 - bytes.len()..].copy_from_slice(&bytes);
    padded
}

pub(crate) fn check_public(number: &BigUint, prime: &BigUint) -> Result<(), String> {
    let bound = BigUint::from(1u32) << 1984;
    let dh_public_is_outside_prime_and_safety_bounds =
        prime.bits() != 2048 || number <= &bound || number >= &(prime - &bound);
    if dh_public_is_outside_prime_and_safety_bounds {
        Err(words::BAD_PUBLIC.into())
    } else {
        Ok(())
    }
}

fn fallible_bounded_pollard_rho_factors(product: u64) -> Option<(u64, u64)> {
    if product.is_multiple_of(2) {
        return Some((2, product / 2));
    }
    let expected_rho_steps_for_32_bit_factor = 1 << 16;
    let mut remaining_rho_steps_across_five_attempts = expected_rho_steps_for_32_bit_factor << 4;
    for constant in [43u64, 47, 53, 59, 61] {
        let step = |n: u64| {
            ((u128::from(n) * u128::from(n) + u128::from(constant)) % u128::from(product)) as u64
        };
        let mut floyd_slow = 2;
        let mut floyd_fast = 2;
        loop {
            if remaining_rho_steps_across_five_attempts == 0 {
                return None;
            }
            remaining_rho_steps_across_five_attempts -= 1;
            floyd_slow = step(floyd_slow);
            floyd_fast = step(step(floyd_fast));
            let mut divisor = floyd_slow.abs_diff(floyd_fast);
            let mut remainder = product;
            while remainder != 0 {
                (divisor, remainder) = (remainder, divisor % remainder);
            }
            if divisor == product {
                break;
            }
            if divisor > 1 {
                return Some((
                    divisor.min(product / divisor),
                    divisor.max(product / divisor),
                ));
            }
        }
    }
    None
}
