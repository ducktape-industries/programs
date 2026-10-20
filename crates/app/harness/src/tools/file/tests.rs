use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

use super::{Piece, filled, pieces};

fn bound() -> Map<String, Value> {
    let Value::Object(bound) = json!({
        "number": 7,
        "repo": "notes",
        "change": { "Change": { "source_head": "abc", "reviews": [{ "id": 4 }] } },
    }) else {
        unreachable!()
    };
    bound
}

#[test]
fn a_reference_alone_keeps_its_type_and_inside_text_is_written_in() {
    let body = json!({ "SquashMerge": {
        "repo": "$repo",
        "change": "$number",
        "expected_from": "$change.Change.source_head",
        "first": "$change.Change.reviews.0.id",
        "message": "Merge #$number of $repo.",
        "held": { "Ref": ["$change.Change.reviews"] },
    } });
    let said = filled(&body, &bound(), &BTreeSet::new()).unwrap();
    assert_eq!(
        said,
        json!({ "SquashMerge": {
            "repo": "notes",
            "change": 7,
            "expected_from": "abc",
            "first": 4,
            "message": "Merge #7 of notes.",
            "held": { "Ref": [[{ "id": 4 }]] },
        } })
    );
}

#[test]
fn a_doubled_dollar_is_one_and_a_dollar_before_no_name_is_refused() {
    let said = filled(&json!("costs $$5 for $repo"), &bound(), &BTreeSet::new()).unwrap();
    assert_eq!(said, json!("costs $5 for notes"));
    let refused = pieces("costs $5").unwrap_err();
    assert!(refused.contains("write $$ for a dollar sign"), "{refused}");
    assert_eq!(
        pieces("$a.b..c").unwrap_err(),
        "`$a.b..c`: a $ starts no name here; write $$ for a dollar sign"
    );
    assert_eq!(
        pieces("x $a.b.").unwrap(),
        vec![
            Piece::Text("x ".into()),
            Piece::Ref(vec!["a", "b"]),
            Piece::Text(".".into())
        ]
    );
}

#[test]
fn a_path_that_reaches_nothing_says_where_it_stopped() {
    let missing = filled(
        &json!("$change.Change.target_head"),
        &bound(),
        &BTreeSet::new(),
    )
    .unwrap_err();
    assert_eq!(
        missing,
        "$change.Change has no target_head: it is an object"
    );
    let deeper = filled(&json!("$repo.name"), &bound(), &BTreeSet::new()).unwrap_err();
    assert_eq!(deeper, "$repo has no name: it is a string");
    let unset = filled(&json!("$merged"), &bound(), &BTreeSet::new()).unwrap_err();
    assert_eq!(unset, "$merged is not set");
}

#[test]
fn an_absent_optional_input_leaves_its_key_out_and_is_refused_inside_text_or_a_list() {
    let absent = BTreeSet::from(["offset".to_owned()]);
    let body = json!({ "path": "$repo", "offset": "$offset", "nested": { "offset": "$offset" } });
    let said = filled(&body, &bound(), &absent).unwrap();
    assert_eq!(said, json!({ "path": "notes", "nested": {} }));
    let inside = filled(&json!("from $offset"), &bound(), &absent).unwrap_err();
    assert_eq!(inside, "$offset is not set");
    let listed = filled(&json!(["$offset"]), &bound(), &absent).unwrap_err();
    assert_eq!(listed, "$offset is not set");
}
