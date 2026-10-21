use std::collections::BTreeMap;

use borsh::{BorshSchema, BorshSerialize};
use serde_json::json;

use super::*;

#[derive(BorshSerialize, BorshSchema)]
struct Named {
    count: u64,
    label: String,
    note: Option<String>,
}

#[derive(BorshSerialize, BorshSchema)]
struct Newtype(u32);

#[derive(BorshSerialize, BorshSchema)]
struct Pair(u8, bool);

#[derive(BorshSerialize, BorshSchema)]
struct Nothing;

#[derive(BorshSerialize, BorshSchema)]
enum Shape {
    Empty,
    Point { x: i32, y: i32 },
    Tagged(String),
    Both(u8, u8),
}

#[derive(BorshSerialize, BorshSchema)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
enum Sparse {
    First = 0,
    Fifth(u8) = 5,
}

#[derive(BorshSerialize, BorshSchema)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
enum Retiring {
    Open { name: String },
    Push { repo: String, at: u64 } = 3,
    Merge(u64),
}

#[derive(BorshSerialize, BorshSchema)]
struct Wide {
    big: u128,
    small: i8,
    signed: i128,
    ratio: f64,
}

#[derive(BorshSerialize, BorshSchema)]
struct Raw {
    bytes: Vec<u8>,
    digest: [u8; 4],
    list: Vec<u16>,
    fixed: [u16; 2],
    map: BTreeMap<String, u8>,
    pair: (u8, String),
    unit: (),
}

#[derive(BorshSerialize, BorshSchema)]
struct Nested {
    maybe: Option<Option<u8>>,
}

#[derive(BorshSerialize, BorshSchema)]
enum Tree {
    Leaf(u8),
    Node(Vec<Tree>),
}

#[test]
fn a_struct_is_an_object_a_newtype_its_value_a_tuple_struct_an_array_and_a_unit_struct_null() {
    let named = Named {
        count: 7,
        label: "seven".into(),
        note: None,
    };
    assert_eq!(
        json_of(&named),
        json!({ "count": 7, "label": "seven", "note": null })
    );
    assert_eq!(json_of(&Newtype(9)), json!(9));
    assert_eq!(json_of(&Pair(1, true)), json!([1, true]));
    assert_eq!(json_of(&Nothing), json!(null));
}

#[test]
fn an_enum_is_its_variants_name_alone_or_an_object_of_one_key() {
    assert_eq!(json_of(&Shape::Empty), json!("empty"));
    assert_eq!(
        json_of(&Shape::Point { x: -1, y: 2 }),
        json!({ "point": { "x": -1, "y": 2 } })
    );
    assert_eq!(
        json_of(&Shape::Tagged("t".into())),
        json!({ "tagged": "t" })
    );
    assert_eq!(json_of(&Shape::Both(1, 2)), json!({ "both": [1, 2] }));
    assert_eq!(json_of(&Sparse::First), json!("first"));
    assert_eq!(json_of(&Sparse::Fifth(3)), json!({ "fifth": 3 }));
    assert_eq!(bytes_of::<Sparse>(json!({ "fifth": 3 })).unwrap(), [5, 3]);
    assert_eq!(bytes_of::<Shape>(json!({ "empty": null })).unwrap(), [0]);
}

#[test]
fn a_retired_tag_is_a_gap_that_neither_reads_nor_writes_and_the_tags_after_it_hold() {
    let open = Retiring::Open { name: "a".into() };
    assert_eq!(json_of(&open), json!({ "open": { "name": "a" } }));
    let push = Retiring::Push {
        repo: "r".into(),
        at: 7,
    };
    assert_eq!(json_of(&push), json!({ "push": { "repo": "r", "at": 7 } }));
    assert_eq!(borsh::to_vec(&push).unwrap()[0], 3);
    assert_eq!(json_of(&Retiring::Merge(9)), json!({ "merge": 9 }));
    assert_eq!(
        bytes_of::<Retiring>(json!({ "merge": 9 })).unwrap(),
        [4, 9, 0, 0, 0, 0, 0, 0, 0]
    );
    let schema = schema_of::<Retiring>();
    assert_eq!(
        crate::variants(&schema, schema.declaration()),
        ["Open", "Push", "Merge"]
    );
    let retired = crate::decode(&schema, schema.declaration(), "value", &[1, 0, 0, 0, 0]);
    assert!(
        matches!(retired, Err(Fault::Tag { tag: 1, .. })),
        "{retired:?}"
    );
    let named = bytes_of::<Retiring>(json!({ "grant": { "who": 1 } }));
    assert!(
        matches!(named, Err(Fault::UnknownVariant { .. })),
        "{named:?}"
    );
}

