use std::path::Path;

use program::schema::{Roots, roots};

fn programs() -> impl Iterator<Item = &'static str> {
    founding::PROGRAMS
        .iter()
        .map(|(program, _)| *program)
        .chain(founding::UNVIEWED.iter().copied())
}

#[test]
fn the_committed_interfaces_are_the_built_ones() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("interfaces");
    let write = std::env::var_os("INTERFACES_WRITE").is_some();
    let every: Vec<&str> = programs()
        .chain(founding::UNFOUNDED.iter().copied())
        .collect();
    let mut stale = Vec::new();
    for program in &every {
        let built = borsh::to_vec(&founding::schema(program).unwrap()).unwrap();
        let path = dir.join(format!("{program}.schema"));
        if write {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&path, &built).unwrap();
        } else if std::fs::read(&path).ok() != Some(built) {
            stale.push(*program);
        }
    }
    let committed: Vec<String> = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    let unknown: Vec<&String> = committed
        .iter()
        .filter(|name| {
            !every
                .iter()
                .any(|program| **name == format!("{program}.schema"))
        })
        .collect();
    assert!(
        stale.is_empty() && unknown.is_empty(),
        "interfaces/ is not what the programs build (stale: {stale:?}, no program: {unknown:?}): \
         write it again with INTERFACES_WRITE=1"
    );
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
    assert!(release.path().join("chat.tools").exists());
    assert!(founding::write_schemas(release.path(), &["nowhere"]).is_err());
}
