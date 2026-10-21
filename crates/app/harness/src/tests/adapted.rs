//! The model's service stood in for: what a test answers in Anthropic's own words reaches harness
//! on the harness wire, read by model-wire's Anthropic adapter as the service reads it, and a call
//! harness sent reads back as the body the service sends Anthropic.
use model_wire::anthropic::{self as adapter, Ask};

use super::*;

const PIN: &str = "x-ccs-account";

fn open_row(chain: &MockChain, id: u64) -> Option<RequestRow> {
    let asked = egress::Query::Assigned { key: NODE.to_vec() };
    let egress::Reply::Requests(rows) = chain.query(egress::PROGRAM, &asked).unwrap() else {
        panic!("egress answers its requests")
    };
    rows.into_iter().find(|row| row.id == id)
}

fn target(row: &RequestRow) -> Option<&str> {
    match &row.request {
        Request::Http { head, .. } => Some(head.target.as_str()),
        _ => None,
    }
}

pub(super) fn provider_body(chain: &MockChain, row: &RequestRow) -> Value {
    let wired = body(chain, row);
    let ask = match target(row) {
        Some(model_wire::TURN) => Ask::Turn,
        Some(model_wire::SUMMARY) => Ask::Summary,
        _ => return wired,
    };
    let asked: model_wire::Request =
        serde_json::from_value(wired).expect("a model call is the harness wire");
    adapter::body(&asked, ask)
}

fn budgeted(mut headers: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut budget = model_wire::budget_of(adapter::PROVIDER, &headers);
    let pin = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(PIN));
    budget.account = pin.map(|(_, label)| label.clone());
    headers.extend(budget.headers());
    headers
}

fn whole(status: u16, headers: Vec<(String, String)>, body: &impl serde::Serialize) -> Vec<u8> {
    abi::encode(&http::Response {
        status,
        headers,
        body: serde_json::to_vec(body).expect("the harness wire is JSON"),
    })
}

fn error_status(status: u16) -> u16 {
    match status {
        200 => 502,
        status => status,
    }
}

fn lines(events: &[Value]) -> Vec<Vec<u8>> {
    let each = events.iter();
    each.map(|event| format!("data: {event}\n\n").into_bytes())
        .collect()
}

fn block_events(index: usize, block: &Value) -> Vec<Value> {
    let start = |content_block: Value| json!({ "type": "content_block_start", "index": index, "content_block": content_block });
    let delta =
        |delta: Value| json!({ "type": "content_block_delta", "index": index, "delta": delta });
    let mut events = match block["type"].as_str().unwrap_or_default() {
        "text" => {
            let text = delta(json!({ "type": "text_delta", "text": block["text"] }));
            let mut events = vec![start(json!({ "type": "text", "text": "" })), text];
            let citations = block["citations"].as_array().into_iter().flatten();
            let cited = citations
                .map(|citation| delta(json!({ "type": "citations_delta", "citation": citation })));
            events.extend(cited);
            events
        }
        "thinking" => {
            let thought = delta(json!({ "type": "thinking_delta", "thinking": block["thinking"] }));
            let mut events = vec![
                start(json!({ "type": "thinking", "thinking": "" })),
                thought,
            ];
            let signed = block["signature"].as_str().map(|signature| {
                delta(json!({ "type": "signature_delta", "signature": signature }))
            });
            events.extend(signed);
            events
        }
        kind @ ("tool_use" | "server_tool_use") => {
            let opened =
                json!({ "type": kind, "id": block["id"], "name": block["name"], "input": {} });
            let input =
                json!({ "type": "input_json_delta", "partial_json": block["input"].to_string() });
            vec![start(opened), delta(input)]
        }
        _ => vec![start(block.clone())],
    };
    events.push(json!({ "type": "content_block_stop", "index": index }));
    events
}

pub(super) fn streamed(message: &Value) -> Vec<Vec<u8>> {
    let begun = json!({ "model": message["model"], "usage": message["usage"] });
    let mut events = vec![json!({ "type": "message_start", "message": begun })];
    let blocks = message["content"].as_array().into_iter().flatten();
    for (index, block) in blocks.enumerate() {
        events.extend(block_events(index, block));
    }
    let stopped = json!({ "stop_reason": message["stop_reason"] });
    events.push(json!({ "type": "message_delta", "delta": stopped, "usage": message["usage"] }));
    events.push(json!({ "type": "message_stop" }));
    lines(&events)
}