#[test]
fn numbers_up_to_64_bits_are_json_numbers_and_128_bits_decimal_strings() {
    let wide = Wide {
        big: u128::MAX,
        small: -128,
        signed: i128::MIN,
        ratio: 0.5,
    };
    assert_eq!(
        json_of(&wide),
        json!({
            "big": u128::MAX.to_string(),
            "small": -128,
            "signed": i128::MIN.to_string(),
            "ratio": 0.5,
        })
    );
    let as_strings = json!({ "count": "7", "label": "x" });
    assert_eq!(
        bytes_of::<Named>(as_strings).unwrap(),
        borsh::to_vec(&Named {
            count: 7,
            label: "x".into(),
            note: None,
        })
        .unwrap()
    );
    assert_eq!(json_of(&u64::MAX), json!(u64::MAX));
    assert_eq!(json_of(&i64::MIN), json!(i64::MIN));
}

#[test]
fn bytes_are_hex_sequences_arrays_maps_pairs_and_tuples_arrays() {
    let raw = Raw {
        bytes: vec![0xde, 0xad],
        digest: [1, 2, 3, 4],
        list: vec![1, 2],
        fixed: [3, 4],
        map: BTreeMap::from([("a".into(), 1), ("b".into(), 2)]),
        pair: (9, "nine".into()),
        unit: (),
    };
    assert_eq!(
        json_of(&raw),
        json!({
            "bytes": "dead",
            "digest": "01020304",
            "list": [1, 2],
            "fixed": [3, 4],
            "map": [["a", 1], ["b", 2]],
            "pair": [9, "nine"],
            "unit": null,
        })
    );
}

#[test]
fn an_option_is_null_or_its_value_and_an_option_of_an_option_names_some() {
    assert_eq!(json_of(&Nested { maybe: None }), json!({ "maybe": null }));
    assert_eq!(
        json_of(&Nested { maybe: Some(None) }),
        json!({ "maybe": { "some": null } })
    );
    assert_eq!(
        json_of(&Nested {
            maybe: Some(Some(4))
        }),
        json!({ "maybe": { "some": 4 } })
    );
    let left_out = json!({ "count": 1, "label": "x" });
    assert!(bytes_of::<Named>(left_out).is_ok());
}

#[test]
fn a_recursive_type_reads_to_any_depth_its_data_has() {
    let tree = Tree::Node(vec![Tree::Leaf(1), Tree::Node(vec![Tree::Leaf(2)])]);
    assert_eq!(
        json_of(&tree),
        json!({ "node": [{ "leaf": 1 }, { "node": [{ "leaf": 2 }] }] })
    );
}

#[test]
fn json_that_does_not_fit_the_schema_says_where_and_why() {
    let at = |path: &str| path.to_owned();
    assert_eq!(
        bytes_of::<Named>(json!({ "count": 1 })),
        Err(Fault::MissingField {
            at: at("value"),
            field: "label".into()
        })
    );
    assert_eq!(
        bytes_of::<Named>(json!({ "count": 1, "label": "x", "extra": 2 })),
        Err(Fault::UnknownField {
            at: at("value"),
            field: "extra".into(),
            known: vec!["count".into(), "label".into(), "note".into()],
        })
    );
    assert_eq!(
        bytes_of::<Named>(json!({ "count": -1, "label": "x" })),
        Err(Fault::OutOfRange {
            at: at("value.count"),
            declaration: "u64".into()
        })
    );
    assert_eq!(
        bytes_of::<Shape>(json!("circle")),
        Err(Fault::UnknownVariant {
            at: at("value"),
            variant: "circle".into(),
            known: vec![
                "empty".into(),
                "point".into(),
                "tagged".into(),
                "both".into()
            ],
        })
    );
    assert!(matches!(
        bytes_of::<Shape>(json!("point")),
        Err(Fault::Mismatch { at, .. }) if at == "value.point"
    ));
    assert!(matches!(
        bytes_of::<Shape>(json!({ "point": { "x": 1, "y": "two" } })),
        Err(Fault::Mismatch { at, .. }) if at == "value.point.y"
    ));
    assert_eq!(
        bytes_of::<Raw>(json!({
            "bytes": "", "digest": "0102", "list": [], "fixed": [1, 2],
            "map": [], "pair": [1, ""], "unit": null,
        })),
        Err(Fault::Length {
            at: at("value.digest"),
            length: 2
        })
    );
    assert!(matches!(
        bytes_of::<Raw>(json!({
            "bytes": "zz", "digest": "01020304", "list": [], "fixed": [1, 2],
            "map": [], "pair": [1, ""], "unit": null,
        })),
        Err(Fault::Mismatch { at, .. }) if at == "value.bytes"
    ));
    assert_eq!(
        bytes_of::<Vec<u8>>(json!([1])).map_err(|fault| fault.to_string()),
        Err(
            "at value: expected bytes as hex, or {\"utf8\": text}, got an array of length 1"
                .to_owned()
        )
    );
}

