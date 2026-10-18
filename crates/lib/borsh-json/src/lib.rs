//! Values of any program as JSON, by the borsh schema its code carries: JSON to borsh bytes, borsh
//! bytes to JSON, and the shape of each type as JSON a model follows. harness's `network_*` tools
//! and chat's widgets use it.
mod decode;
mod encode;
mod shape;
#[cfg(test)]
mod tests;

use std::fmt;
use std::ops::RangeInclusive;

use borsh::schema::{BorshSchemaContainer, Declaration, Definition, Fields};

pub use decode::decode;
pub use encode::encode;
pub use shape::shape;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    Mismatch { at: String, expected: String },
    UnknownVariant { at: String, variant: String },
    UnknownField { at: String, field: String },
    MissingField { at: String, field: String },
    OutOfRange { at: String, declaration: String },
    Length { at: String, length: u64 },
    Truncated { at: String },
    Trailing { left: usize },
    Tag { at: String, tag: u64 },
    NotUtf8 { at: String },
    NotANumber { at: String },
    Undefined { declaration: String },
    Unsupported { declaration: String },
    Several(Vec<Fault>),
}

impl Fault {
    // What a value's parts refused, together: nothing, one fault, or each of several in order.
    fn all(faults: Vec<Fault>) -> Result<(), Fault> {
        let mut flat = Vec::new();
        for fault in faults {
            match fault {
                Fault::Several(inner) => flat.extend(inner),
                fault => flat.push(fault),
            }
        }
        match flat.len() {
            0 => Ok(()),
            1 => Err(flat.remove(0)),
            _ => Err(Fault::Several(flat)),
        }
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fault::Mismatch { at, expected } => write!(f, "at {at}: expected {expected}"),
            Fault::UnknownVariant { at, variant } => {
                write!(f, "at {at}: there is no variant named {variant}")
            }
            Fault::UnknownField { at, field } => write!(f, "at {at}: there is no field {field}"),
            Fault::MissingField { at, field } => write!(f, "at {at}: field {field} is missing"),
            Fault::OutOfRange { at, declaration } => {
                write!(f, "at {at}: the number does not fit {declaration}")
            }
            Fault::Length { at, length } => {
                write!(
                    f,
                    "at {at}: a length of {length} is not one this type allows"
                )
            }
            Fault::Truncated { at } => write!(f, "at {at}: the bytes end early"),
            Fault::Trailing { left } => write!(f, "{left} bytes are left after the value"),
            Fault::Tag { at, tag } => write!(f, "at {at}: no variant has tag {tag}"),
            Fault::NotUtf8 { at } => write!(f, "at {at}: the string is not UTF-8"),
            Fault::NotANumber { at } => write!(f, "at {at}: the float is not a number"),
            Fault::Undefined { declaration } => {
                write!(f, "the schema does not define {declaration}")
            }
            Fault::Unsupported { declaration } => {
                write!(f, "the schema's {declaration} has no JSON form")
            }
            Fault::Several(faults) => {
                let each: Vec<String> = faults.iter().map(Fault::to_string).collect();
                write!(f, "{} problems: {}", faults.len(), each.join("; "))
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Step<'a> {
    Root(&'a str),
    Field(&'a str),
    Index(usize),
}

#[derive(Clone, Copy)]
struct At<'a> {
    up: Option<&'a At<'a>>,
    step: Step<'a>,
}

impl<'a> At<'a> {
    fn root(name: &'a str) -> At<'a> {
        At {
            up: None,
            step: Step::Root(name),
        }
    }

    fn field(&'a self, name: &'a str) -> At<'a> {
        At {
            up: Some(self),
            step: Step::Field(name),
        }
    }

    fn index(&'a self, index: usize) -> At<'a> {
        At {
            up: Some(self),
            step: Step::Index(index),
        }
    }

    fn render(&self) -> String {
        let here = match self.step {
            Step::Root(name) => name.to_owned(),
            Step::Field(name) => format!(".{name}"),
            Step::Index(index) => format!("[{index}]"),
        };
        match self.up {
            Some(up) => up.render() + &here,
            None => here,
        }
    }

    fn mismatch(&self, expected: impl Into<String>) -> Fault {
        Fault::Mismatch {
            at: self.render(),
            expected: expected.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Length<'a> {
    Prefixed {
        width: u8,
        range: &'a RangeInclusive<u64>,
    },
    Fixed(u64),
}

impl Length<'_> {
    fn allows(&self, length: u64) -> bool {
        match self {
            Length::Prefixed { width, range } => {
                let fits_width = *width >= 8 || length < 1u64 << (u32::from(*width) * 8);
                fits_width && range.contains(&length)
            }
            Length::Fixed(fixed) => length == *fixed,
        }
    }
}

type Variants = [(i64, String, Declaration)];

#[derive(Clone, Copy, Debug)]
enum Kind<'a> {
    Unit,
    Bool,
    Int {
        bytes: u8,
        signed: bool,
    },
    Float {
        bytes: u8,
    },
    Text(Length<'a>),
    Bytes(Length<'a>),
    Option(&'a str),
    Seq {
        length: Length<'a>,
        elements: &'a str,
    },
    Tuple(&'a [Declaration]),
    Struct(&'a Fields),
    Enum {
        tag_width: u8,
        variants: &'a Variants,
    },
}

impl Kind<'_> {
    fn nullable(&self) -> bool {
        matches!(self, Kind::Unit | Kind::Option(_))
    }
}

fn kind<'a>(schema: &'a BorshSchemaContainer, declaration: &'a str) -> Result<Kind<'a>, Fault> {
    let definition = schema
        .get_definition(declaration)
        .ok_or_else(|| Fault::Undefined {
            declaration: declaration.to_owned(),
        })?;
    match definition {
        Definition::Primitive(size) => primitive(declaration, *size),
        Definition::Sequence {
            length_width,
            length_range,
            elements,
        } => sequence(schema, declaration, *length_width, length_range, elements),
        Definition::Tuple { elements } => Ok(Kind::Tuple(elements)),
        Definition::Enum {
            tag_width,
            variants,
        } => Ok(enumeration(*tag_width, variants)),
        Definition::Struct { fields } => Ok(Kind::Struct(fields)),
    }
}

fn primitive(declaration: &str, size: u8) -> Result<Kind<'static>, Fault> {
    let kind = match declaration {
        "()" => Kind::Unit,
        "bool" => Kind::Bool,
        "u8" | "u16" | "u32" | "u64" | "u128" => Kind::Int {
            bytes: size,
            signed: false,
        },
        "i8" | "i16" | "i32" | "i64" | "i128" => Kind::Int {
            bytes: size,
            signed: true,
        },
        "f32" | "f64" => Kind::Float { bytes: size },
        _ => {
            return Err(Fault::Unsupported {
                declaration: declaration.to_owned(),
            });
        }
    };
    let sized = match kind {
        Kind::Unit => size == 0,
        Kind::Bool => size == 1,
        Kind::Int { bytes, .. } => matches!(bytes, 1 | 2 | 4 | 8 | 16),
        Kind::Float { bytes } => matches!(bytes, 4 | 8),
        _ => false,
    };
    if !sized {
        return Err(Fault::Unsupported {
            declaration: declaration.to_owned(),
        });
    }
    Ok(kind)
}

fn sequence<'a>(
    schema: &'a BorshSchemaContainer,
    declaration: &'a str,
    width: u8,
    range: &'a RangeInclusive<u64>,
    elements: &'a str,
) -> Result<Kind<'a>, Fault> {
    let fixed = width == 0 && range.start() == range.end();
    let length = match (width, fixed) {
        (0, true) => Length::Fixed(*range.start()),
        (0, false) => {
            return Err(Fault::Unsupported {
                declaration: declaration.to_owned(),
            });
        }
        (width, _) => Length::Prefixed { width, range },
    };
    let of_bytes = elements == "u8"
        && matches!(
            schema.get_definition(elements),
            Some(Definition::Primitive(1))
        );
    let text = declaration == "String" && of_bytes;
    let kind = match (text, of_bytes) {
        (true, _) => Kind::Text(length),
        (false, true) => Kind::Bytes(length),
        (false, false) => Kind::Seq { length, elements },
    };
    Ok(kind)
}

fn enumeration(tag_width: u8, variants: &Variants) -> Kind<'_> {
    let option = match variants {
        [(0, none, unit), (1, some, inner)] if none == "None" && unit == "()" && some == "Some" => {
            Some(inner)
        }
        _ => None,
    };
    match (tag_width, option) {
        (1, Some(inner)) => Kind::Option(inner),
        _ => Kind::Enum {
            tag_width,
            variants,
        },
    }
}

fn payload<'a>(schema: &'a BorshSchemaContainer, variant: &'a str) -> Option<&'a Fields> {
    match schema.get_definition(variant) {
        Some(Definition::Struct { fields }) => Some(fields),
        _ => None,
    }
}

fn unit_variant(fields: Option<&Fields>) -> bool {
    matches!(fields, Some(Fields::Empty))
}

// The variant of enum `declaration` whose tag `bytes` start with.
pub fn variant(schema: &BorshSchemaContainer, declaration: &str, bytes: &[u8]) -> Option<String> {
    let Ok(Kind::Enum {
        tag_width,
        variants,
    }) = kind(schema, declaration)
    else {
        return None;
    };
    let width = usize::from(tag_width);
    let mut le = [0u8; 8];
    le.get_mut(..width)?.copy_from_slice(bytes.get(..width)?);
    let tag = i64::from_le_bytes(le);
    variants
        .iter()
        .find(|(discriminant, _, _)| *discriminant == tag)
        .map(|(_, name, _)| name.clone())
}
