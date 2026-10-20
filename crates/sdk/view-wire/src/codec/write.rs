//! The wire's writer: MessagePack with no names in it. Both sides are one
//! build of this crate (`WIRE_ID`), so a name tells the reader nothing its
//! own types do not.
//!
//! - A struct is the array of its fields, in declaration order.
//! - A sparse struct ([`sparse!`](crate::sparse)) is a map from a field's
//!   declaration index to its value, holding only the fields that say
//!   something.
//! - An enum's variant is its declaration index: alone for a unit variant,
//!   as the one key of a map for a variant that carries data.
//! - Bytes are a `bin`, the bytes as they are, where the field says so
//!   ([`bin`](super::bin)): serde has no way to tell a `Vec<u8>` from any
//!   other sequence.
//!
//! `rmp-serde` reads the first three: a derived `Deserialize` takes a struct from
//! an array or a map, and a field or a variant by its index. A `bin` is read
//! by [`bin`](super::bin)'s own visitor.

use serde::Serialize;
use serde::ser;

pub(crate) const SPARSE: &str = "$view_wire::sparse";

#[derive(Debug)]
pub(crate) struct Refused(pub(crate) String);

impl std::fmt::Display for Refused {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}
impl std::error::Error for Refused {}
impl ser::Error for Refused {
    fn custom<T: std::fmt::Display>(message: T) -> Self {
        Self(message.to_string())
    }
}

fn refuse<T>(what: &'static str) -> Result<T, Refused> {
    Err(Refused(what.into()))
}

pub(crate) struct Writer<'a> {
    out: &'a mut Vec<u8>,
    sparse: bool,
}

impl<'a> Writer<'a> {
    pub(crate) fn new(out: &'a mut Vec<u8>) -> Self {
        Self { out, sparse: false }
    }
}

fn uint(out: &mut Vec<u8>, value: u64) {
    match value {
        0..=0x7f => out.push(value as u8),
        0x80..=0xff => out.extend_from_slice(&[0xcc, value as u8]),
        0x100..=0xffff => {
            out.push(0xcd);
            out.extend_from_slice(&(value as u16).to_be_bytes());
        }
        0x1_0000..=0xffff_ffff => {
            out.push(0xce);
            out.extend_from_slice(&(value as u32).to_be_bytes());
        }
        _ => {
            out.push(0xcf);
            out.extend_from_slice(&value.to_be_bytes());
        }
    }
}

fn int(out: &mut Vec<u8>, value: i64) {
    match value {
        0.. => uint(out, value as u64),
        -32..=-1 => out.push(value as u8),
        -0x80..=-33 => out.extend_from_slice(&[0xd0, value as u8]),
        -0x8000..=-0x81 => {
            out.push(0xd1);
            out.extend_from_slice(&(value as i16).to_be_bytes());
        }
        -0x8000_0000..=-0x8001 => {
            out.push(0xd2);
            out.extend_from_slice(&(value as i32).to_be_bytes());
        }
        _ => {
            out.push(0xd3);
            out.extend_from_slice(&value.to_be_bytes());
        }
    }
}

fn write_length_header(
    out: &mut Vec<u8>,
    len: usize,
    short: u8,
    fits: usize,
    wide: u8,
) -> Result<(), Refused> {
    match len {
        len if len < fits => out.push(short | len as u8),
        len if len <= 0xffff => {
            out.push(wide);
            out.extend_from_slice(&(len as u16).to_be_bytes());
        }
        len if len <= 0xffff_ffff => {
            out.push(wide + 1);
            out.extend_from_slice(&(len as u32).to_be_bytes());
        }
        _ => return refuse("longer than MessagePack counts"),
    }
    Ok(())
}

fn array(out: &mut Vec<u8>, len: usize) -> Result<(), Refused> {
    write_length_header(out, len, 0x90, 16, 0xdc)
}

fn map(out: &mut Vec<u8>, len: usize) -> Result<(), Refused> {
    write_length_header(out, len, 0x80, 16, 0xde)
}

