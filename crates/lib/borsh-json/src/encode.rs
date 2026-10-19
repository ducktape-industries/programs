//! JSON to borsh bytes by a schema: the bytes the type's own `BorshSerialize` writes.
use borsh::schema::{BorshSchemaContainer, Declaration, Fields};
use serde_json::{Map, Value};

use super::{At, Fault, Kind, Length, Variants, kind, payload, unit_variant};

pub fn encode(
    schema: &BorshSchemaContainer,
    declaration: &str,
    name: &str,
    value: &Value,
) -> Result<Vec<u8>, Fault> {
    let mut writer = Writer {
        schema,
        out: Vec::new(),
    };
    writer.value(declaration, value, &At::root(name))?;
    Ok(writer.out)
}

struct Writer<'s> {
    schema: &'s BorshSchemaContainer,
    out: Vec<u8>,
}

impl<'s> Writer<'s> {
    fn value(&mut self, declaration: &'s str, value: &Value, at: &At) -> Result<(), Fault> {
        match kind(self.schema, declaration)? {
            Kind::Unit => unit(value, at),
            Kind::Bool => self.boolean(value, at),
            Kind::Int { bytes, signed } => self.int(declaration, bytes, signed, value, at),
            Kind::Float { bytes } => self.float(bytes, value, at),
            Kind::Text(length) => self.text(length, value, at),
            Kind::Bytes(length) => self.bytes(length, value, at),
            Kind::Option(inner) => self.option(inner, value, at),
            Kind::Seq { length, elements } => self.seq(length, elements, value, at),
            Kind::Tuple(elements) => self.tuple(elements, value, at),
            Kind::Struct(fields) => self.fields(fields, value, at),
            Kind::Enum {
                tag_width,
                variants,
            } => self.variant(tag_width, variants, value, at),
        }
    }

    fn boolean(&mut self, value: &Value, at: &At) -> Result<(), Fault> {
        let flag = value
            .as_bool()
            .ok_or_else(|| got(at, value, "true or false"))?;
        self.out.push(u8::from(flag));
        Ok(())
    }

    fn int(
        &mut self,
        declaration: &str,
        bytes: u8,
        signed: bool,
        value: &Value,
        at: &At,
    ) -> Result<(), Fault> {
        let out_of_range = || Fault::OutOfRange {
            at: at.render(),
            declaration: declaration.to_owned(),
        };
        let width = usize::from(bytes);
        let bits = u32::from(bytes) * 8;
        let number = integer(value).ok_or_else(|| got(at, value, "an integer"))?;
        let le = match (signed, number) {
            (true, Integer::Negative(number)) => {
                let fits = bits >= 128 || number >= -(1i128 << (bits - 1));
                fits.then(|| number.to_le_bytes())
            }
            (true, Integer::Natural(number)) => {
                let limit = 1u128 << (bits - 1);
                (number < limit).then(|| number.to_le_bytes())
            }
            (false, Integer::Negative(_)) => None,
            (false, Integer::Natural(number)) => {
                let fits = bits >= 128 || number < 1u128 << bits;
                fits.then(|| number.to_le_bytes())
            }
        }
        .ok_or_else(out_of_range)?;
        self.out.extend_from_slice(&le[..width]);
        Ok(())
    }

    fn float(&mut self, bytes: u8, value: &Value, at: &At) -> Result<(), Fault> {
        let number = value.as_f64().ok_or_else(|| got(at, value, "a number"))?;
        match bytes {
            4 => self.out.extend_from_slice(&(number as f32).to_le_bytes()),
            _ => self.out.extend_from_slice(&number.to_le_bytes()),
        }
        Ok(())
    }

    fn text(&mut self, length: Length, value: &Value, at: &At) -> Result<(), Fault> {
        let text = value.as_str().ok_or_else(|| got(at, value, "a string"))?;
        self.length(length, text.len(), at)?;
        self.out.extend_from_slice(text.as_bytes());
        Ok(())
    }

