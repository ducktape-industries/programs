use super::*;

fn borsh_string(text: &str) -> Vec<u8> {
    let mut bytes = (text.len() as u32).to_le_bytes().to_vec();
    bytes.extend(text.as_bytes());
    bytes
}

fn set_model(id: &str, name: &str) -> Op {
    Op::SetModel {
        id: id.into(),
        wire: Wire::AnthropicMessages,
        service: SERVICE.into(),
        name: name.into(),
    }
}

#[test]
fn the_retired_model_ops_are_refused_with_words_and_their_queries_answer_nothing() {
    let world = world();
    let chain = &world.chain;
    let retired = [
        set_model("opus", MODEL),
        Op::RemoveModel { id: "opus".into() },
        Op::SetCompactAt {
            id: "opus".into(),
            compact_at: Some(650_000),
        },
        Op::SetServices {
            id: "opus".into(),
            services: vec![SERVICE.into()],
        },
    ];
    for op in retired {
        let error = chain.submit(signed(ADA), PROGRAM, &op).unwrap_err();
        assert_eq!(error.code, code::INVALID_INPUT, "{op:?}");
        assert!(
            error.message.contains("provider = \"anthropic\""),
            "{}",
            error.message
        );
    }
    let model = Query::Model { id: MODEL.into() };
    assert_eq!(
        chain.query::<_, Reply>(PROGRAM, &model).unwrap(),
        Reply::Model(None)
    );
    let models = Query::Models {
        page: PageRequest::default(),
    };
    let Reply::Models(page) = chain.query(PROGRAM, &models).unwrap() else {
        panic!("models answer as a page")
    };
    assert!(page.items.is_empty());
    let services = Query::Services {
        model: MODEL.into(),
    };
    let none = chain.query::<_, Reply>(PROGRAM, &services).unwrap();
    assert_eq!(none, Reply::Services(None));
}

fn retired() -> [Op; 4] {
    [
        set_model(MODEL, "claude-test-1"),
        Op::RemoveModel { id: MODEL.into() },
        Op::SetCompactAt {
            id: MODEL.into(),
            compact_at: Some(650_000),
        },
        Op::SetServices {
            id: MODEL.into(),
            services: vec![SERVICE.into()],
        },
    ]
}

#[test]
fn a_retired_op_reads_as_retired_where_it_is_signed_and_its_bytes_still_decode() {
    let titles: Vec<String> = retired().iter().map(|op| describe(op).title).collect();
    assert_eq!(
        titles,
        [
            "Retired, always refused: set model · claude-test",
            "Retired, always refused: remove model · claude-test",
            "Retired, always refused: set when model claude-test starts compacting",
            "Retired, always refused: set the services that reach model claude-test",
        ]
    );
    for op in retired() {
        let bytes = borsh::to_vec(&op).unwrap();
        assert_eq!(borsh::from_slice::<Op>(&bytes).unwrap(), op);
    }
}

#[test]
fn set_model_bytes_from_before_set_compact_at_still_decode() {
    let mut old = vec![2];
    old.extend(borsh_string(MODEL));
    old.push(0);
    old.extend(borsh_string(SERVICE));
    old.extend(borsh_string("claude-test-1"));
    assert_eq!(
        borsh::from_slice::<Op>(&old).unwrap(),
        set_model(MODEL, "claude-test-1")
    );
}
