//! Borsh bytes to JSON by a schema, in the one form `encode` reads back to the same bytes.
use borsh::schema::{BorshSchemaContainer, Declaration, Fields};
use serde_json::{Map, Number, Value, json};

use super::{At, Fault, Kind, Length, Variants, kind, payload, spelled, unit_variant};

pub fn decode(
    schema: &BorshSchemaContainer,
    declaration: &str,
    name: &str,
    bytes: &[u8],
) -> Result<Value, Fault> {
    let mut reader = Reader {
        schema,
        rest: bytes,
    };
    let value = reader.value(declaration, &At::root(name))?;
    match reader.rest.len() {
        0 => Ok(value),
        left => Err(Fault::Trailing { left }),
    }
}

struct Reader<'s, 'b> {
    schema: &'s BorshSchemaContainer,
    rest: &'b [u8],
}

impl<'s> Reader<'s, '_> {
    fn value(&mut self, declaration: &'s str, at: &At) -> Result<Value, Fault> {
        match kind(self.schema, declaration)? {
            Kind::Unit => Ok(Value::Null),
            Kind::Bool => self.boolean(at),
            Kind::Int { bytes, signed } => self.int(bytes, signed, at),
            Kind::Float { bytes } => self.float(bytes, at),
            Kind::Text(length) => self.text(length, at),
            Kind::Bytes(length) => self.bytes(length, at),
            Kind::Option(inner) => self.option(inner, at),
            Kind::Seq { length, elements } => self.seq(length, elements, at),
            Kind::Tuple(elements) => self.tuple(elements, at),
            Kind::Struct(fields) => self.fields(fields, at),
            Kind::Enum {
                tag_width,
                variants,
            } => self.variant(tag_width, variants, at),
        }
    }

    fn take(&mut self, count: usize, at: &At) -> Result<&[u8], Fault> {
        let Some((taken, rest)) = self.rest.split_at_checked(count) else {
            return Err(Fault::Truncated { at: at.render() });
        };
        self.rest = rest;
        Ok(taken)
    }

    fn le(&mut self, width: u8, at: &At) -> Result<[u8; 16], Fault> {
        let mut le = [0u8; 16];
        let Some(slot) = le.get_mut(..usize::from(width)) else {
            return Err(at.mismatch(format!("a number of at most 16 bytes, not {width}")));
        };
        slot.copy_from_slice(self.take(usize::from(width), at)?);
        Ok(le)
    }

    fn byte(&mut self, at: &At) -> Result<u8, Fault> {
        Ok(self.le(1, at)?[0])
    }

    fn boolean(&mut self, at: &At) -> Result<Value, Fault> {
        match self.byte(at)? {
            0 => Ok(Value::Bool(false)),
            1 => Ok(Value::Bool(true)),
            _ => Err(at.mismatch("a bool byte, 0 or 1")),
        }
    }

    fn int(&mut self, bytes: u8, signed: bool, at: &At) -> Result<Value, Fault> {
        let mut le = self.le(bytes, at)?;
        let negative = signed && le[usize::from(bytes) - 1] & 0x80 != 0;
        if negative {
            le[usize::from(bytes)..].fill(0xff);
        }
        let wide = bytes == 16;
        let value = match (signed, wide) {
            (false, false) => json!(u64::from_le_bytes(low(&le))),
            (true, false) => json!(i64::from_le_bytes(low(&le))),
            (false, true) => json!(u128::from_le_bytes(le).to_string()),
            (true, true) => json!(i128::from_le_bytes(le).to_string()),
        };
        Ok(value)
    }

    fn float(&mut self, bytes: u8, at: &At) -> Result<Value, Fault> {
        let le = self.le(bytes, at)?;
        let number = match bytes {
            4 => f64::from(f32::from_le_bytes([le[0], le[1], le[2], le[3]])),
            _ => f64::from_le_bytes(low(&le)),
        };
        Number::from_f64(number)
            .map(Value::Number)
            .ok_or_else(|| Fault::NotANumber { at: at.render() })
    }

    fn length(&mut self, length: Length, at: &At) -> Result<usize, Fault> {
        let count = match length {
            Length::Prefixed { width, .. } => u64::from_le_bytes(low(&self.le(width, at)?)),
            Length::Fixed(fixed) => fixed,
        };
        let allowed = length.allows(count);
        let addressable = usize::try_from(count).ok();
        match (allowed, addressable) {
            (true, Some(count)) => Ok(count),
            _ => Err(Fault::Length {
                at: at.render(),
                length: count,
            }),
        }
    }

    fn text(&mut self, length: Length, at: &At) -> Result<Value, Fault> {
        let count = self.length(length, at)?;
        let bytes = self.take(count, at)?;
        let text = std::str::from_utf8(bytes).map_err(|_| Fault::NotUtf8 { at: at.render() })?;
        Ok(Value::String(text.to_owned()))
    }

    fn bytes(&mut self, length: Length, at: &At) -> Result<Value, Fault> {
        let count = self.length(length, at)?;
        let bytes = self.take(count, at)?;
        Ok(Value::String(abi::hex(bytes)))
    }

    fn option(&mut self, inner: &'s str, at: &At) -> Result<Value, Fault> {
        let present = match self.byte(at)? {
            0 => false,
            1 => true,
            tag => {
                return Err(Fault::Tag {
                    at: at.render(),
                    tag: u64::from(tag),
                });
            }
        };
        if !present {
            return Ok(Value::Null);
        }
        let value = self.value(inner, at)?;
        let nested = kind(self.schema, inner)?.nullable();
        match nested {
            true => Ok(json!({ "some": value })),
            false => Ok(value),
        }
    }

    fn seq(&mut self, length: Length, elements: &'s str, at: &At) -> Result<Value, Fault> {
        let count = self.length(length, at)?;
        let mut items = Vec::new();
        for index in 0..count {
            items.push(self.value(elements, &at.index(index))?);
        }
        Ok(Value::Array(items))
    }

    fn tuple(&mut self, elements: &'s [Declaration], at: &At) -> Result<Value, Fault> {
        let mut items = Vec::with_capacity(elements.len());
        for (index, element) in elements.iter().enumerate() {
            items.push(self.value(element, &at.index(index))?);
        }
        Ok(Value::Array(items))
    }

    fn fields(&mut self, fields: &'s Fields, at: &At) -> Result<Value, Fault> {
        match fields {
            Fields::NamedFields(named) => {
                let mut object = Map::new();
                for (name, declaration) in named {
                    let field = self.value(declaration, &at.field(name))?;
                    object.insert(name.clone(), field);
                }
                Ok(Value::Object(object))
            }
            Fields::UnnamedFields(unnamed) => match unnamed.as_slice() {
                [only] => self.value(only, at),
                _ => self.tuple(unnamed, at),
            },
            Fields::Empty => Ok(Value::Null),
        }
    }

    fn variant(&mut self, tag_width: u8, variants: &'s Variants, at: &At) -> Result<Value, Fault> {
        let tag = u64::from_le_bytes(low(&self.le(tag_width, at)?));
        let Some((_, name, declaration)) = variants
            .iter()
            .find(|(discriminant, _, _)| u64::try_from(*discriminant).ok() == Some(tag))
        else {
            return Err(Fault::Tag {
                at: at.render(),
                tag,
            });
        };
        let written = spelled(name);
        let fields = payload(self.schema, declaration);
        if unit_variant(fields) {
            return Ok(Value::String(written));
        }
        let here = at.field(&written);
        let body = match fields {
            Some(fields) => self.fields(fields, &here)?,
            None => self.value(declaration, &here)?,
        };
        let mut object = Map::new();
        object.insert(written, body);
        Ok(Value::Object(object))
    }
}

fn low(le: &[u8; 16]) -> [u8; 8] {
    let mut low = [0u8; 8];
    low.copy_from_slice(&le[..8]);
    low
}