pub(super) fn adapted(events: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let mut fold = adapter::Fold::new(MODEL);
    let each = events.iter().map(|event| {
        let data = model_wire::event_data(event);
        let folded = data.map(|data| fold.event(&data)).unwrap_or_default();
        match folded.as_slice() {
            [] => b": quiet\n\n".to_vec(),
            [one] => model_wire::sse(one).into_bytes(),
            more => panic!("one provider event folds to {} events", more.len()),
        }
    });
    each.collect()
}

fn from_node(chain: &MockChain, op: &egress::Op) {
    chain.submit(signed(&NODE), egress::PROGRAM, op).unwrap();
}

pub(super) enum Adapted {
    Whole(Vec<u8>),
    Streamed {
        events: Vec<Vec<u8>>,
        headers: Vec<(String, String)>,
    },
}

pub(super) fn stream_whole(
    chain: &MockChain,
    id: u64,
    events: Vec<Vec<u8>>,
    headers: Vec<(String, String)>,
) {
    let count = events.len() as u64;
    let each = (0u64..).zip(events);
    let events = each.map(|(seq, data)| egress::Event { id, seq, data });
    from_node(chain, &egress::Op::Events(events.collect()));
    let end = egress::StreamEnd::Complete {
        status: 200,
        headers,
    };
    let ended = egress::Op::EndStream {
        id,
        events: count,
        end,
    };
    from_node(chain, &ended);
}

fn asked_model(chain: &MockChain, row: &RequestRow) -> String {
    let asked = body(chain, row)["model"].as_str().map(str::to_owned);
    asked.unwrap_or_else(|| MODEL.to_owned())
}

fn turn(response: http::Response, output: Vec<u8>) -> Adapted {
    let headers = budgeted(response.headers);
    if response.status != 200 {
        let failure = adapter::refused(response.status, &headers, &response.body, 0);
        return Adapted::Whole(whole(response.status, headers, &failure));
    }
    let message: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    if !message["content"].is_array() {
        return Adapted::Whole(output);
    }
    let events = adapted(&streamed(&message));
    Adapted::Streamed { events, headers }
}

fn summary(chain: &MockChain, row: &RequestRow, response: http::Response) -> Adapted {
    let headers = budgeted(response.headers);
    let asked = asked_model(chain, row);
    let read = adapter::summarized(&asked, response.status, &headers, &response.body, 0);
    let output = match read {
        Ok(summary) => whole(200, headers, &summary),
        Err(failure) => whole(error_status(response.status), headers, &failure),
    };
    Adapted::Whole(output)
}

fn model(response: http::Response) -> Adapted {
    let headers = budgeted(response.headers);
    let read = adapter::model(response.status, &headers, &response.body, 0);
    let output = match read {
        Ok(model) => whole(200, Vec::new(), &model),
        Err(failure) => whole(error_status(response.status), headers, &failure),
    };
    Adapted::Whole(output)
}

fn wired<T: serde::de::DeserializeOwned>(response: &http::Response) -> bool {
    serde_json::from_slice::<T>(&response.body).is_ok()
}

pub(super) fn adapt(chain: &MockChain, id: u64, output: Vec<u8>) -> Adapted {
    let Some(row) = open_row(chain, id) else {
        return Adapted::Whole(output);
    };
    let Ok(response) = abi::decode::<http::Response>(&output) else {
        return Adapted::Whole(output);
    };
    let ours = wired::<model_wire::Failure>(&response)
        || wired::<model_wire::Summarized>(&response)
        || wired::<model_wire::Model>(&response);
    if ours {
        return Adapted::Whole(output);
    }
    let asked = target(&row).unwrap_or_default();
    match asked {
        model_wire::TURN => turn(response, output),
        model_wire::SUMMARY => summary(chain, &row, response),
        asked if asked.starts_with(model_wire::MODELS) => model(response),
        _ => Adapted::Whole(output),
    }
}
