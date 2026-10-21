//! A request larger than the model's service takes (413): the largest input of the conversation is
//! left out as a note that stays the same on every later turn, everything before it goes as it
//! went, and a request still too large with every input left out ends the turn.
use super::*;

fn too_large() -> Vec<u8> {
    let error = json!({
        "type": "error",
        "error": { "type": "request_too_large", "message": "Request exceeds the maximum allowed number of bytes." },
    });
    response(413, error)
}

fn raw(chain: &MockChain, row: &RequestRow) -> Vec<u8> {
    let host = chain.host(egress::PROGRAM).borrow();
    let mut bytes = Vec::new();
    for part in row.request.parts() {
        bytes.extend(&host.blobs[part].body);
    }
    bytes
}

fn escaped(text: &str) -> Vec<u8> {
    let quoted = serde_json::to_string(text).unwrap();
    quoted.as_bytes()[1..quoted.len() - 1].to_vec()
}

#[test]
fn a_request_too_large_for_the_service_leaves_its_largest_result_out_and_asks_again() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "read it", Some(WINDOW));
    chain.at(3, 0);
    let call = tool_use("toolu_l", "files_list", json!({ "path": "/" }));
    model_says(chain, json!([call]), "tool_use");
    chain.at(4, 0);
    relay_back(chain, "toolu_l");
    let (text, _) = result_of(chain, thread, "toolu_l");
    let (refused, _) = open_request(chain);
    let before = raw(chain, &refused);
    chain.at(5, 0);
    answer(chain, refused.id, Answer::Output(too_large()));

    let (asked, _) = open_request(chain);
    let after = raw(chain, &asked);
    let note = format!(
        "left out of your context: frame 3 is {} bytes in 1 lines, too large for the model to take whole. Find lines in it with thread_search (frame 3) and read parts of it with thread_read (frame 3, an offset and a length).",
        text.len()
    );
    let (result, stub) = (escaped(&text), escaped(&note));
    let at = before
        .iter()
        .zip(&after)
        .position(|(was, is)| was != is)
        .expect("the request asked again differs");
    assert!(
        before[at..].starts_with(&result),
        "every byte before the result's text goes as it went"
    );
    assert!(
        after[at..].starts_with(&stub),
        "the result's text is the note"
    );
    let rest_unchanged = before[at + result.len()..] == after[at + stub.len()..];
    assert!(
        rest_unchanged,
        "every byte after the result's text goes as it went"
    );
    let Reply::Withheld(left_out) = chain.query(PROGRAM, &Query::Withheld { thread }).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        left_out,
        [WithheldFrame {
            seq: 3,
            size: text.len() as u64
        }]
    );
    let told = notices(chain, thread);
    assert!(
        told.last().unwrap().starts_with(
            "the result of files_list call toolu_l makes the request larger than the model's service takes (the model answered 413"
        ),
        "{told:?}"
    );

    chain.at(6, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Read." }]),
        "end_turn",
    );
    world.mention("m2", "again");
    assert_eq!(
        sent_result(chain, "toolu_l")["content"],
        note,
        "a later turn sends the same note"
    );
}

fn sent_result(chain: &MockChain, call: &str) -> Value {
    let messages = sent_messages(chain);
    let blocks = messages
        .iter()
        .flat_map(|message| message["content"].as_array().cloned().unwrap_or_default());
    let mut found = blocks.filter(|block| block["tool_use_id"] == call);
    found.next().expect("the request sends the call's result")
}

#[test]
fn a_huge_result_a_give_up_already_moved_past_is_the_one_left_out_not_the_new_input() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "read it", Some(WINDOW));
    chain.at(3, 0);
    let call = tool_use("toolu_l", "files_list", json!({ "path": "/" }));
    model_says(chain, json!([call]), "tool_use");
    chain.at(4, 0);
    relay_back(chain, "toolu_l");
    let (text, _) = result_of(chain, thread, "toolu_l");
    let (refused, _) = open_request(chain);
    chain.at(5, 0);
    let error = json!({
        "type": "error",
        "error": { "type": "invalid_request_error", "message": "messages: something else" },
    });
    answer(chain, refused.id, Answer::Output(response(400, error)));
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "the refusal gives the turn up, which moves seen past the result"
    );
    assert!(thread_row(chain, thread).seen >= 3);

    world.mention("m2", "again");
    let (refused, _) = open_request(chain);
    chain.at(6, 0);
    answer(chain, refused.id, Answer::Output(too_large()));
    let Reply::Withheld(left_out) = chain.query(PROGRAM, &Query::Withheld { thread }).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        left_out,
        [WithheldFrame {
            seq: 3,
            size: text.len() as u64
        }],
        "the huge result is left out, not the new input"
    );
    let sent = Value::Array(sent_messages(chain)).to_string();
    assert!(sent.contains("again"), "the new input still goes");
    let note = sent_result(chain, "toolu_l")["content"].clone();
    assert!(
        note.as_str()
            .unwrap()
            .starts_with("left out of your context: frame 3 is"),
        "{note}"
    );
}

#[test]
fn a_request_still_too_large_with_every_input_left_out_ends_the_turn() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "read it", Some(WINDOW));
    chain.at(3, 0);
    let (request, _) = open_request(chain);
    answer(chain, request.id, Answer::Output(too_large()));
    chain.at(4, 0);
    let (request, _) = open_request(chain);
    answer(chain, request.id, Answer::Output(too_large()));
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "with every input left out and still too large, the turn ends"
    );
    let told = notices(chain, thread);
    assert!(
        told.last().unwrap().starts_with("the model answered 413"),
        "{told:?}"
    );
}

#[test]
fn a_huge_result_already_sealed_is_left_out_of_its_sealed_part_too() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "read it", Some(WINDOW));
    chain.at(3, 0);
    let call = tool_use("toolu_l", "files_list", json!({ "path": "/" }));
    model_says(chain, json!([call]), "tool_use");
    chain.at(4, 0);
    relay_back(chain, "toolu_l");
    let (text, _) = result_of(chain, thread, "toolu_l");
    chain.at(5, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Read." }]),
        "end_turn",
    );
    world.mention("m2", "again");
    assert_eq!(sent_result(chain, "toolu_l")["content"], text);
    assert!(
        thread_row(chain, thread).tail_from > 3,
        "the result sits in a sealed part"
    );
    let (refused, _) = open_request(chain);
    chain.at(6, 0);
    answer(chain, refused.id, Answer::Output(too_large()));
    let note = sent_result(chain, "toolu_l")["content"].clone();
    assert!(
        note.as_str()
            .unwrap()
            .starts_with("left out of your context: frame 3 is"),
        "the re-asked request carries the note, not the text: {}",
        note.as_str().map_or(0, str::len)
    );
    let Reply::Withheld(left_out) = chain.query(PROGRAM, &Query::Withheld { thread }).unwrap()
    else {
        panic!()
    };
    assert_eq!(left_out.len(), 1, "one frame is left out");
}