#[test]
fn bytes_that_do_not_fit_the_schema_are_refused() {
    let schema = schema_of::<Named>();
    let declaration = schema.declaration();
    let mut bytes = borsh::to_vec(&Named {
        count: 1,
        label: "x".into(),
        note: None,
    })
    .unwrap();
    bytes.push(0);
    assert_eq!(
        decode(&schema, declaration, "value", &bytes),
        Err(Fault::Trailing { left: 1 })
    );
    assert_eq!(
        decode(&schema, declaration, "value", &bytes[..3]),
        Err(Fault::Truncated {
            at: "value.count".into()
        })
    );
    let shape = schema_of::<Shape>();
    assert_eq!(
        decode(&shape, shape.declaration(), "value", &[9]),
        Err(Fault::Tag {
            at: "value".into(),
            tag: 9
        })
    );
    let text = schema_of::<String>();
    assert_eq!(
        decode(&text, text.declaration(), "value", &[1, 0, 0, 0, 0xff]),
        Err(Fault::NotUtf8 { at: "value".into() })
    );
    assert_eq!(
        decode(
            &text,
            text.declaration(),
            "value",
            &[0xff, 0xff, 0xff, 0xff]
        ),
        Err(Fault::Truncated { at: "value".into() })
    );
}

#[test]
fn a_shape_names_structs_and_enums_and_writes_the_rest_inline() {
    let schema = schema_of::<Named>();
    assert_eq!(
        shape(&schema, &[("value", schema.declaration())], None).unwrap(),
        json!({
            "value": "Named",
            "types": {
                "Named": { "struct": {
                    "count": "u64",
                    "label": "string",
                    "note": { "option": "string" },
                } },
            },
        })
    );
    let schema = schema_of::<Shape>();
    assert_eq!(
        shape(&schema, &[("value", schema.declaration())], None).unwrap()["types"]["Shape"],
        json!({ "enum": {
            "empty": null,
            "point": { "struct": { "x": "i32", "y": "i32" } },
            "tagged": "string",
            "both": { "tuple": ["u8", "u8"] },
        } })
    );
    let schema = schema_of::<Raw>();
    assert_eq!(
        shape(&schema, &[("value", schema.declaration())], None).unwrap()["types"]["Raw"],
        json!({ "struct": {
            "bytes": "hex",
            "digest": { "hex": 4 },
            "list": { "vec": "u16" },
            "fixed": { "vec": "u16", "len": 2 },
            "map": { "vec": { "tuple": ["string", "u8"] } },
            "pair": { "tuple": ["u8", "string"] },
            "unit": "null",
        } })
    );
    let schema = schema_of::<Tree>();
    assert_eq!(
        shape(&schema, &[("value", schema.declaration())], None).unwrap()["types"]["Tree"],
        json!({ "enum": { "leaf": "u8", "node": { "vec": "Tree" } } })
    );
}

