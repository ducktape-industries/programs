//! How a typed key becomes bytes, so byte order is value order: integers big-endian and fixed
//! width, bytes and strings NUL-escaped and NUL-terminated, an option a 0 or a 1 then its value,
//! tuples concatenated (a prefix scan on the leading elements works), fixed arrays as they are.

pub trait KeyCodec: Sized {
    fn encode_key(&self, out: &mut Vec<u8>);
    /// Reads one key from the front of `bytes`, leaving the rest.
    fn decode_key(bytes: &mut &[u8]) -> Option<Self>;

    fn key_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode_key(&mut out);
        out
    }
}

fn take<'a>(bytes: &mut &'a [u8], len: usize) -> Option<&'a [u8]> {
    let (head, rest) = bytes.split_at_checked(len)?;
    *bytes = rest;
    Some(head)
}

macro_rules! integers {
    ($($t:ty),*) => {$(
        impl KeyCodec for $t {
            fn encode_key(&self, out: &mut Vec<u8>) {
                out.extend_from_slice(&self.to_be_bytes());
            }
            fn decode_key(bytes: &mut &[u8]) -> Option<Self> {
                take(bytes, size_of::<$t>()).map(|b| <$t>::from_be_bytes(b.try_into().unwrap()))
            }
        }
    )*};
}
integers!(u8, u16, u32, u64, u128);

fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    for &b in bytes {
        out.push(b);
        if b == 0 {
            out.push(0xFF);
        }
    }
    out.extend_from_slice(&[0, 0]);
}

fn get_bytes(bytes: &mut &[u8]) -> Option<Vec<u8>> {
    let mut value = Vec::new();
    let mut rest = *bytes;
    loop {
        match rest {
            [0, 0, tail @ ..] => {
                *bytes = tail;
                return Some(value);
            }
            [0, 0xFF, tail @ ..] => {
                value.push(0);
                rest = tail;
            }
            [b, tail @ ..] if *b != 0 => {
                value.push(*b);
                rest = tail;
            }
            _ => return None,
        }
    }
}

impl KeyCodec for Vec<u8> {
    fn encode_key(&self, out: &mut Vec<u8>) {
        put_bytes(out, self);
    }
    fn decode_key(bytes: &mut &[u8]) -> Option<Self> {
        get_bytes(bytes)
    }
}

impl KeyCodec for String {
    fn encode_key(&self, out: &mut Vec<u8>) {
        put_bytes(out, self.as_bytes());
    }
    fn decode_key(bytes: &mut &[u8]) -> Option<Self> {
        String::from_utf8(get_bytes(bytes)?).ok()
    }
}

impl<const N: usize> KeyCodec for [u8; N] {
    fn encode_key(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self);
    }
    fn decode_key(bytes: &mut &[u8]) -> Option<Self> {
        take(bytes, N)?.try_into().ok()
    }
}

impl KeyCodec for program::Principal {
    fn encode_key(&self, out: &mut Vec<u8>) {
        match self {
            program::Principal::Account(number) => {
                out.push(0);
                number.encode_key(out);
            }
            program::Principal::Root => out.push(1),
            program::Principal::Foreign(foreign) => {
                out.push(2);
                foreign.network.encode_key(out);
                foreign.account.encode_key(out);
            }
        }
    }
    fn decode_key(bytes: &mut &[u8]) -> Option<Self> {
        match take(bytes, 1)? {
            [0] => u64::decode_key(bytes).map(program::Principal::Account),
            [1] => Some(program::Principal::Root),
            [2] => {
                let network = <[u8; 32]>::decode_key(bytes)?;
                let account = u64::decode_key(bytes)?;
                Some(program::Principal::Foreign(program::Foreign {
                    network,
                    account,
                }))
            }
            _ => None,
        }
    }
}

impl<T: KeyCodec> KeyCodec for Option<T> {
    fn encode_key(&self, out: &mut Vec<u8>) {
        match self {
            None => out.push(0),
            Some(value) => {
                out.push(1);
                value.encode_key(out);
            }
        }
    }
    fn decode_key(bytes: &mut &[u8]) -> Option<Self> {
        match take(bytes, 1)? {
            [0] => Some(None),
            [1] => T::decode_key(bytes).map(Some),
            _ => None,
        }
    }
}

impl KeyCodec for () {
    fn encode_key(&self, _out: &mut Vec<u8>) {}
    fn decode_key(_bytes: &mut &[u8]) -> Option<Self> {
        Some(())
    }
}

