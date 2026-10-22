//! The crosspond request: a reserved service no one registers, asked only as
//! `Request::Crosspond`, elected among the member nodes that serve it, and described by the
//! network it relays to.
use super::*;

const LINK: [u8; 32] = [5; 32];

fn crosspond() -> Request {
    Request::Crosspond { link: LINK }
}

#[test]
fn crosspond_is_reserved_and_registered_by_no_one() {
    let chain = chain();
    let register = Op::Register {
        service: CROSSPOND.into(),
    };
    let refused = submit(&chain, OWNER, &register).unwrap_err();
    assert_eq!(refused.code, code::INVALID_INPUT);
    assert!(RESERVED.contains(&CROSSPOND));
}

#[test]
fn a_crosspond_request_is_elected_only_among_the_nodes_serving_crosspond() {
    let chain = chain();
    serve(&chain, ADA, &[HTTP, RELAY]).unwrap();
    ask(&chain, crosspond());
    serve(&chain, BO, &[CROSSPOND]).unwrap();
    assert_eq!(serving(&chain, CROSSPOND), [BO.to_vec()]);
    for _ in 0..2 {
        ask(&chain, crosspond());
    }
    ask(&chain, http_request(CROSSPOND, "/", Vec::new()));
    assert!(assigned(&chain, ADA).is_empty());
    let rows = assigned(&chain, BO);
    let asked: Vec<&Request> = rows.iter().map(|row| &row.request).collect();
    assert_eq!(asked, [&crosspond(), &crosspond()]);
    let [unserved, as_http] = answers(&chain).try_into().unwrap();
    assert_eq!(unserved.unwrap_err().code, code::NOT_FOUND);
    assert_eq!(as_http.unwrap_err().code, code::INVALID_INPUT);
    chain.at(2, 0);
    answer(&chain, BO, rows[0].id, Answer::Output(b"acks".to_vec())).unwrap();
    let failed = Answer::Failed("the peer is unreachable".into());
    answer(&chain, BO, rows[1].id, failed).unwrap();
    let [_, _, acks, unreachable] = answers(&chain).try_into().unwrap();
    assert_eq!(acks.unwrap(), b"acks");
    assert_eq!(unreachable.unwrap_err().code, code::WRONG_STATE);
}

#[test]
fn a_crosspond_request_reads_as_the_network_it_relays_to() {
    use describe::{Value, field};
    let request = Op::Request {
        request: crosspond(),
        delivery: Delivery::Whole,
        wait: None,
    };
    let described = crate::describe(&request);
    assert!(described.title.contains("crosspond"), "{}", described.title);
    assert!(
        described
            .fields
            .contains(&field("network", Value::Hash(LINK.to_vec()))),
        "{described:?}"
    );
    assert!(
        described
            .fields
            .contains(&field("wait", Value::count(DEFAULT_WAIT)))
    );
}