#[test]
fn a_schema_whose_sequence_holds_itself_is_shaped_by_name() {
    let mut definitions = std::collections::BTreeMap::new();
    definitions.insert(
        "Loop".to_owned(),
        Definition::Sequence {
            length_width: 4,
            length_range: 0..=u64::from(u32::MAX),
            elements: "Loop".to_owned(),
        },
    );
    let schema = BorshSchemaContainer::new("Loop".to_owned(), definitions);
    assert_eq!(
        shape(&schema, &[("value", "Loop")], None).unwrap(),
        json!({ "value": { "vec": "Loop" }, "types": { "Loop": { "vec": "Loop" } } })
    );
    assert_eq!(
        decode(&schema, "Loop", "value", &[1, 0, 0, 0, 0, 0, 0, 0]).unwrap(),
        json!([[]])
    );
}

#[derive(BorshSerialize, BorshSchema)]
struct Text {
    name: Vec<u8>,
    digest: [u8; 4],
}

#[test]
fn bytes_written_as_utf8_text_are_the_bytes_their_hex_is() {
    let by_hex = json!({ "name": "726566732f68656164732f646576", "digest": "64657621" });
    let by_text = json!({ "name": { "utf8": "refs/heads/dev" }, "digest": { "utf8": "dev!" } });
    let value = Text {
        name: b"refs/heads/dev".to_vec(),
        digest: *b"dev!",
    };
    assert_eq!(
        bytes_of::<Text>(by_text).unwrap(),
        borsh::to_vec(&value).unwrap()
    );
    assert_eq!(
        bytes_of::<Text>(by_hex).unwrap(),
        borsh::to_vec(&value).unwrap()
    );
    let short = json!({ "name": "", "digest": { "utf8": "dev" } });
    assert!(matches!(bytes_of::<Text>(short), Err(Fault::Length { .. })));
}

#[test]
fn every_wrong_field_of_a_value_is_named_in_one_refusal() {
    let fault = bytes_of::<Named>(json!({ "count": "x", "extra": 1 })).unwrap_err();
    let text = fault.to_string();
    assert!(text.starts_with("3 problems: "), "{text}");
    for named in ["no field extra", "at value.count", "field label is missing"] {
        assert!(text.contains(named), "{text}");
    }
}

#[derive(BorshSerialize, BorshSchema)]
enum Pointer {
    Id(String),
    Name(Vec<u8>),
    Both { name: Vec<u8>, at: Option<Vec<u8>> },
}

#[derive(BorshSerialize, BorshSchema)]
struct Deep {
    from: Pointer,
    all: Vec<Pointer>,
    maybe: Option<Vec<u8>>,
    pairs: Vec<(Vec<u8>, [u8; 2])>,
}

#[test]
fn bytes_written_as_utf8_text_read_so_wherever_they_nest() {
    let by_text = json!({
        "from": { "name": { "utf8": "refs/heads/dev" } },
        "all": [
            { "id": "c1" },
            { "both": { "name": { "utf8": "it's \"x\"" }, "at": { "utf8": "main" } } },
        ],
        "maybe": { "utf8": "🦆" },
        "pairs": [[{ "utf8": "a/b" }, { "utf8": "ok" }]],
    });
    let value = Deep {
        from: Pointer::Name(b"refs/heads/dev".to_vec()),
        all: vec![
            Pointer::Id("c1".into()),
            Pointer::Both {
                name: b"it's \"x\"".to_vec(),
                at: Some(b"main".to_vec()),
            },
        ],
        maybe: Some("🦆".as_bytes().to_vec()),
        pairs: vec![(b"a/b".to_vec(), *b"ok")],
    };
    assert_eq!(
        bytes_of::<Deep>(by_text).unwrap(),
        borsh::to_vec(&value).unwrap()
    );
}

fn written_back<T: BorshSchema>() -> Value {
    let schema = schema_of::<T>();
    let made = example(&schema, schema.declaration()).unwrap();
    let bytes = encode(&schema, schema.declaration(), "value", &made).unwrap();
    let read = decode(&schema, schema.declaration(), "value", &bytes).unwrap();
    assert_eq!(read, made);
    made
}

