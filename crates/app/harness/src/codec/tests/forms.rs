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
    assert_eq!(json_of(&Shape::Empty), json!("Empty"));
    assert_eq!(
        json_of(&Shape::Point { x: -1, y: 2 }),
        json!({ "Point": { "x": -1, "y": 2 } })
    );
    assert_eq!(
        json_of(&Shape::Tagged("t".into())),
        json!({ "Tagged": "t" })
    );
    assert_eq!(json_of(&Shape::Both(1, 2)), json!({ "Both": [1, 2] }));
    assert_eq!(json_of(&Sparse::First), json!("First"));
    assert_eq!(json_of(&Sparse::Fifth(3)), json!({ "Fifth": 3 }));
    assert_eq!(bytes_of::<Sparse>(json!({ "Fifth": 3 })).unwrap(), [5, 3]);
    assert_eq!(bytes_of::<Shape>(json!({ "Empty": null })).unwrap(), [0]);
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
        json!({ "maybe": { "Some": null } })
    );
    assert_eq!(
        json_of(&Nested {
            maybe: Some(Some(4))
        }),
        json!({ "maybe": { "Some": 4 } })
    );
    let left_out = json!({ "count": 1, "label": "x" });
    assert!(bytes_of::<Named>(left_out).is_ok());
}

#[test]
fn a_recursive_type_reads_to_any_depth_its_data_has() {
    let tree = Tree::Node(vec![Tree::Leaf(1), Tree::Node(vec![Tree::Leaf(2)])]);
    assert_eq!(
        json_of(&tree),
        json!({ "Node": [{ "Leaf": 1 }, { "Node": [{ "Leaf": 2 }] }] })
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
            field: "extra".into()
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
        bytes_of::<Shape>(json!("Circle")),
        Err(Fault::UnknownVariant {
            at: at("value"),
            variant: "Circle".into()
        })
    );
    assert!(matches!(
        bytes_of::<Shape>(json!("Point")),
        Err(Fault::Mismatch { at, .. }) if at == "value.Point"
    ));
    assert!(matches!(
        bytes_of::<Shape>(json!({ "Point": { "x": 1, "y": "two" } })),
        Err(Fault::Mismatch { at, .. }) if at == "value.Point.y"
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
        Err("at value: expected bytes as hex".to_owned())
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
            "Empty": null,
            "Point": { "struct": { "x": "i32", "y": "i32" } },
            "Tagged": "string",
            "Both": { "tuple": ["u8", "u8"] },
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
        json!({ "enum": { "Leaf": "u8", "Node": { "vec": "Tree" } } })
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
