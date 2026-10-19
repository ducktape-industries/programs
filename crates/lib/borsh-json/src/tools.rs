//! A program's `ducktape.tools` section read back, and held to its interface: every op and query
//! described once, each saying something, and each example a value of the variant it describes.
use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::{Interface, Kind, decode, encode, kind, variant};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Described {
    pub about: String,
    pub ops: BTreeMap<String, Told>,
    pub queries: BTreeMap<String, Told>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Told {
    pub says: String,
    pub example: Option<Value>,
}

// The section's JSON, keys it does not know left alone so a section can grow.
pub fn read(section: &[u8]) -> Result<Described, String> {
    let value: Value = serde_json::from_slice(section)
        .map_err(|error| format!("the tools section is no JSON: {error}"))?;
    let object = value
        .as_object()
        .ok_or("the tools section is no JSON object")?;
    let about = match object.get("about") {
        None => String::new(),
        Some(Value::String(about)) => about.clone(),
        Some(_) => return Err("the tools section's about is no string".into()),
    };
    Ok(Described {
        about,
        ops: told(object, "ops")?,
        queries: told(object, "queries")?,
    })
}

fn told(object: &Map<String, Value>, key: &str) -> Result<BTreeMap<String, Told>, String> {
    let Some(group) = object.get(key) else {
        return Ok(BTreeMap::new());
    };
    let group = group
        .as_object()
        .ok_or_else(|| format!("the tools section's {key} is no object"))?;
    let mut read = BTreeMap::new();
    for (name, entry) in group {
        let says = entry
            .get("says")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("the tools section's {key}.{name} says no string"))?;
        let told = Told {
            says: says.to_owned(),
            example: entry.get("example").cloned(),
        };
        read.insert(name.clone(), told);
    }
    Ok(read)
}

// What is wrong with how a section describes `interface`, each problem a sentence; none when every
// op and query is described, says something, and has an example only of itself.
pub fn check(interface: &Interface, described: &Described) -> Vec<String> {
    let mut problems = Vec::new();
    let any = !(described.ops.is_empty() && described.queries.is_empty());
    if any && described.about.trim().is_empty() {
        problems.push("about says nothing of what the program is for".to_owned());
    }
    let groups = [
        ("op", interface.op.as_str(), &described.ops),
        ("query", interface.query.as_str(), &described.queries),
    ];
    for (root, declaration, told) in groups {
        let variants = names(interface, declaration);
        for name in &variants {
            if !told.contains_key(name) {
                problems.push(format!("{root} {name} is not described"));
            }
        }
        for (name, told) in told {
            if !variants.contains(name) {
                problems.push(format!(
                    "{root} {name} is described, and the {root} has no variant {name}"
                ));
                continue;
            }
            if told.says.trim().is_empty() {
                problems.push(format!("{root} {name} says nothing"));
            }
            if let Some(example) = &told.example {
                problems.extend(written(interface, declaration, root, name, example));
            }
        }
    }
    problems
}

fn names(interface: &Interface, declaration: &str) -> Vec<String> {
    match kind(&interface.schema, declaration) {
        Ok(Kind::Enum { variants, .. }) => {
            variants.iter().map(|(_, name, _)| name.clone()).collect()
        }
        _ => Vec::new(),
    }
}

fn written(
    interface: &Interface,
    declaration: &str,
    root: &str,
    name: &str,
    example: &Value,
) -> Option<String> {
    let bytes = match encode(&interface.schema, declaration, root, example) {
        Ok(bytes) => bytes,
        Err(fault) => return Some(format!("{root} {name}'s example does not encode: {fault}")),
    };
    let encoded = variant(&interface.schema, declaration, &bytes);
    if encoded.as_deref() != Some(name) {
        let other = encoded.unwrap_or_default();
        return Some(format!("{root} {name}'s example is a {other}"));
    }
    match decode(&interface.schema, declaration, root, &bytes) {
        Ok(read) if read == *example => None,
        Ok(read) => Some(format!(
            "{root} {name}'s example reads back as {read}: write it so"
        )),
        Err(fault) => Some(format!(
            "{root} {name}'s example does not read back: {fault}"
        )),
    }
}
