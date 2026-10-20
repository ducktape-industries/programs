//! Every tool of the catalog takes the input a model sends it, read whole or streamed in pieces
//! split anywhere: each input `samples.rs` gives a tool fits its schema and reaches its `check`
//! as the model wrote it.
mod samples;

use abi::role::egress::http::Response;
use serde_json::{Value, json};

use super::{CATALOG, Entry, find};
use crate::wire::{self, Answered};
use crate::{Partial, Said, Wire};

fn unfit(schema: &Value, value: &Value, at: &str, faults: &mut Vec<String>) {
    if let Some(kind) = schema["type"].as_str() {
        let fits = match kind {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "integer" => value.is_u64() || value.is_i64(),
            "boolean" => value.is_boolean(),
            _ => true,
        };
        if !fits {
            faults.push(format!("{at} is not {kind}: {value}"));
            return;
        }
    }
    if let Some(choices) = schema["enum"].as_array()
        && !choices.contains(value)
    {
        faults.push(format!("{at} is none of {choices:?}"));
    }
    if let (Some(fields), Some(properties)) = (value.as_object(), schema["properties"].as_object())
    {
        let required: Vec<&str> = schema["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        for name in &required {
            if !fields.contains_key(*name) {
                faults.push(format!("{at} has no {name}, which the schema requires"));
            }
        }
        for (name, field) in fields {
            let at = format!("{at}.{name}");
            match properties.get(name) {
                None => faults.push(format!("{at} is no property of the schema")),
                Some(_) if field.is_null() && !required.contains(&name.as_str()) => {}
                Some(property) => unfit(property, field, &at, faults),
            }
        }
    }
    if let (Some(items), Some(each)) = (value.as_array(), schema.get("items")) {
        for (index, item) in items.iter().enumerate() {
            unfit(each, item, &format!("{at}[{index}]"), faults);
        }
    }
}

fn untyped(entry: &Entry) -> Vec<String> {
    let schema = (entry.schema)();
    let typed = ["type", "enum", "const", "anyOf", "oneOf", "allOf", "$ref"];
    let properties = schema["properties"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    properties
        .into_iter()
        .filter(|(_, field)| typed.iter().all(|key| field.get(key).is_none()))
        .map(|(name, _)| name)
        .collect()
}

fn written(sample: &Value, untyped: &[String]) -> Vec<(&'static str, String)> {
    let spaced = serde_json::to_string_pretty(sample).unwrap();
    let mut forms = vec![("compact", sample.to_string()), ("spaced", spaced)];
    let mut quoted = sample.clone();
    let mut any = false;
    for name in untyped {
        if let Some(field) = quoted.get_mut(name.as_str())
            && !field.is_null()
        {
            *field = json!(field.to_string());
            any = true;
        }
    }
    if any {
        forms.push(("with its untyped fields as JSON text", quoted.to_string()));
    }
    forms
}

fn splits(text: &str) -> Vec<(String, Vec<&str>)> {
    let mut all: Vec<(String, Vec<&str>)> = text
        .char_indices()
        .skip(1)
        .map(|(at, _)| {
            let (head, tail) = text.split_at(at);
            (format!("cut at byte {at}"), vec![head, tail])
        })
        .collect();
    all.push(("whole".to_owned(), vec![text]));
    let chars = text
        .char_indices()
        .map(|(at, c)| &text[at..at + c.len_utf8()])
        .collect();
    all.push(("one char a piece".to_owned(), chars));
    all
}

fn input_of(said: &[Said]) -> Result<Value, String> {
    let [Said::ToolUse { input, .. }] = said else {
        return Err(format!("the reply is not one call: {said:?}"));
    };
    serde_json::from_str(input).map_err(|error| format!("its input is no JSON ({error}): {input}"))
}

fn whole(tool: &str, sample: &Value) -> Result<Value, String> {
    let call = json!({ "type": "tool_use", "id": "toolu_1", "name": tool, "input": sample });
    let body = json!({
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4-5",
        "content": [call],
        "stop_reason": "tool_use",
        "usage": { "input_tokens": 10, "output_tokens": 5 },
    });
    let response = Response {
        status: 200,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    };
    match wire::answered(Wire::AnthropicMessages, &response) {
        Answered::Reply(reply) => input_of(&reply.said),
        _ => Err("the whole reply does not read as a turn".to_owned()),
    }
}

fn event(data: &Value) -> Vec<u8> {
    format!("data: {data}").into_bytes()
}

fn streamed(tool: &str, pieces: &[&str]) -> Result<Value, String> {
    let mut partial = Partial::new(Wire::AnthropicMessages);
    let started = json!({
        "type": "message_start",
        "message": { "model": "claude-sonnet-4-5", "usage": { "input_tokens": 10 } },
    });
    let block = json!({
        "type": "content_block_start",
        "index": 0,
        "content_block": { "type": "tool_use", "id": "toolu_1", "name": tool, "input": {} },
    });
    wire::fold(&mut partial, &event(&started));
    wire::fold(&mut partial, &event(&block));
    for piece in pieces {
        let delta = json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": { "type": "input_json_delta", "partial_json": piece },
        });
        wire::fold(&mut partial, &event(&delta));
    }
    let stopped = json!({ "type": "content_block_stop", "index": 0 });
    wire::fold(&mut partial, &event(&stopped));
    input_of(&partial.said)
}

