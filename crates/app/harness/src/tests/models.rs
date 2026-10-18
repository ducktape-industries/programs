use super::*;

fn model(chain: &MockChain, id: &str) -> Option<ModelRow> {
    let Reply::Model(row) = chain
        .query(PROGRAM, &Query::Model { id: id.into() })
        .unwrap()
    else {
        panic!()
    };
    row
}

#[test]
fn the_first_account_to_set_a_model_owns_it_and_only_it_changes_or_removes_it() {
    let world = world();
    let chain = &world.chain;
    let taken = chain.submit(signed(BO), PROGRAM, &set_model(MODEL, "claude-other"));
    assert_eq!(taken.unwrap_err().code, code::UNAUTHORIZED);
    let removed = chain.submit(signed(BO), PROGRAM, &Op::RemoveModel { id: MODEL.into() });
    assert_eq!(removed.unwrap_err().code, code::UNAUTHORIZED);
    chain
        .submit(signed(BO), PROGRAM, &set_model("bo-model", "claude-bo"))
        .unwrap();
    let Reply::Models(page) = chain
        .query(
            PROGRAM,
            &Query::Models {
                page: PageRequest::default(),
            },
        )
        .unwrap()
    else {
        panic!()
    };
    let owners: Vec<(String, AccountNumber)> = page
        .items
        .into_iter()
        .map(|row| (row.id, row.owner))
        .collect();
    assert_eq!(
        owners,
        [("bo-model".into(), world.bo), (MODEL.into(), world.ada)]
    );
    chain
        .submit(signed(ADA), PROGRAM, &Op::RemoveModel { id: MODEL.into() })
        .unwrap();
    assert_eq!(model(chain, MODEL), None);
    let gone = chain.submit(signed(ADA), PROGRAM, &Op::RemoveModel { id: MODEL.into() });
    assert_eq!(gone.unwrap_err().code, code::NOT_FOUND);
}

#[test]
fn a_model_record_is_checked_and_keeps_its_window_while_it_names_the_same_model() {
    let world = world();
    let chain = &world.chain;
    for refused in [
        set_model("Claude", MODEL),
        set_model("", MODEL),
        set_model("ok", "has space"),
        set_model("ok", ""),
    ] {
        let error = chain.submit(signed(ADA), PROGRAM, &refused).unwrap_err();
        assert_eq!(error.code, code::INVALID_INPUT, "{refused:?}");
    }
    for service in egress::RESERVED {
        let reserved = Op::SetModel {
            id: "ok".into(),
            wire: Wire::AnthropicMessages,
            service: service.into(),
            name: MODEL.into(),
        };
        let error = chain.submit(signed(ADA), PROGRAM, &reserved).unwrap_err();
        assert_eq!(error.code, code::INVALID_INPUT, "{service}");
    }
    world.opened("m1", "hello", Some(WINDOW));
    assert_eq!(model(chain, MODEL).unwrap().window, Window::Tokens(WINDOW));
    let moved = Op::SetModel {
        id: MODEL.into(),
        wire: Wire::AnthropicMessages,
        service: "elsewhere".into(),
        name: MODEL.into(),
    };
    chain.submit(signed(ADA), PROGRAM, &moved).unwrap();
    let row = model(chain, MODEL).unwrap();
    assert_eq!(
        (row.service.as_str(), row.window),
        ("elsewhere", Window::Tokens(WINDOW))
    );
    chain
        .submit(signed(ADA), PROGRAM, &set_model(MODEL, "claude-next"))
        .unwrap();
    assert_eq!(model(chain, MODEL).unwrap().window, Window::Unasked);
}

#[test]
fn a_model_op_reads_as_its_title() {
    let set = describe(&set_model(MODEL, "claude-test-1"));
    assert_eq!(set.title, "Set model · claude-test");
    let removed = describe(&Op::RemoveModel { id: MODEL.into() });
    assert_eq!(removed.title, "Remove model · claude-test");
}

fn borsh_string(text: &str) -> Vec<u8> {
    let mut bytes = (text.len() as u32).to_le_bytes().to_vec();
    bytes.extend(text.as_bytes());
    bytes
}

#[test]
fn a_models_compact_at_is_its_owners_to_set_and_clear_and_set_model_keeps_it() {
    let world = world();
    let chain = &world.chain;
    let set = |compact_at| Op::SetCompactAt {
        id: MODEL.into(),
        compact_at,
    };
    let taken = chain.submit(signed(BO), PROGRAM, &set(Some(1000)));
    assert_eq!(taken.unwrap_err().code, code::UNAUTHORIZED);
    let zero = chain.submit(signed(ADA), PROGRAM, &set(Some(0)));
    assert_eq!(zero.unwrap_err().code, code::INVALID_INPUT);
    let nothing = Op::SetCompactAt {
        id: "nothing".into(),
        compact_at: Some(5),
    };
    let missing = chain.submit(signed(ADA), PROGRAM, &nothing);
    assert_eq!(missing.unwrap_err().code, code::NOT_FOUND);
    assert_eq!(model(chain, MODEL).unwrap().compact_at, None);

    chain
        .submit(signed(ADA), PROGRAM, &set(Some(650_000)))
        .unwrap();
    assert_eq!(model(chain, MODEL).unwrap().compact_at, Some(650_000));
    chain
        .submit(signed(ADA), PROGRAM, &set_model(MODEL, "claude-next"))
        .unwrap();
    let row = model(chain, MODEL).unwrap();
    assert_eq!(
        (row.name.as_str(), row.compact_at),
        ("claude-next", Some(650_000))
    );
    chain.submit(signed(ADA), PROGRAM, &set(None)).unwrap();
    assert_eq!(model(chain, MODEL).unwrap().compact_at, None);

    chain.submit(signed(ADA), PROGRAM, &set(Some(9))).unwrap();
    let removed = Op::RemoveModel { id: MODEL.into() };
    chain.submit(signed(ADA), PROGRAM, &removed).unwrap();
    chain
        .submit(signed(ADA), PROGRAM, &set_model(MODEL, "claude-test"))
        .unwrap();
    assert_eq!(
        model(chain, MODEL).unwrap().compact_at,
        None,
        "a removed model's default goes with it"
    );
    let title = describe(&set(Some(650_000))).title;
    assert_eq!(title, "Set when model claude-test starts compacting");
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

#[test]
fn a_model_row_stored_before_compact_at_reads_as_it_was() {
    let stored = crate::state::StoredModel {
        id: MODEL.into(),
        owner: 7,
        wire: Wire::AnthropicMessages,
        service: SERVICE.into(),
        name: MODEL.into(),
        window: Window::Tokens(WINDOW),
    };
    let mut old = borsh_string(MODEL);
    old.extend(7u64.to_le_bytes());
    old.push(0);
    old.extend(borsh_string(SERVICE));
    old.extend(borsh_string(MODEL));
    old.push(2);
    old.extend(WINDOW.to_le_bytes());
    assert_eq!(borsh::to_vec(&stored).unwrap(), old);
    let read = borsh::from_slice::<crate::state::StoredModel>(&old).unwrap();
    assert_eq!(read.row(Some(9)).compact_at, Some(9));
}
