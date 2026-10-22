use super::*;

#[test]
fn op_variants_keep_their_places_and_tag_8_stays_retired() {
    assert_eq!(
        ::describe::variants::<Op>(),
        [
            "Open",
            "Repin",
            "Close",
            "Port",
            "Send",
            "SendFor",
            "Pump",
            "Header",
            "-",
            "DeliverProven"
        ]
    );
}

#[test]
fn a_send_is_described_by_its_port_network_and_deadline() {
    let send = Op::Send {
        to: THERE,
        port: PORT.into(),
        op: vec![1, 2],
        deadline: Some(9_000),
    };
    let described = describe(&send);
    assert_eq!(described.title, "Send to chat on another network");
    let names: Vec<&str> = described
        .fields
        .iter()
        .map(|field| field.label.as_str())
        .collect();
    assert_eq!(names, ["network", "op", "deadline"]);
}

#[test]
fn a_port_with_no_networks_is_described_as_closing_it() {
    let closing = Op::Port {
        role: PORT.into(),
        from: Vec::new(),
    };
    assert_eq!(describe(&closing).title, "Close the chat port");
}