fn missed(entry: &Entry, named: &str, sample: &Value) -> Vec<String> {
    let mut faults = Vec::new();
    unfit(&(entry.schema)(), sample, named, &mut faults);
    if let Err(refused) = (entry.check)(sample) {
        faults.push(format!("{named}: its check refuses it: {refused}"));
    }
    match whole(entry.name, sample) {
        Ok(input) if input == *sample => {}
        Ok(input) => faults.push(format!("{named}, read whole, reaches the tool as {input}")),
        Err(why) => faults.push(format!("{named}, read whole: {why}")),
    }
    for (form, text) in written(sample, &untyped(entry)) {
        let first = splits(&text).into_iter().find_map(|(split, pieces)| {
            let why = match streamed(entry.name, &pieces) {
                Ok(input) if input == *sample => return None,
                Ok(input) => format!("reaches the tool as {input}"),
                Err(why) => why,
            };
            Some(format!("{named}, streamed {form}, {split}: {why}"))
        });
        faults.extend(first);
    }
    faults
}

#[test]
fn every_tool_takes_its_input_whole_and_streamed_in_pieces_split_anywhere() {
    let all = samples::all();
    let mut faults = Vec::new();
    for entry in CATALOG {
        let Some((_, inputs)) = all.iter().find(|(name, _)| *name == entry.name) else {
            let name = entry.name;
            faults.push(format!(
                "{name} has no sample input in tools/tests/samples.rs"
            ));
            continue;
        };
        for (n, sample) in inputs.iter().enumerate() {
            let named = format!("{} input {}", entry.name, n + 1);
            faults.extend(missed(entry, &named, sample));
        }
    }
    assert!(faults.is_empty(), "\n{}", faults.join("\n"));
}

#[test]
fn every_sample_input_is_for_a_tool_of_the_catalog() {
    let stale: Vec<&str> = samples::all()
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| find(name).is_none())
        .collect();
    assert!(stale.is_empty(), "no tool is named {stale:?}");
}

#[test]
fn the_samples_write_untyped_fields_and_awkward_text() {
    let all = samples::all();
    let untyped: Vec<&str> = CATALOG
        .iter()
        .filter(|entry| !untyped(entry).is_empty())
        .map(|entry| entry.name)
        .collect();
    assert!(untyped.contains(&"network_call"), "{untyped:?}");
    let text = serde_json::to_string(&all.iter().map(|(_, inputs)| inputs).collect::<Vec<_>>());
    let text = text.unwrap();
    for awkward in ["'", "\\\"", "\\\\", "\\n", "\\u0001", "🦆", "null", "[["] {
        assert!(text.contains(awkward), "no sample writes {awkward}");
    }
}