fn write_variant_map(out: &mut Vec<u8>, index: u32) {
    out.push(0x81);
    uint(out, u64::from(index));
}

impl<'a> ser::Serializer for Writer<'a> {
    type Ok = ();
    type Error = Refused;
    type SerializeSeq = Elements<'a>;
    type SerializeTuple = Elements<'a>;
    type SerializeTupleStruct = Elements<'a>;
    type SerializeTupleVariant = Elements<'a>;
    type SerializeMap = Elements<'a>;
    type SerializeStruct = Fields<'a>;
    type SerializeStructVariant = Fields<'a>;

    fn is_human_readable(&self) -> bool {
        false
    }

    fn serialize_bool(self, value: bool) -> Result<(), Refused> {
        self.out.push(0xc2 | u8::from(value));
        Ok(())
    }
    fn serialize_i8(self, value: i8) -> Result<(), Refused> {
        self.serialize_i64(i64::from(value))
    }
    fn serialize_i16(self, value: i16) -> Result<(), Refused> {
        self.serialize_i64(i64::from(value))
    }
    fn serialize_i32(self, value: i32) -> Result<(), Refused> {
        self.serialize_i64(i64::from(value))
    }
    fn serialize_i64(self, value: i64) -> Result<(), Refused> {
        int(self.out, value);
        Ok(())
    }
    fn serialize_u8(self, value: u8) -> Result<(), Refused> {
        self.serialize_u64(u64::from(value))
    }
    fn serialize_u16(self, value: u16) -> Result<(), Refused> {
        self.serialize_u64(u64::from(value))
    }
    fn serialize_u32(self, value: u32) -> Result<(), Refused> {
        self.serialize_u64(u64::from(value))
    }
    fn serialize_u64(self, value: u64) -> Result<(), Refused> {
        uint(self.out, value);
        Ok(())
    }
    fn serialize_f32(self, value: f32) -> Result<(), Refused> {
        self.out.push(0xca);
        self.out.extend_from_slice(&value.to_be_bytes());
        Ok(())
    }
    fn serialize_f64(self, value: f64) -> Result<(), Refused> {
        self.out.push(0xcb);
        self.out.extend_from_slice(&value.to_be_bytes());
        Ok(())
    }
    fn serialize_char(self, value: char) -> Result<(), Refused> {
        self.serialize_str(value.encode_utf8(&mut [0; 4]))
    }
    fn serialize_str(self, value: &str) -> Result<(), Refused> {
        match value.len() {
            len if len < 32 => self.out.push(0xa0 | len as u8),
            len if len <= 0xff => self.out.extend_from_slice(&[0xd9, len as u8]),
            len => write_length_header(self.out, len, 0, 0, 0xda)?,
        }
        self.out.extend_from_slice(value.as_bytes());
        Ok(())
    }
    fn serialize_bytes(self, value: &[u8]) -> Result<(), Refused> {
        match value.len() {
            len if len <= 0xff => self.out.extend_from_slice(&[0xc4, len as u8]),
            len => write_length_header(self.out, len, 0, 0, 0xc5)?,
        }
        self.out.extend_from_slice(value);
        Ok(())
    }
    fn serialize_none(self) -> Result<(), Refused> {
        self.serialize_unit()
    }
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<(), Refused> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<(), Refused> {
        self.out.push(0xc0);
        Ok(())
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<(), Refused> {
        array(self.out, 0)
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        index: u32,
        _: &'static str,
    ) -> Result<(), Refused> {
        uint(self.out, u64::from(index));
        Ok(())
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<(), Refused> {
        value.serialize(Writer {
            out: self.out,
            sparse: name == SPARSE,
        })
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        index: u32,
        _: &'static str,
        value: &T,
    ) -> Result<(), Refused> {
        write_variant_map(self.out, index);
        value.serialize(Writer::new(self.out))
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<Elements<'a>, Refused> {
        match len {
            Some(len) => self.serialize_tuple(len),
            None => Ok(Elements::counted(self.out, 0xdd)),
        }
    }
    fn serialize_tuple(self, len: usize) -> Result<Elements<'a>, Refused> {
        array(self.out, len)?;
        Ok(Elements {
            out: self.out,
            counted: None,
        })
    }
    fn serialize_tuple_struct(self, _: &'static str, len: usize) -> Result<Elements<'a>, Refused> {
        self.serialize_tuple(len)
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        index: u32,
        _: &'static str,
        len: usize,
    ) -> Result<Elements<'a>, Refused> {
        write_variant_map(self.out, index);
        self.serialize_tuple(len)
    }
    fn serialize_map(self, len: Option<usize>) -> Result<Elements<'a>, Refused> {
        let Some(len) = len else {
            return Ok(Elements::counted(self.out, 0xdf));
        };
        map(self.out, len)?;
        Ok(Elements {
            out: self.out,
            counted: None,
        })
    }
    fn serialize_struct(self, _: &'static str, len: usize) -> Result<Fields<'a>, Refused> {
        match self.sparse {
            true => map(self.out, len)?,
            false => array(self.out, len)?,
        }
        Ok(Fields {
            out: self.out,
            sparse: self.sparse,
            index: 0,
        })
    }
    fn serialize_struct_variant(
        self,
        name: &'static str,
        index: u32,
        _: &'static str,
        len: usize,
    ) -> Result<Fields<'a>, Refused> {
        write_variant_map(self.out, index);
        Writer::new(self.out).serialize_struct(name, len)
    }
}

