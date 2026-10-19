//! A program's `ducktape.tools` section: written from what its enums tell (`program::tools`), each
//! example as JSON and each response's shape from the schema; read back; held to its interface.
use std::collections::BTreeMap;

use program::tools::{Tool, Tools};
use serde_json::{Map, Value, json};

use crate::{Interface, decode, encode, examples, shape, variant, variant_shape, variants};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Described {
    pub about: String,
    pub ops: BTreeMap<String, Told>,
    pub queries: BTreeMap<String, Told>,
}

// What the section tells of one variant, empty where it tells nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Told {
    pub description: String,
    pub example: Option<Value>,
    pub returns: Option<Value>,
}

// The section's bytes: `{"about", "ops": {variant: {"description", "example", "returns"}},
// "queries": …}`. An example is the variant's own value as JSON, or one `examples`
// makes where it gives none; `returns` is the shape of what comes back (an op's declared return
// type, the `Reply` variant answering a query), with the types it reaches.
pub fn section(interface: &Interface, tools: &Tools) -> Result<Vec<u8>, String> {
    if tools.is_empty() {
        return Ok(b"{}".to_vec());
    }
    let ops = group(interface, &interface.op, "op", &tools.ops, |tool| {
        returned_by(interface, tool)
    })?;
    let queries = group(
        interface,
        &interface.query,
        "query",
        &tools.queries,
        |tool| answered_by(interface, tool),
    )?;
    let section = json!({ "about": tools.about, "ops": ops, "queries": queries });
    Ok(serde_json::to_vec(&section).expect("JSON values serialize"))
}

fn group(
    interface: &Interface,
    declaration: &str,
    root: &str,
    tools: &[Tool],
    returns: impl Fn(&Tool) -> Result<Option<Value>, String>,
) -> Result<Value, String> {
    let made: BTreeMap<String, Value> = examples(&interface.schema, declaration)
        .map(|made| made.into_iter().collect())
        .unwrap_or_default();
    let mut group = Map::new();
    for tool in tools {
        let mut entry = Map::new();
        if !tool.description.is_empty() {
            entry.insert("description".into(), json!(tool.description));
        }
        let example = match &tool.example {
            Some(bytes) => Some(decode(&interface.schema, declaration, root, bytes).map_err(
                |fault| format!("{root} {}'s example is no {root}: {fault}", tool.variant),
            )?),
            None => made.get(tool.variant).cloned(),
        };
        if let Some(example) = example {
            entry.insert("example".into(), example);
        }
        if let Some(returns) = returns(tool)? {
            entry.insert("returns".into(), returns);
        }
        group.insert(tool.variant.to_owned(), Value::Object(entry));
    }
    Ok(Value::Object(group))
}

fn returned_by(interface: &Interface, tool: &Tool) -> Result<Option<Value>, String> {
    let declared = interface
        .returns
        .iter()
        .flatten()
        .find(|(op, _)| op == tool.variant);
    let Some((_, of)) = declared else {
        return Ok(None);
    };
    let shaped = shape(&interface.schema, &[("returns", of.as_str())], None)
        .map_err(|fault| format!("op {}'s return type: {fault}", tool.variant))?;
    Ok(Some(shaped))
}

fn answered_by(interface: &Interface, tool: &Tool) -> Result<Option<Value>, String> {
    if tool.reply.is_empty() {
        return Ok(None);
    }
    let shaped = variant_shape(&interface.schema, &interface.reply, tool.reply)
        .map_err(|fault| format!("query {}'s reply {}: {fault}", tool.variant, tool.reply))?;
    Ok(Some(shaped))
}

// The section's JSON, keys it does not know left alone so a section can grow.
pub fn read(section: &[u8]) -> Result<Described, String> {
    let value: Value = serde_json::from_slice(section)
        .map_err(|error| format!("the tools section is no JSON: {error}"))?;
    let object = value
        .as_object()
        .ok_or("the tools section is no JSON object")?;
    Ok(Described {
        about: text(object, "about", "the tools section's about")?,
        ops: told(object, "ops")?,
        queries: told(object, "queries")?,
    })
}

fn text(object: &Map<String, Value>, key: &str, what: &str) -> Result<String, String> {
    match object.get(key) {
        None => Ok(String::new()),
        Some(Value::String(text)) => Ok(text.clone()),
        Some(_) => Err(format!("{what} is no string")),
    }
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
        let at = format!("the tools section's {key}.{name}");
        let entry = entry
            .as_object()
            .ok_or_else(|| format!("{at} is no object"))?;
        let told = Told {
            description: text(entry, "description", &format!("{at}.description"))?,
            example: entry.get("example").cloned(),
            returns: entry.get("returns").cloned(),
        };
        read.insert(name.clone(), told);
    }
    Ok(read)
}

// What is wrong with how a section describes `interface`, each problem a sentence; none when every
// op and query is listed with a description and an example of itself.
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
        let known = variants(&interface.schema, declaration);
        for name in &known {
            if !told.contains_key(name) {
                problems.push(format!("{root} {name} is not described"));
            }
        }
        for (name, told) in told {
            if !known.contains(name) {
                problems.push(format!(
                    "{root} {name} is described, and the {root} has no variant {name}"
                ));
                continue;
            }
            if told.description.trim().is_empty() {
                problems.push(format!("{root} {name} has no description"));
            }
            match &told.example {
                Some(example) => {
                    problems.extend(written(interface, declaration, root, name, example))
                }
                None => problems.push(format!("{root} {name} has no example")),
            }
        }
    }
    problems
}

// An example is any JSON that encodes as its own variant, `{"utf8": …}` byte forms included.
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
    match encoded.as_deref() == Some(name) {
        true => None,
        false => Some(format!(
            "{root} {name}'s example is a {}",
            encoded.unwrap_or_default()
        )),
    }
}