macro_rules! tuples {
    ($(($($n:tt $t:ident),+))*) => {$(
        impl<$($t: KeyCodec),+> KeyCodec for ($($t,)+) {
            fn encode_key(&self, out: &mut Vec<u8>) {
                $(self.$n.encode_key(out);)+
            }
            fn decode_key(bytes: &mut &[u8]) -> Option<Self> {
                Some(($($t::decode_key(bytes)?,)+))
            }
        }
    )*};
}
tuples!((0 A) (0 A, 1 B) (0 A, 1 B, 2 C) (0 A, 1 B, 2 C, 3 D));

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<K: KeyCodec + PartialEq + std::fmt::Debug>(key: K) -> Vec<u8> {
        let bytes = key.key_bytes();
        let mut rest = bytes.as_slice();
        assert_eq!(K::decode_key(&mut rest), Some(key));
        assert!(rest.is_empty());
        bytes
    }

    #[test]
    fn integers_sort_numerically_and_tuples_scan_by_their_head() {
        assert!(round_trip(2u64) < round_trip(10u64));
        let head = 7u64.key_bytes();
        assert!(round_trip((7u64, "b".to_string())).starts_with(&head));
        assert!(round_trip((7u64, vec![1u8, 2], [9u8; 3])).starts_with(&head));
        assert!(round_trip((1u8, 2u16, 3u32, "s".to_string())).len() == 1 + 2 + 4 + 1 + 2);
        let principals = [1, 2, 255, 256, 257, u64::MAX].map(program::Principal::Account);
        let mut keys: Vec<Vec<u8>> = principals.iter().map(|p| round_trip(p.clone())).collect();
        keys.push(round_trip(program::Principal::Root));
        let foreign = |network: u8, account| {
            program::Principal::Foreign(program::Foreign {
                network: [network; 32],
                account,
            })
        };
        for principal in [foreign(1, 2), foreign(1, 256), foreign(2, 1)] {
            keys.push(round_trip(principal));
        }
        assert!(
            keys.is_sorted(),
            "accounts by number, then the chain, then foreign users by network and number"
        );
        assert_eq!(program::Principal::decode_key(&mut &[3u8][..]), None);
        assert_eq!(program::Principal::decode_key(&mut &[2u8, 1][..]), None);
        assert_eq!(u64::decode_key(&mut &[1u8, 2][..]), None);
        assert_eq!(String::decode_key(&mut &[b'a'][..]), None, "unterminated");
        assert_eq!(Vec::<u8>::decode_key(&mut &[0, 1][..]), None, "bad escape");
        assert_eq!(
            String::decode_key(&mut &[0xFF, 0, 0][..]),
            None,
            "not utf-8"
        );
    }

    #[test]
    fn an_option_sorts_none_first_then_by_its_value() {
        let keys = [None, Some(0u64), Some(1), Some(u64::MAX)].map(round_trip);
        assert!(keys.is_sorted());
        let scoped = [(7u64, None), (7, Some(2u64)), (8, None)].map(round_trip);
        assert!(scoped.is_sorted());
        assert!(scoped[1].starts_with(&7u64.key_bytes()));
        assert_eq!(Option::<u64>::decode_key(&mut &[2u8][..]), None);
        assert_eq!(Option::<u64>::decode_key(&mut &[1u8, 0][..]), None);
    }

    #[test]
    fn bytes_and_strings_sort_as_they_compare() {
        let mut values: Vec<Vec<u8>> = vec![
            vec![],
            vec![0],
            vec![0, 0],
            vec![0, 0xFF],
            vec![0xFF],
            b"a".to_vec(),
            b"a\0".to_vec(),
            b"a\0b".to_vec(),
            b"a\x01".to_vec(),
            b"ab".to_vec(),
            b"abc".to_vec(),
            b"b".to_vec(),
            b"docs".to_vec(),
            b"general".to_vec(),
            b"zz".to_vec(),
        ];
        for a in 0..=3u8 {
            for b in [0u8, 1, 0xFE, 0xFF] {
                values.push(vec![a, b]);
                values.push(vec![b, a, 0]);
            }
        }
        for x in &values {
            for y in &values {
                assert_eq!(
                    round_trip(x.clone()).cmp(&round_trip(y.clone())),
                    x.cmp(y),
                    "{x:?} {y:?}"
                );
                assert_eq!(
                    (x.clone(), 1u8)
                        .key_bytes()
                        .cmp(&(y.clone(), 0u8).key_bytes()),
                    (x, 1).cmp(&(y, 0)),
                    "{x:?} {y:?}"
                );
            }
        }
        let mut names =
            ["docs", "perf", "general", "random", "design", "forge-ci"].map(String::from);
        let mut keys: Vec<Vec<u8>> = names.iter().map(|n| round_trip(n.clone())).collect();
        keys.sort();
        names.sort();
        let decoded: Vec<String> = keys
            .iter()
            .map(|k| String::decode_key(&mut k.as_slice()).unwrap())
            .collect();
        assert_eq!(decoded, names);
    }
}
