use borsh::{BorshSchema, BorshSerialize};
use serde_json::json;

use super::*;

#[derive(BorshSerialize, BorshSchema)]
enum Op {
    RemoveChannelMeta { channel_id: String, key: String },
    HTTPGet,
    Sha256Hex(u8),
    Pin(Option<Option<u8>>),
}

#[test]
fn a_variant_is_spelled_in_snake_case() {
    let names = [
        "RemoveChannelMeta",
        "HTTPGet",
        "Sha256Hex",
        "V2",
        "Ref",
        "already_snake",
    ];
    let written: Vec<String> = names.into_iter().map(spelled).collect();
    assert_eq!(
        written,
        [
            "remove_channel_meta",
            "http_get",
            "sha256_hex",
            "v2",
            "ref",
            "already_snake"
        ]
    );
}

#[test]
fn snake_case_and_the_rust_name_encode_to_the_same_bytes() {
    let value = Op::RemoveChannelMeta {
        channel_id: "c".into(),
        key: "widget.pinned".into(),
    };
    let bytes = borsh::to_vec(&value).unwrap();
    let snake = json!({ "remove_channel_meta": { "channel_id": "c", "key": "widget.pinned" } });
    let rust = json!({ "RemoveChannelMeta": { "channel_id": "c", "key": "widget.pinned" } });
    assert_eq!(bytes_of::<Op>(snake.clone()).unwrap(), bytes);
    assert_eq!(bytes_of::<Op>(rust).unwrap(), bytes);
    assert_eq!(json_of(&value), snake);
    assert_eq!(bytes_of::<Op>(json!("HTTPGet")).unwrap(), [1]);
    assert_eq!(bytes_of::<Op>(json!("http_get")).unwrap(), [1]);
    assert_eq!(json_of(&Op::HTTPGet), json!("http_get"));
    assert_eq!(json_of(&Op::Sha256Hex(3)), json!({ "sha256_hex": 3 }));
}

#[test]
fn an_option_of_an_option_names_some_in_snake_case_and_reads_either() {
    assert_eq!(
        json_of(&Op::Pin(Some(None))),
        json!({ "pin": { "some": null } })
    );
    let four = borsh::to_vec(&Op::Pin(Some(Some(4)))).unwrap();
    assert_eq!(
        bytes_of::<Op>(json!({ "pin": { "Some": 4 } })).unwrap(),
        four
    );
    assert_eq!(
        bytes_of::<Op>(json!({ "pin": { "some": 4 } })).unwrap(),
        four
    );
}

#[test]
fn shapes_examples_and_refusals_speak_snake_case() {
    let schema = schema_of::<Op>();
    let declaration = schema.declaration();
    let shaped = shape(&schema, &[("value", declaration)], None).unwrap();
    let variants = &shaped["types"]["Op"]["enum"];
    assert_eq!(variants["http_get"], Value::Null);
    assert_eq!(variants["sha256_hex"], json!("u8"));
    assert!(variants.get("HTTPGet").is_none(), "{shaped}");
    assert_eq!(
        example(&schema, declaration).unwrap(),
        json!({ "remove_channel_meta": { "channel_id": "", "key": "" } })
    );
    assert_eq!(
        variant_example(&schema, declaration, "http_get").unwrap(),
        Some(json!("http_get"))
    );
    assert_eq!(
        variant_shape(&schema, declaration, "sha256_hex").unwrap()["returns"],
        json!({ "sha256_hex": "u8" })
    );
    assert_eq!(
        variant_named(&schema, declaration, "remove_channel_meta").as_deref(),
        Some("RemoveChannelMeta")
    );
    assert_eq!(
        bytes_of::<Op>(json!("http")).unwrap_err().to_string(),
        "at value: there is no variant named http; the variants are remove_channel_meta, http_get, sha256_hex, pin"
    );
    let missing = bytes_of::<Op>(json!({ "remove_channel_meta": { "key": "k" } })).unwrap_err();
    assert_eq!(
        missing.to_string(),
        "at value.remove_channel_meta: field channel_id is missing"
    );
}

#[test]
fn a_path_step_finds_a_field_by_its_rust_name_while_replies_change_spelling() {
    let reply = json!({ "change": { "source_head": "ab" } });
    let fields = reply.as_object().unwrap();
    assert_eq!(
        entry(fields, "Change"),
        Some(&json!({ "source_head": "ab" }))
    );
    assert_eq!(entry(fields, "change"), entry(fields, "Change"));
    assert_eq!(entry(fields, "Merged"), None);
}
