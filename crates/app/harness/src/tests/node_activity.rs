use super::*;

const NODED: &str = "node";

fn noded(chain: &MockChain) {
    let register = egress::Op::Register {
        service: NODED.into(),
    };
    let assign = egress::Op::Assign {
        service: NODED.into(),
        nodes: vec![NODE.to_vec()],
    };
    for op in [register, assign] {
        chain.submit(signed(ADA), egress::PROGRAM, &op).unwrap();
    }
    let serve = egress::Op::Serve {
        services: vec![SERVICE.into(), RELAY.into(), NODED.into()],
    };
    chain
        .submit(signed(&NODE), egress::PROGRAM, &serve)
        .unwrap();
}

fn sent(chain: &MockChain, row: &RequestRow) -> Vec<u8> {
    let host = chain.host(egress::PROGRAM).borrow();
    let mut bytes = Vec::new();
    for part in row.request.parts() {
        bytes.extend(&host.blobs[part].body);
    }
    bytes
}

fn node_says(chain: &MockChain, row: &RequestRow, status: u16, body: Vec<u8>) {
    let response = http::Response {
        status,
        headers: Vec::new(),
        body,
    };
    answer(chain, row.id, Answer::Output(abi::encode(&response)));
}

fn listed() -> Vec<u8> {
    let frame = activity::Frame {
        height: 9,
        position: 1,
        time: 7,
        signer: vec![4; 32],
        target: "chat".into(),
        applied: false,
    };
    abi::encode(&activity::Reply::Frames(store::PageResponse {
        height: 9,
        items: vec![frame],
        next: Some(vec![1, 2]),
    }))
}

fn refused(why: Option<Error>) -> Vec<u8> {
    [vec![3], abi::encode(&why)].concat()
}

#[test]
fn network_activity_reads_the_nodes_index_and_says_why_a_frame_was_refused() {
    let world = world();
    let chain = &world.chain;
    noded(chain);
    let thread = world.opened("m1", "what did 21 do", Some(WINDOW));
    chain.at(3, 0);
    let calls = [
        ("a_list", json!({ "account": 21, "cursor": null })),
        (
            "a_why",
            json!({ "account": 21, "height": 9, "position": 1 }),
        ),
        (
            "a_ran",
            json!({ "account": 21, "height": 8, "position": 0 }),
        ),
        (
            "a_old",
            json!({ "account": 21, "height": 2, "position": 0 }),
        ),
        ("a_half", json!({ "account": 21, "height": 9 })),
    ];
    let uses: Vec<Value> = calls
        .iter()
        .map(|(id, input)| tool_use(id, "network_activity", input.clone()))
        .collect();
    model_says(chain, Value::Array(uses), "tool_use");
    chain.at(4, 0);
    for (id, _) in &calls {
        relay_back(chain, id);
    }
    chain.at(5, 0);
    let rows = assigned(chain, NODED);
    assert_eq!(rows.len(), 4, "a half-named frame asks nothing: {rows:?}");
    for row in &rows {
        let Request::Http { head, .. } = &row.request else {
            panic!("an HTTP request to the node");
        };
        assert!(matches!(head.method, http::Method::Post));
        assert_eq!(head.target, "/activity");
        assert_eq!(row.delivery, egress::Delivery::Whole);
        let asked = sent(chain, row);
        match asked.split_first() {
            Some((0, _)) => {
                let query: activity::Query = abi::decode(&asked).unwrap();
                assert!(matches!(query, activity::Query::Acted { account: 21, .. }));
                node_says(chain, row, 200, listed());
            }
            Some((5, at)) => match abi::decode::<(u64, u32)>(at).unwrap() {
                (9, 1) => {
                    let why = Error::new("unauthorized", "not yours");
                    node_says(chain, row, 200, refused(Some(why)));
                }
                (8, 0) => node_says(chain, row, 200, refused(None)),
                _ => {
                    let behind =
                        abi::Refusal::new("not_yet", "the activity index is still opening");
                    node_says(chain, row, 400, abi::encode(&behind));
                }
            },
            other => panic!("not an activity query: {other:?}"),
        }
    }
    let json_of = |id: &str| {
        let (text, failed) = result_of(chain, thread, id);
        assert!(!failed, "{id}: {text}");
        serde_json::from_str::<Value>(&text).unwrap()
    };
    assert_eq!(
        json_of("a_list"),
        json!({
            "frames": [{ "height": 9, "position": 1, "time": 7, "program": "chat", "applied": false }],
            "next": "0102",
        })
    );
    assert_eq!(
        json_of("a_why"),
        json!({ "refusal": { "code": "unauthorized", "message": "not yours" } })
    );
    assert_eq!(json_of("a_ran"), json!({ "refusal": null }));
    let (old, failed) = result_of(chain, thread, "a_old");
    assert!(failed && old.starts_with("not_yet"), "{old}");
    let (half, failed) = result_of(chain, thread, "a_half");
    assert!(
        failed && half.contains("height and its position together"),
        "{half}"
    );
}
