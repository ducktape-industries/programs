use super::*;

fn posting() -> Partial {
    Partial {
        said: vec![
            Said::Text("On it.".into()),
            Said::ToolUse {
                id: "toolu_03".into(),
                name: "chat_post".into(),
                input: r#"{"channel":"general","text":"The deploy"#.into(),
            },
        ],
        forming: Forming::Kept { index: 1 },
        ..streaming()
    }
}

fn call(state: CallState) -> CallRow {
    CallRow {
        thread: 7,
        id: "toolu_03".into(),
        tool: "chat_post".into(),
        input: r#"{"channel":"general","text":"The deploy"#.into(),
        made: 8970,
        state,
    }
}

#[test]
fn a_call_in_the_streaming_reply_reads_as_its_call_runs() {
    let mut net = Net {
        partials: vec![(7, posting())],
        ..Net::default()
    };
    net.calls.push(call(CallState::Showing));
    let (mut cx, shared) = seated(net, EDDY);
    let changes = cx.host().stream::<Changes<harness::Harness>>();
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    cx.simulate_click("thread-7");
    cx.run_until_parked();
    assert!(cx.has_text("Showing"), "{:?}", cx.texts());
    assert!(!cx.has_text("Forming"), "{:?}", cx.texts());
    let steps = [
        (CallState::Relaying, "Relaying", 10),
        (CallState::Writing, "Writing", 11),
        (
            CallState::Done {
                height: 8972,
                failed: false,
            },
            "Done",
            12,
        ),
        (CallState::Voided { height: 8973 }, "Taken back", 13),
    ];
    for (state, words, height) in steps {
        let formed = Partial {
            forming: Forming::Nothing,
            ..posting()
        };
        {
            let mut net = shared.borrow_mut();
            net.partials = vec![(7, formed)];
            net.calls.retain(|row| row.id != "toolu_03");
            net.calls.push(call(state));
        }
        let written = [
            Written::Partial(7),
            Written::Call {
                thread: 7,
                call: "toolu_03".into(),
            },
        ];
        changes.send(Some(Change {
            height,
            keys: written.iter().map(Written::key).collect(),
        }));
        cx.run_until_parked();
        assert!(cx.has_text(words), "{words}: {:?}", cx.texts());
        assert!(!cx.has_text("Formed"), "{words}: {:?}", cx.texts());
    }
}

#[test]
fn a_call_taken_back_says_when() {
    let mut net = Net::default();
    net.calls.push(call(CallState::Voided { height: 8973 }));
    let tool_use = Said::ToolUse {
        id: "toolu_03".into(),
        name: "chat_post".into(),
        input: r#"{"channel":"general","text":"The deploy"}"#.into(),
    };
    let (_, turn) = net
        .frames
        .iter_mut()
        .find(|(thread, frame)| *thread == 7 && matches!(frame.body, Body::Turn { .. }))
        .unwrap();
    let Body::Turn { said, .. } = &mut turn.body else {
        panic!()
    };
    said.push(tool_use);
    let (mut cx, _) = seated(net, EDDY);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    cx.simulate_click("thread-7");
    cx.run_until_parked();
    assert!(cx.has_text("Taken back"), "{:?}", cx.texts());
    cx.simulate_click("call-toolu_03");
    cx.run_until_parked();
    assert!(
        cx.has_text("made at block 8,970 · taken back at block 8,973"),
        "{:?}",
        cx.texts()
    );
}
