//! Parts held on lines beyond a thread's own request parts: the close and summarize parts on lines
//! named by wire and target, and `HoldSealed` putting parts sealed before lines back on their lines.
use ::program::{HashKind, MockHost};

use super::*;
use crate::Asking;
use crate::state::{SEALED, THREADS};

fn held_under(chain: &MockChain, under: &[u8]) -> Vec<(Vec<u8>, BlobId)> {
    let host = chain.host(PROGRAM).borrow();
    host.lines
        .iter()
        .filter(|((holder, at), _)| holder == PROGRAM && at.starts_with(under))
        .map(|((_, at), id)| (at.clone(), *id))
        .collect()
}

fn summarizing(world: &World) -> u64 {
    let chain = &world.chain;
    let thread = world.opened("m1", "remember the plan", Some(2_000));
    chain.at(3, 0);
    model_says_after(
        chain,
        json!([{ "type": "text", "text": "Noted." }]),
        "end_turn",
        1000,
    );
    world.mention("m2", "and now?");
    thread
}

fn released_alone(chain: &MockChain, mine: &[u8], before: &[(Vec<u8>, BlobId)]) {
    let host = chain.host(PROGRAM);
    let exec = host.exec(MockHost::env(PROGRAM));
    for (_, id) in before {
        let (kind, body) = {
            let state = host.borrow();
            let blob = &state.blobs[id];
            (blob.kind.clone(), blob.body.clone())
        };
        exec.blob_put(HashKind::Sha256, kind, body).unwrap();
    }
    exec.blob_release_lines(mine.to_vec());
    assert!(held_under(chain, mine).is_empty());
}

#[test]
fn the_close_and_summarize_parts_are_held_on_their_wires_lines() {
    let world = world();
    summarizing(&world);
    let chain = &world.chain;
    assert_eq!(held_under(chain, b"const/close/").len(), 1);
    assert_eq!(held_under(chain, b"const/summarize/").len(), 1);
}

#[test]
fn hold_sealed_puts_parts_sealed_before_lines_back_on_their_threads_lines() {
    let world = world();
    let thread = summarizing(&world);
    let chain = &world.chain;
    let mine = [thread.to_be_bytes().as_slice(), b"/sealed/"].concat();
    let before = held_under(chain, &mine);
    assert!(
        !before.is_empty(),
        "the summary request is made of sealed parts"
    );
    released_alone(chain, &mine, &before);
    chain.submit(signed(ADA), PROGRAM, &Op::HoldSealed).unwrap();
    assert_eq!(held_under(chain, &mine), before);
    let again = chain
        .submit(signed(ADA), PROGRAM, &Op::HoldSealed)
        .unwrap_err();
    assert_eq!(again.code, code::WRONG_STATE, "{}", again.message);
    chain.at(6, 0);
    chain.at(7, 0);
    chain.submit(signed(ADA), PROGRAM, &Op::HoldSealed).unwrap();
    assert_eq!(held_under(chain, &mine), before);
}

#[test]
fn hold_sealed_lets_a_closed_idle_thread_go_of_its_sealed_rows_and_holds_nothing() {
    let world = world();
    let thread = summarizing(&world);
    let chain = &world.chain;
    let mine = [thread.to_be_bytes().as_slice(), b"/sealed/"].concat();
    let before = held_under(chain, &mine);
    assert!(
        !before.is_empty(),
        "the summary request is made of sealed parts"
    );
    released_alone(chain, &mine, &before);
    let host = chain.host(PROGRAM);
    let exec = host.exec(MockHost::env(PROGRAM));
    let mut row = THREADS.get(&exec, &thread).unwrap().unwrap();
    row.closed = true;
    row.asking = Asking::Idle;
    THREADS.put(&exec, &thread, &row);
    let kept = SEALED.scan(&exec, SEALED.prefix_of(&thread)).unwrap();
    assert!(
        !kept.is_empty(),
        "a thread closed before lines kept its sealed rows"
    );
    chain.submit(signed(ADA), PROGRAM, &Op::HoldSealed).unwrap();
    assert!(
        held_under(chain, &mine).is_empty(),
        "a closed thread's parts are not held"
    );
    let exec = host.exec(MockHost::env(PROGRAM));
    let left = SEALED.scan(&exec, SEALED.prefix_of(&thread)).unwrap();
    assert!(
        left.is_empty(),
        "a closed idle thread lets go of its sealed rows"
    );
}
