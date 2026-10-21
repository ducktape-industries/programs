//! A working value of any type of a schema, for a caller to edit rather than build from its shape:
//! each enum's first variant that holds no value of itself, one element where a list may hold one,
//! zero, empty text and null.
use borsh::schema::{BorshSchemaContainer, Declaration, Fields};
use serde_json::{Map, Value, json};

use super::{Fault, Kind, Length, Variants, kind, named, payload, spelled, unit_variant};

pub fn example<'s>(schema: &'s BorshSchemaContainer, declaration: &'s str) -> Result<Value, Fault> {
    let mut maker = Maker {
        schema,
        open: Vec::new(),
    };
    maker
        .value(declaration)
        .map_err(|stop| stop.fault(declaration))
}

// Each variant of enum `declaration`, in the schema's order, with a working value of it; none for a
// type that is no enum.
pub fn examples<'s>(
    schema: &'s BorshSchemaContainer,
    declaration: &'s str,
) -> Result<Vec<(String, Value)>, Fault> {
    let Kind::Enum { variants, .. } = kind(schema, declaration)? else {
        return Ok(Vec::new());
    };
    let mut maker = Maker {
        schema,
        open: vec![declaration],
    };
    let mut made = Vec::with_capacity(variants.len());
    for (_, name, body) in variants {
        let value = maker.variant(name, body).map_err(|stop| stop.fault(body))?;
        made.push((name.clone(), value));
    }
    Ok(made)
}

// Variant `name` of enum `declaration` with a working value of it; none for a type that is no
// enum or has no such variant.
pub fn variant_example<'s>(
    schema: &'s BorshSchemaContainer,
    declaration: &'s str,
    name: &str,
) -> Result<Option<Value>, Fault> {
    let Kind::Enum { variants, .. } = kind(schema, declaration)? else {
        return Ok(None);
    };
    let Some((_, variant, body)) = named(variants, name) else {
        return Ok(None);
    };
    let mut maker = Maker {
        schema,
        open: vec![declaration],
    };
    let made = maker
        .variant(variant, body)
        .map_err(|stop| stop.fault(body))?;
    Ok(Some(made))
}

enum Stop {
    Fault(Fault),
    Cycle,
}

impl Stop {
    fn fault(self, declaration: &str) -> Fault {
        match self {
            Stop::Fault(fault) => fault,
            Stop::Cycle => Fault::Unsupported {
                declaration: declaration.to_owned(),
            },
        }
    }
}

impl From<Fault> for Stop {
    fn from(fault: Fault) -> Stop {
        Stop::Fault(fault)
    }
}

struct Maker<'s> {
    schema: &'s BorshSchemaContainer,
    open: Vec<&'s str>,
}

impl<'s> Maker<'s> {
    fn value(&mut self, declaration: &'s str) -> Result<Value, Stop> {
        if self.open.contains(&declaration) {
            return Err(Stop::Cycle);
        }
        let kind = kind(self.schema, declaration)?;
        self.open.push(declaration);
        let made = self.made(kind);
        self.open.pop();
        made
    }

    fn made(&mut self, kind: Kind<'s>) -> Result<Value, Stop> {
        match kind {
            Kind::Unit | Kind::Option(_) => Ok(Value::Null),
            Kind::Bool => Ok(json!(false)),
            Kind::Int { bytes: 16, .. } => Ok(json!("0")),
            Kind::Int { .. } => Ok(json!(0)),
            Kind::Float { .. } => Ok(json!(0.0)),
            Kind::Text(_) => Ok(json!("")),
            Kind::Bytes(length) => Ok(json!("00".repeat(least(length)))),
            Kind::Seq { length, elements } => self.seq(length, elements),
            Kind::Tuple(elements) => self.tuple(elements),
            Kind::Struct(fields) => self.fields(fields),
            Kind::Enum { variants, .. } => self.first(variants),
        }
    }

    fn seq(&mut self, length: Length, elements: &'s str) -> Result<Value, Stop> {
        let least = least(length);
        let wanted = match length {
            Length::Fixed(_) => least,
            Length::Prefixed { range, .. } => least.max(1).min(*range.end() as usize),
        };
        if wanted == 0 {
            return Ok(json!([]));
        }
        let item = match self.value(elements) {
            Ok(item) => item,
            Err(Stop::Cycle) if least == 0 => return Ok(json!([])),
            Err(stop) => return Err(stop),
        };
        Ok(Value::Array(vec![item; wanted]))
    }

    fn tuple(&mut self, elements: &'s [Declaration]) -> Result<Value, Stop> {
        let mut items = Vec::with_capacity(elements.len());
        for element in elements {
            items.push(self.value(element)?);
        }
        Ok(Value::Array(items))
    }

    fn fields(&mut self, fields: &'s Fields) -> Result<Value, Stop> {
        match fields {
            Fields::NamedFields(named) => {
                let mut object = Map::new();
                for (name, declaration) in named {
                    object.insert(name.clone(), self.value(declaration)?);
                }
                Ok(Value::Object(object))
            }
            Fields::UnnamedFields(unnamed) => match unnamed.as_slice() {
                [only] => self.value(only),
                _ => self.tuple(unnamed),
            },
            Fields::Empty => Ok(Value::Null),
        }
    }

    fn first(&mut self, variants: &'s Variants) -> Result<Value, Stop> {
        for (_, name, body) in variants {
            match self.variant(name, body) {
                Err(Stop::Cycle) => continue,
                made => return made,
            }
        }
        Err(Stop::Cycle)
    }

    fn variant(&mut self, name: &str, body: &'s str) -> Result<Value, Stop> {
        let fields = payload(self.schema, body);
        if unit_variant(fields) {
            return Ok(json!(spelled(name)));
        }
        let made = match fields {
            Some(fields) => self.fields(fields)?,
            None => self.value(body)?,
        };
        let mut object = Map::new();
        object.insert(spelled(name), made);
        Ok(Value::Object(object))
    }
}

fn least(length: Length) -> usize {
    match length {
        Length::Fixed(fixed) => fixed as usize,
        Length::Prefixed { range, .. } => *range.start() as usize,
    }
}