pub(crate) struct Elements<'a> {
    out: &'a mut Vec<u8>,
    counted: Option<(usize, u32)>,
}

impl<'a> Elements<'a> {
    fn counted(out: &'a mut Vec<u8>, marker: u8) -> Self {
        out.push(marker);
        let at = out.len();
        out.extend_from_slice(&[0; 4]);
        Self {
            out,
            counted: Some((at, 0)),
        }
    }

    fn write<T: ?Sized + Serialize>(&mut self, value: &T, counts: u32) -> Result<(), Refused> {
        if let Some((_, count)) = &mut self.counted {
            *count += counts;
        }
        value.serialize(Writer::new(self.out))
    }

    fn end(self) {
        if let Some((at, count)) = self.counted {
            self.out[at..at + 4].copy_from_slice(&count.to_be_bytes());
        }
    }
}

macro_rules! elements {
    ($($compound:ident $($method:ident $counts:literal)+;)+) => {$(
        impl ser::$compound for Elements<'_> {
            type Ok = ();
            type Error = Refused;
            $(fn $method<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Refused> {
                self.write(value, $counts)
            })+
            fn end(self) -> Result<(), Refused> {
                Elements::end(self);
                Ok(())
            }
        }
    )+};
}
elements! {
    SerializeSeq serialize_element 1;
    SerializeTuple serialize_element 1;
    SerializeTupleStruct serialize_field 1;
    SerializeTupleVariant serialize_field 1;
    SerializeMap serialize_key 1 serialize_value 0;
}

pub(crate) struct Fields<'a> {
    out: &'a mut Vec<u8>,
    sparse: bool,
    index: u64,
}

impl Fields<'_> {
    fn field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Refused> {
        if self.sparse {
            uint(self.out, self.index);
        }
        self.index += 1;
        value.serialize(Writer::new(self.out))
    }

    fn skip(&mut self) -> Result<(), Refused> {
        self.index += 1;
        match self.sparse {
            true => Ok(()),
            false => refuse("a struct that leaves a field out is not marked sparse"),
        }
    }
}

macro_rules! fields {
    ($($compound:ident)+) => {$(
        impl ser::$compound for Fields<'_> {
            type Ok = ();
            type Error = Refused;
            fn serialize_field<T: ?Sized + Serialize>(
                &mut self,
                _: &'static str,
                value: &T,
            ) -> Result<(), Refused> {
                self.field(value)
            }
            fn skip_field(&mut self, _: &'static str) -> Result<(), Refused> {
                self.skip()
            }
            fn end(self) -> Result<(), Refused> {
                Ok(())
            }
        }
    )+};
}
fields!(SerializeStruct SerializeStructVariant);
