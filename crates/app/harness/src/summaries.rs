//! Summary streams retain only their terminal answer; progress extends the model flight.
use ::program::{Error, ExecCtx};
use abi::role::egress::http;
use borsh::{BorshDeserialize, BorshSerialize};
use store::Map;

use crate::{Event, ThreadRow, Wire, wire};

const STREAMS: Map<u64, Stream> = Map::new("summary-stream/");

#[derive(BorshDeserialize, BorshSerialize)]
struct Stream {
    next_seq: u64,
    flow: Flow,
}

#[derive(BorshDeserialize, BorshSerialize)]
enum Flow {
    Open,
    Answer(http::Response),
    Broken(String),
}

pub(crate) fn clear(ctx: &ExecCtx, thread: u64) {
    STREAMS.remove(ctx, &thread);
}

pub(crate) fn streamed(ctx: &ExecCtx, thread: u64, event: &Event) -> Result<(), Error> {
    let kept = STREAMS.get(ctx, &thread)?;
    let mut stream = match kept.filter(|_| event.seq != 0) {
        Some(kept) => kept,
        None => Stream {
            next_seq: 0,
            flow: Flow::Open,
        },
    };
    if event.seq < stream.next_seq {
        return Ok(());
    }
    if event.seq > stream.next_seq {
        stream.flow = Flow::Broken(format!(
            "summary event {} arrived where {} was due",
            event.seq, stream.next_seq
        ));
    }
    stream.next_seq = event.seq + 1;
    if let Some(data) = model_wire::event_data(&event.data) {
        stream.flow = match stream.flow {
            Flow::Open => match serde_json::from_str::<model_wire::SummaryEvent>(&data) {
                Ok(model_wire::SummaryEvent::Summary(summary)) => answer(200, &summary),
                Ok(model_wire::SummaryEvent::Failed(failure)) => answer(400, &failure),
                Err(error) => Flow::Broken(format!(
                    "the summary event is not the harness wire: {error}"
                )),
            },
            Flow::Answer(_) => {
                Flow::Broken("the summary stream carried data after its terminal answer".into())
            }
            broken @ Flow::Broken(_) => broken,
        };
    }
    STREAMS.put(ctx, &thread, &stream);
    Ok(())
}

fn answer(status: u16, body: &impl serde::Serialize) -> Flow {
    match serde_json::to_vec(body) {
        Ok(body) => Flow::Answer(http::Response {
            status,
            headers: Vec::new(),
            body,
        }),
        Err(error) => Flow::Broken(format!("the summary could not be encoded: {error}")),
    }
}

pub(crate) fn read(
    ctx: &ExecCtx,
    thread: u64,
    wire: Wire,
    response: &http::Response,
) -> wire::Summarized {
    let streamed = response.status == 200 && response.body.is_empty();
    if !streamed {
        return wire::summarized(wire, response);
    }
    match STREAMS.get(ctx, &thread) {
        Ok(Some(Stream {
            flow: Flow::Answer(answer),
            ..
        })) => wire::summarized(wire, &answer),
        Ok(Some(Stream {
            flow: Flow::Broken(text),
            ..
        })) => wire::Summarized::Failed(text),
        Ok(_) => {
            wire::Summarized::Failed("the summary stream ended without a completed summary".into())
        }
        Err(error) => {
            wire::Summarized::Failed(format!("the summary stream could not be read: {error}"))
        }
    }
}

pub(crate) fn failed(ctx: &ExecCtx, thread: &mut ThreadRow, text: String) -> Result<(), Error> {
    crate::turns::unhold(ctx, thread)?;
    crate::turns::give_up(
        ctx,
        thread,
        format!("the conversation could not compact: {text}"),
    )
}