    fn bytes(&mut self, length: Length, value: &Value, at: &At) -> Result<(), Fault> {
        let bytes = match single(value, "utf8") {
            Some(text) => text.as_str().map(|text| text.as_bytes().to_vec()),
            None => value.as_str().and_then(design::unhex),
        }
        .ok_or_else(|| got(at, value, "bytes as hex, or {\"utf8\": text}"))?;
        self.length(length, bytes.len(), at)?;
        self.out.extend_from_slice(&bytes);
        Ok(())
    }

    fn option(&mut self, inner: &'s str, value: &Value, at: &At) -> Result<(), Fault> {
        if value.is_null() {
            self.out.push(0);
            return Ok(());
        }
        self.out.push(1);
        let nested = kind(self.schema, inner)?.nullable();
        let some = match nested {
            true => {
                single(value, "Some").ok_or_else(|| got(at, value, "null or {\"Some\": value}"))?
            }
            false => value,
        };
        self.value(inner, some, at)
    }

    fn seq(
        &mut self,
        length: Length,
        elements: &'s str,
        value: &Value,
        at: &At,
    ) -> Result<(), Fault> {
        let items = value.as_array().ok_or_else(|| got(at, value, "an array"))?;
        self.length(length, items.len(), at)?;
        let mut faults = Vec::new();
        for (index, item) in items.iter().enumerate() {
            faults.extend(self.value(elements, item, &at.index(index)).err());
        }
        Fault::all(faults)
    }

    fn tuple(&mut self, elements: &'s [Declaration], value: &Value, at: &At) -> Result<(), Fault> {
        let items = value
            .as_array()
            .filter(|items| items.len() == elements.len())
            .ok_or_else(|| got(at, value, format!("an array of {}", elements.len())))?;
        let mut faults = Vec::new();
        for (index, (element, item)) in elements.iter().zip(items).enumerate() {
            faults.extend(self.value(element, item, &at.index(index)).err());
        }
        Fault::all(faults)
    }

    fn fields(&mut self, fields: &'s Fields, value: &Value, at: &At) -> Result<(), Fault> {
        match fields {
            Fields::NamedFields(named) => self.named(named, value, at),
            Fields::UnnamedFields(unnamed) => match unnamed.as_slice() {
                [only] => self.value(only, value, at),
                _ => self.tuple(unnamed, value, at),
            },
            Fields::Empty => empty(value, at),
        }
    }

    fn named(
        &mut self,
        named: &'s [(String, Declaration)],
        value: &Value,
        at: &At,
    ) -> Result<(), Fault> {
        let object = value
            .as_object()
            .ok_or_else(|| got(at, value, "an object"))?;
        // every field is written, so one refusal names each field that is unknown, missing or wrong
        let mut faults: Vec<Fault> = object
            .keys()
            .filter(|key| !named.iter().any(|(name, _)| name == *key))
            .map(|field| Fault::UnknownField {
                at: at.render(),
                field: field.clone(),
                known: named.iter().map(|(name, _)| name.clone()).collect(),
            })
            .collect();
        for (name, declaration) in named {
            let here = at.field(name);
            let written = match object.get(name) {
                Some(field) => self.value(declaration, field, &here),
                None => self.absent(name, declaration, &here, at),
            };
            faults.extend(written.err());
        }
        Fault::all(faults)
    }

    fn absent(
        &mut self,
        name: &str,
        declaration: &'s str,
        here: &At,
        at: &At,
    ) -> Result<(), Fault> {
        let may_be_left_out = kind(self.schema, declaration)?.nullable();
        if !may_be_left_out {
            return Err(Fault::MissingField {
                at: at.render(),
                field: name.to_owned(),
            });
        }
        self.value(declaration, &Value::Null, here)
    }

