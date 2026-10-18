use program::schema::{Roots, roots};

fn programs() -> impl Iterator<Item = &'static str> {
    founding::PROGRAMS
        .iter()
        .map(|(program, _)| *program)
        .chain(founding::UNVIEWED.iter().copied())
}

#[test]
fn every_founded_program_has_a_valid_schema_of_its_op_query_and_reply() {
    for program in programs() {
        let schema = founding::schema(program).unwrap_or_else(|| panic!("{program} has no schema"));
        schema
            .validate()
            .unwrap_or_else(|error| panic!("{program}: {error:?}"));
        let Some(Roots {
            op,
            query,
            reply,
            returns: Some(returns),
        }) = roots(&schema)
        else {
            panic!("{program}: the root names no op, query, reply and returns");
        };
        let rooted = [op, query, reply, returns]
            .iter()
            .all(|declaration| schema.get_definition(*declaration).is_some());
        assert!(rooted, "{program}");
    }
}

#[test]
fn a_name_no_program_carries_has_no_schema() {
    assert!(founding::schema("nowhere").is_none());
}

#[test]
fn a_program_with_no_schema_is_refused_by_name() {
    let release = tempfile::tempdir().unwrap();
    assert!(founding::write_schemas(release.path(), &["chat"]).is_ok());
    assert!(release.path().join("chat.schema").exists());
    assert!(founding::write_schemas(release.path(), &["nowhere"]).is_err());
}
