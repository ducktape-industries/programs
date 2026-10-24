//! Native files_write captures before awaiting preparation and commits only a live call.
use super::*;

fn pending(world: &World, call: &str, path: &str, content: &str) -> (u64, u64) {
    let chain = &world.chain;
    let thread = world.opened("prepare", "write", Some(WINDOW));
    chain.at(3, 0);
    model_says(
        chain,
        json!([tool_use(
            call,
            "files_write",
            json!({ "path": path, "content": content })
        )]),
        "tool_use",
    );
    assert!(assigned(chain, RELAY).is_empty());
    let [(id, open)] = chain.preparations().try_into().unwrap();
    assert_eq!(open.requester, harness());
    assert_eq!(open.preparation.body, content.as_bytes());
    (thread, id)
}

fn stat(chain: &MockChain, path: &str) -> Option<files::EntryInfo> {
    let files::Reply::Stat { entry, .. } = chain
        .query(
            files::PROGRAM,
            &files::Query::Stat {
                path: path.into(),
                at: None,
            },
        )
        .unwrap()
    else {
        panic!()
    };
    entry
}

#[test]
fn files_write_waits_without_relay_and_reads_back_ordered_multichunk_content() {
    let world = world();
    let chain = &world.chain;
    let content = "a".repeat(files::CHUNK_SIZE as usize + 1);
    let (thread, id) = pending(&world, "prepared", "/shared/prepared", &content);
    assert!(stat(chain, "/shared/prepared").is_none());
    chain.at(4, 0);
    chain.answer_preparation(id).unwrap();
    assert!(stat(chain, "/shared/prepared").is_none());
    let [(_, open)] = chain.preparations().try_into().unwrap();
    let (read, products) = open.answered.unwrap();
    assert_eq!(products.len(), 2);
    let before = chain.host(&harness()).borrow().puts.len();
    chain.ready_preparation(id).unwrap();
    assert!(
        !chain
            .host(&harness())
            .borrow()
            .lines
            .contains_key(&(harness(), [b"read/".as_slice(), &id.to_be_bytes()].concat(),))
    );
    assert_eq!(chain.host(&harness()).borrow().puts.len(), before);
    let (text, failed) = result_of(chain, thread, "prepared");
    assert!(!failed, "{text}");
    let info = stat(chain, "/shared/prepared").unwrap();
    let files::Node::File { file, size, .. } = info.node else {
        panic!()
    };
    let host = chain.host(&harness()).borrow();
    let file: files::FileObj = abi::decode(&host.blobs[&file].body).unwrap();
    let chunks = file.chunks;
    assert_eq!(size, content.len() as u64);
    let readback: Vec<u8> = chunks
        .iter()
        .flat_map(|id| host.blobs[id].body.clone())
        .collect();
    assert_eq!(readback, content.as_bytes());
    assert!(host.blobs.contains_key(&read.entry.id));
}

#[test]
fn a_preparation_refusal_never_writes_the_file() {
    let world = world();
    let chain = &world.chain;
    let (thread, id) = pending(&world, "refused", "/shared/refused", "x");
    chain
        .refuse_preparation(id, ::program::invalid("worker refused"))
        .unwrap();
    let (text, failed) = result_of(chain, thread, "refused");
    assert!(failed, "{text}");
    assert!(text.contains("worker refused"), "{text}");
    assert!(stat(chain, "/shared/refused").is_none());
}

#[test]
fn an_intervening_write_uses_the_captured_base_without_refreshing() {
    let world = world();
    let chain = &world.chain;
    let (thread, id) = pending(&world, "stale", "/shared/stale", "old");
    commit(chain, ADA, "/shared/stale", "new");
    chain.at(4, 0);
    chain.answer_preparation(id).unwrap();
    chain.ready_preparation(id).unwrap();
    assert!(
        !chain
            .host(&harness())
            .borrow()
            .lines
            .contains_key(&(harness(), [b"read/".as_slice(), &id.to_be_bytes()].concat(),))
    );
    let (text, failed) = result_of(chain, thread, "stale");
    assert!(failed, "{text}");
}

#[test]
fn a_closed_call_releases_its_preparation_reply() {
    let world = world();
    let chain = &world.chain;
    let (thread, id) = pending(&world, "closed", "/shared/closed", "x");
    chain
        .submit(signed(ADA), harness(), &Op::Close { thread })
        .unwrap();
    chain.at(4, 0);
    chain.answer_preparation(id).unwrap();
    chain.ready_preparation(id).unwrap();
    let (text, failed) = result_of(chain, thread, "closed");
    assert!(failed, "{text}");
    assert!(stat(chain, "/shared/closed").is_none());
    assert!(
        !chain
            .host(&harness())
            .borrow()
            .lines
            .contains_key(&(harness(), [b"read/".as_slice(), &id.to_be_bytes()].concat(),))
    );
}

#[test]
fn a_preparation_releases_only_its_ticket_when_another_holds_the_same_reply() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("shared-reply", "write", Some(WINDOW));
    chain.at(3, 0);
    model_says(
        chain,
        json!([
            tool_use(
                "first",
                "files_write",
                json!({"path":"/shared/first", "content":"same"})
            ),
            tool_use(
                "second",
                "files_write",
                json!({"path":"/shared/second", "content":"same"})
            ),
        ]),
        "tool_use",
    );
    let [(first, _), (second, _)] = chain.preparations().try_into().unwrap();
    chain.at(4, 0);
    chain.answer_preparation(first).unwrap();
    chain.answer_preparation(second).unwrap();
    let prepared = chain.preparations();
    let first_reply = prepared[0].1.answered.as_ref().unwrap().0.entry.id;
    let second_reply = prepared[1].1.answered.as_ref().unwrap().0.entry.id;
    assert_eq!(first_reply, second_reply);
    let second_line = (
        harness(),
        [b"read/".as_slice(), &second.to_be_bytes()].concat(),
    );
    chain
        .host(&harness())
        .borrow_mut()
        .lines
        .insert(second_line.clone(), second_reply);
    chain.ready_preparation(first).unwrap();
    assert!(!result_of(chain, thread, "first").1);
    assert_eq!(
        chain.host(&harness()).borrow().lines.get(&second_line),
        Some(&second_reply)
    );
    chain.ready_preparation(second).unwrap();
    assert!(!result_of(chain, thread, "second").1);
    assert!(
        !chain
            .host(&harness())
            .borrow()
            .lines
            .keys()
            .any(|(program, line)| { program == &harness() && line.starts_with(b"read/") })
    );
}

#[test]
fn a_missing_product_does_not_resume_even_with_a_held_reply() {
    let world = world();
    let chain = &world.chain;
    let (_, id) = pending(&world, "missing", "/shared/missing", "x");
    chain.at(4, 0);
    chain.answer_preparation(id).unwrap();
    let [(_, open)] = chain.preparations().try_into().unwrap();
    let (_, products) = open.answered.unwrap();
    chain.drop_blob(&products[0].id);
    assert_eq!(
        chain.ready_preparation(id).unwrap_err().code,
        code::WRONG_STATE
    );
    assert!(stat(chain, "/shared/missing").is_none());
}
