//! A schema's types as the JSON a value of each is written in: an expression per root, and each
//! struct and enum they reach under `types`, by name.
use std::collections::{BTreeMap, BTreeSet};

use borsh::schema::{BorshSchemaContainer, Fields};
use serde_json::{Map, Value, json};

use super::{Fault, Kind, Length, Variants, kind, payload, unit_variant};

// Under `returns` (when given), each op that names what it returns, and that type's shape.
pub(crate) fn shape(
    schema: &BorshSchemaContainer,
    roots: &[(&str, &str)],
    returns: Option<&[(&str, &str)]>,
) -> Result<Value, Fault> {
    let mut shaper = Shaper {
        schema,
        named: BTreeMap::new(),
        pending: Vec::new(),
        inlining: BTreeSet::new(),
    };
    let mut out = Map::new();
    for (name, declaration) in roots {
        out.insert((*name).to_owned(), shaper.expr(declaration)?);
    }
    if let Some(returns) = returns {
        let mut returned = Map::new();
        for (op, declaration) in returns {
            returned.insert((*op).to_owned(), shaper.expr(declaration)?);
        }
        out.insert("returns".to_owned(), Value::Object(returned));
    }
    while let Some(declaration) = shaper.pending.pop() {
        let defined = shaper.definition(declaration)?;
        shaper.named.insert(declaration.to_owned(), defined);
    }
    let types: Map<String, Value> = shaper.named.into_iter().collect();
    out.insert("types".to_owned(), Value::Object(types));
    Ok(Value::Object(out))
}

struct Shaper<'s> {
    schema: &'s BorshSchemaContainer,
    named: BTreeMap<String, Value>,
    pending: Vec<&'s str>,
    inlining: BTreeSet<&'s str>,
}

impl<'s> Shaper<'s> {
    fn expr(&mut self, declaration: &'s str) -> Result<Value, Fault> {
        let kind = kind(self.schema, declaration)?;
        let by_name = matches!(kind, Kind::Struct(_) | Kind::Enum { .. })
            || self.inlining.contains(declaration);
        if by_name {
            self.name(declaration);
            return Ok(json!(declaration));
        }
        self.inlining.insert(declaration);
        let inline = self.inline(declaration, kind);
        self.inlining.remove(declaration);
        inline
    }

    fn name(&mut self, declaration: &'s str) {
        if !self.named.contains_key(declaration) {
            self.named.insert(declaration.to_owned(), Value::Null);
            self.pending.push(declaration);
        }
    }

    fn definition(&mut self, declaration: &'s str) -> Result<Value, Fault> {
        let kind = kind(self.schema, declaration)?;
        self.inlining.insert(declaration);
        let defined = self.inline(declaration, kind);
        self.inlining.remove(declaration);
        defined
    }

    fn inline(&mut self, declaration: &'s str, kind: Kind<'s>) -> Result<Value, Fault> {
        match kind {
            Kind::Unit => Ok(json!("null")),
            Kind::Bool => Ok(json!("bool")),
            Kind::Int { .. } | Kind::Float { .. } => Ok(json!(declaration)),
            Kind::Text(_) => Ok(json!("string")),
            Kind::Bytes(Length::Fixed(length)) => Ok(json!({ "hex": length })),
            Kind::Bytes(Length::Prefixed { .. }) => Ok(json!("hex")),
            Kind::Option(inner) => Ok(json!({ "option": self.expr(inner)? })),
            Kind::Seq { length, elements } => self.seq(length, elements),
            Kind::Tuple(elements) => self.tuple(elements),
            Kind::Struct(fields) => self.fields(fields),
            Kind::Enum { variants, .. } => self.variants(variants),
        }
    }

    fn seq(&mut self, length: Length, elements: &'s str) -> Result<Value, Fault> {
        let element = self.expr(elements)?;
        match length {
            Length::Fixed(length) => Ok(json!({ "vec": element, "len": length })),
            Length::Prefixed { .. } => Ok(json!({ "vec": element })),
        }
    }

    fn tuple(&mut self, elements: &'s [String]) -> Result<Value, Fault> {
        let items = elements
            .iter()
            .map(|element| self.expr(element))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(json!({ "tuple": items }))
    }

    fn fields(&mut self, fields: &'s Fields) -> Result<Value, Fault> {
        match fields {
            Fields::NamedFields(named) => {
                let mut object = Map::new();
                for (name, declaration) in named {
                    object.insert(name.clone(), self.expr(declaration)?);
                }
                Ok(json!({ "struct": object }))
            }
            Fields::UnnamedFields(unnamed) => match unnamed.as_slice() {
                [only] => self.expr(only),
                _ => self.tuple(unnamed),
            },
            Fields::Empty => Ok(json!("null")),
        }
    }

    fn variants(&mut self, variants: &'s Variants) -> Result<Value, Fault> {
        let mut object = Map::new();
        for (_, name, declaration) in variants {
            let fields = payload(self.schema, declaration);
            let body = match (unit_variant(fields), fields) {
                (true, _) => Value::Null,
                (false, Some(fields)) => self.fields(fields)?,
                (false, None) => self.expr(declaration)?,
            };
            object.insert(name.clone(), body);
        }
        Ok(json!({ "enum": object }))
    }
}
