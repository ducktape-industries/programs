//! The wire's reader: what `write.rs` writes, read through serde, tolerant
//! of what a later build appends (`codec.rs`, "Appends"): trailing fields,
//! a sparse struct's unknown keys, a variant this build does not know.

use serde::de::{self, DeserializeSeed, IntoDeserializer, Visitor};

use super::{FALLBACKS, FELL};

#[derive(Debug)]
pub(crate) struct Error {
    message: String,
    unknown: bool,
}

impl Error {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            unknown: false,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl de::Error for Error {
    fn custom<T: std::fmt::Display>(message: T) -> Self {
        Self::new(message.to_string())
    }
}

const CUT: &str = "the bytes end inside a value";

pub(crate) struct Reader<'de> {
    bytes: &'de [u8],
    at: usize,
}

impl<'de> Reader<'de> {
    pub(crate) fn new(bytes: &'de [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    pub(crate) fn rest(&self) -> usize {
        self.bytes.len() - self.at
    }

    fn peek(&self) -> Result<u8, Error> {
        self.bytes
            .get(self.at)
            .copied()
            .ok_or_else(|| Error::new(CUT))
    }

    fn take(&mut self, len: usize) -> Result<&'de [u8], Error> {
        let end = self
            .at
            .checked_add(len)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| Error::new(CUT))?;
        let taken = &self.bytes[self.at..end];
        self.at = end;
        Ok(taken)
    }

    fn byte(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }

    fn be(&mut self, len: usize) -> Result<u64, Error> {
        Ok(self
            .take(len)?
            .iter()
            .fold(0, |value, byte| (value << 8) | u64::from(*byte)))
    }

    fn container(&mut self, marker: u8) -> Result<Option<(u64, u64)>, Error> {
        Ok(match marker {
            0x90..=0x9f => Some((u64::from(marker & 0x0f), 1)),
            0x80..=0x8f => Some((u64::from(marker & 0x0f), 2)),
            0xdc => Some((self.be(2)?, 1)),
            0xdd => Some((self.be(4)?, 1)),
            0xde => Some((self.be(2)?, 2)),
            0xdf => Some((self.be(4)?, 2)),
            _ => None,
        })
    }

    pub(crate) fn skip(&mut self) -> Result<(), Error> {
        let mut owed: u64 = 1;
        while owed > 0 {
            owed -= 1;
            let marker = self.byte()?;
            if let Some((count, each)) = self.container(marker)? {
                owed += count * each;
                if owed > self.rest() as u64 {
                    return Err(Error::new(CUT));
                }
                continue;
            }
            let len = match marker {
                0x00..=0x7f | 0xe0..=0xff | 0xc0 | 0xc2 | 0xc3 => 0,
                0xa0..=0xbf => u64::from(marker & 0x1f),
                0xcc | 0xd0 => 1,
                0xcd | 0xd1 => 2,
                0xca | 0xce | 0xd2 => 4,
                0xcb | 0xcf | 0xd3 => 8,
                0xc4 | 0xd9 => self.be(1)?,
                0xc5 | 0xda => self.be(2)?,
                0xc6 | 0xdb => self.be(4)?,
                _ => return Err(Error::new(format!("no wire value starts {marker:#04x}"))),
            };
            self.take(usize::try_from(len).map_err(|_| Error::new(CUT))?)?;
        }
        Ok(())
    }

    fn str(&mut self, marker: u8) -> Result<Option<&'de str>, Error> {
        let len = match marker {
            0xa0..=0xbf => u64::from(marker & 0x1f),
            0xd9 => self.be(1)?,
            0xda => self.be(2)?,
            0xdb => self.be(4)?,
            _ => return Ok(None),
        };
        let bytes = self.take(usize::try_from(len).map_err(|_| Error::new(CUT))?)?;
        std::str::from_utf8(bytes)
            .map(Some)
            .map_err(|_| Error::new("a string that is not UTF-8"))
    }

    fn bin(&mut self, marker: u8) -> Result<Option<&'de [u8]>, Error> {
        let len = match marker {
            0xc4 => self.be(1)?,
            0xc5 => self.be(2)?,
            0xc6 => self.be(4)?,
            _ => return Ok(None),
        };
        self.take(usize::try_from(len).map_err(|_| Error::new(CUT))?)
            .map(Some)
    }

    fn integer(&mut self, marker: u8) -> Result<Option<Integer>, Error> {
        Ok(Some(match marker {
            0x00..=0x7f => Integer::Unsigned(u64::from(marker)),
            0xe0..=0xff => Integer::Signed(i64::from(marker as i8)),
            0xcc => Integer::Unsigned(self.be(1)?),
            0xcd => Integer::Unsigned(self.be(2)?),
            0xce => Integer::Unsigned(self.be(4)?),
            0xcf => Integer::Unsigned(self.be(8)?),
            0xd0 => Integer::Signed(i64::from(self.be(1)? as u8 as i8)),
            0xd1 => Integer::Signed(i64::from(self.be(2)? as u16 as i16)),
            0xd2 => Integer::Signed(i64::from(self.be(4)? as u32 as i32)),
            0xd3 => Integer::Signed(self.be(8)? as i64),
            _ => return Ok(None),
        }))
    }

    fn visit_items<V: Visitor<'de>>(
        &mut self,
        count: u64,
        each: u64,
        visitor: V,
    ) -> Result<V::Value, Error> {
        if count * each > self.rest() as u64 {
            return Err(Error::new(CUT));
        }
        let mut items = Items {
            reader: self,
            left: count,
        };
        let value = match each {
            1 => visitor.visit_seq(&mut items)?,
            _ => visitor.visit_map(&mut items)?,
        };
        let left = items.left * each;
        for _ in 0..left {
            self.skip()?;
        }
        Ok(value)
    }
}

