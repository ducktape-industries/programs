use borsh::schema::Definition;

use super::*;

const LIVE_OPS: [(&str, i64); 9] = [
    ("Open", 0),
    ("Repin", 1),
    ("Close", 2),
    ("Port", 3),
    ("Send", 4),
    ("SendFor", 5),
    ("Pump", 6),
    ("Header", 7),
    ("DeliverProven", 9),
];

const RETIRED_OP: i64 = 8;

fn tags_of(declaration: &str) -> Vec<(String, i64)> {
    let schema = ::program::schema::interface::<crate::Crosspond>();
    let Some(Definition::Enum { variants, .. }) = schema.get_definition(declaration) else {
        panic!("{declaration} is an enum in crosspond's schema");
    };
    variants
        .iter()
        .map(|(tag, name, _)| (name.clone(), *tag))
        .collect()
}

#[test]
fn each_op_keeps_its_tag_and_tag_8_names_none() {
    let tags = tags_of("Op");
    let live: Vec<(String, i64)> = LIVE_OPS
        .iter()
        .map(|(name, tag)| ((*name).to_owned(), *tag))
        .collect();
    assert_eq!(tags, live);
    assert!(tags.iter().all(|(_, tag)| *tag != RETIRED_OP));
}

#[test]
fn bytes_at_tag_8_decode_as_no_op_and_tag_9_is_deliver_proven() {
    let retired = [
        [8u8].as_slice(),
        &THERE,
        &1u64.to_le_bytes(),
        &1u64.to_le_bytes(),
        &0u32.to_le_bytes(),
    ]
    .concat();
    assert!(abi::decode::<Op>(&retired).is_err());
    let proven = Op::DeliverProven {
        from: THERE,
        epoch: 1,
        low: None,
        packets: Vec::new(),
    };
    let bytes = abi::encode(&proven);
    assert_eq!(bytes[0], 9);
    assert_eq!(abi::decode::<Op>(&bytes).unwrap(), proven);
}

#[test]
fn the_one_certificate_kind_is_tag_0_and_no_other_byte_decodes() {
    assert_eq!(
        tags_of("CertificateKind"),
        [("Ed25519SimplexFinalize".to_owned(), 0)]
    );
    assert_eq!(abi::encode(&CertificateKind::Ed25519SimplexFinalize), [0]);
    for byte in 1..=u8::MAX {
        assert!(abi::decode::<CertificateKind>(&[byte]).is_err());
    }
}
