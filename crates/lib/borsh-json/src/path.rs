//! The type a dotted path reaches in the JSON `decode` writes: a variant or a field by either
//! spelling, an element by its index, an option and a one-field struct looked through.
use borsh::schema::{BorshSchemaContainer, Declaration, Fields};

use super::{At, Fault, Kind, kind, named, payload, spelled, spelled_all};

pub fn declared_at(
    schema: &BorshSchemaContainer,
    declaration: &str,
    name: &str,
    path: &str,
) -> Result<Declaration, Fault> {
    let steps: Vec<&str> = match path {
        "" => Vec::new(),
        path => path.split('.').collect(),
    };
    reach(schema, declaration, &steps, &At::root(name))
}

fn reach(
    schema: &BorshSchemaContainer,
    declaration: &str,
    steps: &[&str],
    at: &At,
) -> Result<Declaration, Fault> {
    let Some((step, rest)) = steps.split_first() else {
        return Ok(declaration.to_owned());
    };
    match kind(schema, declaration)? {
        Kind::Option(inner) if kind(schema, inner)?.nullable() => {
            if *step != "some" {
                return Err(Fault::UnknownField {
                    at: at.render(),
                    field: (*step).to_owned(),
                    known: vec!["some".to_owned()],
                });
            }
            reach(schema, inner, rest, &at.field("some"))
        }
        Kind::Option(inner) => reach(schema, inner, steps, at),
        Kind::Struct(fields) => within(schema, fields, declaration, steps, at),
        Kind::Tuple(elements) => element(schema, elements, declaration, step, rest, at),
        Kind::Seq { elements, .. } => {
            let index = index(step, at)?;
            reach(schema, elements, rest, &at.index(index))
        }
        Kind::Enum { variants, .. } => {
            let (_, variant, of) = named(variants, step).ok_or_else(|| Fault::UnknownVariant {
                at: at.render(),
                variant: (*step).to_owned(),
                known: spelled_all(variants),
            })?;
            let written = spelled(variant);
            let here = at.field(&written);
            match payload(schema, of) {
                Some(fields) => within(schema, fields, of, rest, &here),
                None => reach(schema, of, rest, &here),
            }
        }
        _ => Err(no_part(at, step, declaration)),
    }
}

fn within(
    schema: &BorshSchemaContainer,
    fields: &Fields,
    declaration: &str,
    steps: &[&str],
    at: &At,
) -> Result<Declaration, Fault> {
    let Some((step, rest)) = steps.split_first() else {
        return Ok(declaration.to_owned());
    };
    match fields {
        Fields::NamedFields(listed) => {
            let spelled_step = spelled(step);
            let found = listed
                .iter()
                .find(|(name, _)| name.as_str() == *step || *name == spelled_step);
            let Some((name, of)) = found else {
                return Err(Fault::UnknownField {
                    at: at.render(),
                    field: (*step).to_owned(),
                    known: listed.iter().map(|(name, _)| name.clone()).collect(),
                });
            };
            reach(schema, of, rest, &at.field(name))
        }
        Fields::UnnamedFields(unnamed) => match unnamed.as_slice() {
            [only] => reach(schema, only, steps, at),
            elements => element(schema, elements, declaration, step, rest, at),
        },
        Fields::Empty => Err(no_part(at, step, declaration)),
    }
}

fn element(
    schema: &BorshSchemaContainer,
    elements: &[Declaration],
    declaration: &str,
    step: &str,
    rest: &[&str],
    at: &At,
) -> Result<Declaration, Fault> {
    let index = index(step, at)?;
    let element = elements
        .get(index)
        .ok_or_else(|| no_part(at, step, declaration))?;
    reach(schema, element, rest, &at.index(index))
}

fn index(step: &str, at: &At) -> Result<usize, Fault> {
    step.parse().map_err(|_| Fault::Mismatch {
        at: at.render(),
        expected: "an index".to_owned(),
        given: Some(format!("the step {step}")),
    })
}

fn no_part(at: &At, step: &str, declaration: &str) -> Fault {
    Fault::Mismatch {
        at: at.render(),
        expected: format!("a part {step}"),
        given: Some(format!("{declaration}, which has none")),
    }
}