enum Integer {
    Unsigned(u64),
    Signed(i64),
}

struct Items<'a, 'de> {
    reader: &'a mut Reader<'de>,
    left: u64,
}

impl<'de> de::SeqAccess<'de> for Items<'_, 'de> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        if self.left == 0 {
            return Ok(None);
        }
        self.left -= 1;
        seed.deserialize(&mut *self.reader).map(Some)
    }

    fn size_hint(&self) -> Option<usize> {
        usize::try_from(self.left).ok()
    }
}

impl<'de> de::MapAccess<'de> for Items<'_, 'de> {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        if self.left == 0 {
            return Ok(None);
        }
        self.left -= 1;
        seed.deserialize(&mut *self.reader).map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        seed.deserialize(&mut *self.reader)
    }

    fn size_hint(&self) -> Option<usize> {
        usize::try_from(self.left).ok()
    }
}

impl<'de> de::Deserializer<'de> for &mut Reader<'de> {
    type Error = Error;

    fn is_human_readable(&self) -> bool {
        false
    }

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let marker = self.byte()?;
        if let Some(integer) = self.integer(marker)? {
            return match integer {
                Integer::Unsigned(value) => visitor.visit_u64(value),
                Integer::Signed(value) => visitor.visit_i64(value),
            };
        }
        if let Some(text) = self.str(marker)? {
            return visitor.visit_borrowed_str(text);
        }
        if let Some(bytes) = self.bin(marker)? {
            return visitor.visit_borrowed_bytes(bytes);
        }
        if let Some((count, each)) = self.container(marker)? {
            return self.visit_items(count, each, visitor);
        }
        match marker {
            0xc0 => visitor.visit_unit(),
            0xc2 => visitor.visit_bool(false),
            0xc3 => visitor.visit_bool(true),
            0xca => visitor.visit_f32(f32::from_bits(self.be(4)? as u32)),
            0xcb => visitor.visit_f64(f64::from_bits(self.be(8)?)),
            _ => Err(Error::new(format!("no wire value starts {marker:#04x}"))),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.peek()? {
            0xc0 => {
                self.at += 1;
                visitor.visit_none()
            }
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.byte()? {
            0xc0 | 0x90 => visitor.visit_unit(),
            marker => Err(Error::new(format!("a unit, not {marker:#04x}"))),
        }
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        if !FALLBACKS.contains(&name) {
            return self.read_enum(visitor);
        }
        let start = self.at;
        match self.read_enum(visitor) {
            Err(error) if error.unknown => {
                self.at = start;
                self.skip()?;
                Err(Error::new(format!("{FELL}{name}: {}", error.message)))
            }
            answer => answer,
        }
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.skip()?;
        visitor.visit_unit()
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf seq tuple tuple_struct map struct identifier
    }
}

impl<'de> Reader<'de> {
    fn read_enum<V: Visitor<'de>>(&mut self, visitor: V) -> Result<V::Value, Error> {
        let mut marker = self.byte()?;
        let payload = marker == 0x81;
        if payload {
            marker = self.byte()?;
        }
        let tag = match self.integer(marker)? {
            Some(Integer::Unsigned(index)) => Tag::Index(index),
            Some(Integer::Signed(_)) => return Err(Error::new("a variant index is not negative")),
            None => match self.str(marker)? {
                Some(name) => Tag::Name(name),
                None => {
                    return Err(Error::new(
                        "a variant is its index or its name, alone or as a one-entry map's key",
                    ));
                }
            },
        };
        visitor.visit_enum(Variant {
            reader: self,
            tag,
            payload,
        })
    }
}

enum Tag<'de> {
    Index(u64),
    Name(&'de str),
}

struct Variant<'a, 'de> {
    reader: &'a mut Reader<'de>,
    tag: Tag<'de>,
    payload: bool,
}

impl<'de> de::EnumAccess<'de> for Variant<'_, 'de> {
    type Error = Error;
    type Variant = Self;

    fn variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<(T::Value, Self), Error> {
        let read = match self.tag {
            Tag::Index(index) => {
                let index: de::value::U64Deserializer<Error> = index.into_deserializer();
                seed.deserialize(index)
            }
            Tag::Name(name) => {
                seed.deserialize(de::value::BorrowedStrDeserializer::<Error>::new(name))
            }
        };
        match read {
            Ok(value) => Ok((value, self)),
            Err(mut error) => {
                error.unknown = true;
                Err(error)
            }
        }
    }
}

impl<'de> de::VariantAccess<'de> for Variant<'_, 'de> {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        match self.payload {
            true => self.reader.skip(),
            false => Ok(()),
        }
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, Error> {
        self.carried()?;
        seed.deserialize(self.reader)
    }

    fn tuple_variant<V: Visitor<'de>>(self, _: usize, visitor: V) -> Result<V::Value, Error> {
        self.carried()?;
        de::Deserializer::deserialize_any(self.reader, visitor)
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.carried()?;
        de::Deserializer::deserialize_any(self.reader, visitor)
    }
}

impl Variant<'_, '_> {
    fn carried(&self) -> Result<(), Error> {
        match self.payload {
            true => Ok(()),
            false => Err(Error::new("a variant with no data where data was expected")),
        }
    }
}