#[test]
fn an_example_is_a_value_the_codec_writes_and_reads_back_the_same() {
    assert_eq!(
        written_back::<Named>(),
        json!({ "count": 0, "label": "", "note": null })
    );
    assert_eq!(written_back::<Shape>(), json!("empty"));
    assert_eq!(written_back::<Sparse>(), json!("first"));
    assert_eq!(
        written_back::<Wide>(),
        json!({ "big": "0", "small": 0, "signed": "0", "ratio": 0.0 })
    );
    assert_eq!(
        written_back::<Raw>(),
        json!({
            "bytes": "",
            "digest": "00000000",
            "list": [0],
            "fixed": [0, 0],
            "map": [["", 0]],
            "pair": [0, ""],
            "unit": null,
        })
    );
    assert_eq!(written_back::<Nested>(), json!({ "maybe": null }));
    assert_eq!(written_back::<Tree>(), json!({ "leaf": 0 }));
    written_back::<Deep>();
}

#[test]
fn every_variant_of_an_enum_has_an_example_and_one_that_holds_itself_holds_none() {
    let schema = schema_of::<Shape>();
    assert_eq!(
        examples(&schema, schema.declaration()).unwrap(),
        vec![
            ("Empty".to_owned(), json!("empty")),
            ("Point".to_owned(), json!({ "point": { "x": 0, "y": 0 } })),
            ("Tagged".to_owned(), json!({ "tagged": "" })),
            ("Both".to_owned(), json!({ "both": [0, 0] })),
        ]
    );
    let schema = schema_of::<Tree>();
    let made = examples(&schema, schema.declaration()).unwrap();
    assert_eq!(made[1], ("Node".to_owned(), json!({ "node": [] })));
    for (_, value) in &made {
        encode(&schema, schema.declaration(), "value", value).unwrap();
    }
    let schema = schema_of::<Named>();
    assert_eq!(examples(&schema, schema.declaration()).unwrap(), Vec::new());
}

#[test]
fn a_sequence_that_holds_itself_is_empty_in_an_example() {
    let mut definitions = BTreeMap::new();
    definitions.insert(
        "Loop".to_owned(),
        Definition::Sequence {
            length_width: 4,
            length_range: 0..=u64::from(u32::MAX),
            elements: "Loop".to_owned(),
        },
    );
    let schema = BorshSchemaContainer::new("Loop".to_owned(), definitions);
    assert_eq!(example(&schema, "Loop").unwrap(), json!([]));
}

#[test]
fn a_refusal_names_what_exists_and_what_was_given() {
    let said = |fault: Result<Vec<u8>, Fault>| fault.unwrap_err().to_string();
    assert_eq!(
        said(bytes_of::<Shape>(json!("circle"))),
        "at value: there is no variant named circle; the variants are empty, point, tagged, both"
    );
    let extra = json!({ "count": 1, "label": "x", "extra": 2 });
    assert_eq!(
        said(bytes_of::<Named>(extra)),
        "at value: there is no field extra; the fields are count, label, note"
    );
    let point = json!({ "point": { "x": 1, "y": "two" } });
    assert_eq!(
        said(bytes_of::<Shape>(point)),
        "at value.point.y: expected an integer, got the string \"two\""
    );
    assert_eq!(
        said(bytes_of::<Shape>(json!("point"))),
        "at value.point: expected {\"point\": value}, got the string \"point\""
    );
    assert_eq!(
        said(bytes_of::<Named>(json!([1, "x"]))),
        "at value: expected an object, got an array of length 2"
    );
    let two = json!({ "point": 1, "both": 2 });
    assert_eq!(
        said(bytes_of::<Shape>(two)),
        "at value: expected \"variant\" or {\"variant\": value}, got an object with keys both, point"
    );
}

#[test]
fn every_wrong_element_of_a_list_is_named_in_one_refusal() {
    let fault = bytes_of::<Vec<u16>>(json!(["a", 1, true])).unwrap_err();
    assert_eq!(
        fault.to_string(),
        "2 problems: at value[0]: expected an integer, got the string \"a\"; at value[2]: expected an integer, got true"
    );
}

#[test]
fn json_written_out_as_a_string_reads_as_the_json_it_holds() {
    let op = json!({ "point": { "x": 1, "y": 2 } });
    assert_eq!(spoken(&json!(op.to_string())), op);
    assert_eq!(spoken(&json!("empty")), json!("empty"));
    assert_eq!(spoken(&json!(7)), json!(7));
    assert_eq!(unquoted(&json!("[1,2]")), Some(json!([1, 2])));
    assert_eq!(unquoted(&json!("7")), None);
}