    fn variant(
        &mut self,
        tag_width: u8,
        variants: &'s Variants,
        value: &Value,
        at: &At,
    ) -> Result<(), Fault> {
        let (name, body) = match value {
            Value::String(name) => (name.as_str(), None),
            Value::Object(object) => one_key(object).ok_or_else(|| got(at, value, VARIANT))?,
            _ => return Err(got(at, value, VARIANT)),
        };
        let (tag, variant_name, declaration) = variants
            .iter()
            .find(|(_, variant, _)| variant == name)
            .ok_or_else(|| Fault::UnknownVariant {
                at: at.render(),
                variant: name.to_owned(),
                known: variants.iter().map(|(_, known, _)| known.clone()).collect(),
            })?;
        self.tag(tag_width, *tag, at)?;
        let here = at.field(variant_name);
        let fields = payload(self.schema, declaration);
        let unit = unit_variant(fields);
        match (fields, body) {
            (Some(fields), Some(body)) => self.fields(fields, body, &here),
            (None, Some(body)) => self.value(declaration, body, &here),
            (_, None) => match unit {
                true => Ok(()),
                false => Err(got(&here, value, format!("{{\"{variant_name}\": value}}"))),
            },
        }
    }

    fn tag(&mut self, tag_width: u8, tag: i64, at: &At) -> Result<(), Fault> {
        let width = usize::from(tag_width);
        let le = tag.to_le_bytes();
        let fits = width <= le.len();
        if !fits {
            return Err(at.mismatch(format!("a tag of {tag_width} bytes")));
        }
        self.out.extend_from_slice(&le[..width]);
        Ok(())
    }

    fn length(&mut self, length: Length, count: usize, at: &At) -> Result<(), Fault> {
        let count = count as u64;
        if !length.allows(count) {
            return Err(Fault::Length {
                at: at.render(),
                length: count,
            });
        }
        if let Length::Prefixed { width, .. } = length {
            self.out
                .extend_from_slice(&count.to_le_bytes()[..usize::from(width)]);
        }
        Ok(())
    }
}

// What a value that does not fit was: its kind, with a string's text, a number and an object's keys.
fn given(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => format!("the number {number}"),
        Value::String(_) => format!("the string {value}"),
        Value::Array(items) => format!("an array of length {}", items.len()),
        Value::Object(object) if object.is_empty() => "an empty object".to_owned(),
        Value::Object(object) => {
            let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
            keys.sort_unstable();
            format!("an object with keys {}", keys.join(", "))
        }
    }
}

fn got(at: &At, value: &Value, expected: impl Into<String>) -> Fault {
    Fault::Mismatch {
        at: at.render(),
        expected: expected.into(),
        given: Some(given(value)),
    }
}

const VARIANT: &str = "\"Variant\" or {\"Variant\": value}";

fn unit(value: &Value, at: &At) -> Result<(), Fault> {
    match value {
        Value::Null => Ok(()),
        _ => Err(got(at, value, "null")),
    }
}

fn empty(value: &Value, at: &At) -> Result<(), Fault> {
    let empty_object = value.as_object().is_some_and(Map::is_empty);
    match value.is_null() || empty_object {
        true => Ok(()),
        false => Err(got(at, value, "null")),
    }
}

fn one_key(object: &Map<String, Value>) -> Option<(&str, Option<&Value>)> {
    let mut entries = object.iter();
    let (name, body) = entries.next()?;
    match entries.next() {
        Some(_) => None,
        None => Some((name.as_str(), Some(body))),
    }
}

fn single<'v>(value: &'v Value, key: &str) -> Option<&'v Value> {
    let object = value.as_object()?;
    match one_key(object)? {
        (name, Some(body)) if name == key => Some(body),
        _ => None,
    }
}

enum Integer {
    Negative(i128),
    Natural(u128),
}

fn integer(value: &Value) -> Option<Integer> {
    match value {
        Value::Number(number) => match (number.as_u64(), number.as_i64()) {
            (Some(natural), _) => Some(Integer::Natural(u128::from(natural))),
            (None, Some(negative)) => Some(Integer::Negative(i128::from(negative))),
            (None, None) => None,
        },
        Value::String(text) => match text.parse::<u128>() {
            Ok(natural) => Some(Integer::Natural(natural)),
            Err(_) => text.parse::<i128>().ok().map(Integer::Negative),
        },
        _ => None,
    }
}
