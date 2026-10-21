use borsh_json::discover::{Side, unfold};
use borsh_json::tools::{check, read, section};
use borsh_json::{Interface, decode, encode, variants};

// Programs whose ops and queries are not described yet. Each leaves the list as its enums derive
// `Described`, and from then on every op and query it has is held to its schema.
const UNDESCRIBED: &[&str] = &["attribution"];

fn every() -> impl Iterator<Item = &'static str> {
    founding::PROGRAMS
        .iter()
        .map(|(program, _)| *program)
        .chain(founding::UNVIEWED.iter().copied())
        .chain(founding::UNFOUNDED.iter().copied())
}

#[test]
fn every_program_describes_each_op_and_query_with_examples_of_its_own() {
    for program in every() {
        let tools =
            founding::tools(program).unwrap_or_else(|| panic!("{program} has no interface"));
        if UNDESCRIBED.contains(&program) {
            assert!(
                tools.is_empty(),
                "{program} describes its ops: take it off UNDESCRIBED"
            );
            continue;
        }
        let schema = borsh::to_vec(&founding::schema(program).unwrap()).unwrap();
        let interface = Interface::read(&schema).unwrap();
        let written = section(&interface, &tools).unwrap_or_else(|why| panic!("{program}: {why}"));
        let problems = check(&interface, &read(&written).unwrap());
        assert!(problems.is_empty(), "{program}: {problems:#?}");
    }
}

#[test]
fn undescribed_names_only_programs_a_founding_packs() {
    for program in UNDESCRIBED {
        assert!(every().any(|known| known == *program), "{program}");
    }
}

#[test]
fn a_program_is_packed_with_the_tools_section_schemas_writes() {
    let release = tempfile::tempdir().unwrap();
    founding::write_schemas(release.path(), &["chat"]).unwrap();
    let written = std::fs::read(release.path().join("chat.tools")).unwrap();
    assert!(read(&written).is_ok());
    let module = b"\0asm\x01\0\0\0".to_vec();
    let packed = founding::embed_tools(&module, &written).unwrap();
    assert_eq!(
        founding::section(&packed, founding::TOOLS_SECTION).unwrap(),
        Some(written)
    );
    assert!(founding::embed_tools(&module, b"[]").is_err());
}

#[test]
fn every_example_network_schema_shows_encodes_and_reads_back_as_itself() {
    let mut broken = Vec::new();
    for program in every() {
        let schema = borsh::to_vec(&founding::schema(program).unwrap()).unwrap();
        let interface = Interface::read(&schema).unwrap();
        let tools = founding::tools(program).unwrap();
        let told = read(&section(&interface, &tools).unwrap()).unwrap();
        for (side, declaration, root) in [
            (Side::Op, interface.op.as_str(), "op"),
            (Side::Query, interface.query.as_str(), "query"),
        ] {
            for name in variants(&interface.schema, declaration) {
                let Ok(unfolded) = unfold(program, &interface, &told, side, &name) else {
                    broken.push(format!("{program} {root} {name}: does not unfold"));
                    continue;
                };
                let Some(example) = unfolded.get("example") else {
                    broken.push(format!("{program} {root} {name}: no example"));
                    continue;
                };
                let round = encode(&interface.schema, declaration, root, example)
                    .map_err(|fault| fault.to_string())
                    .and_then(|bytes| {
                        decode(&interface.schema, declaration, root, &bytes)
                            .map_err(|fault| fault.to_string())
                    });
                match round {
                    Ok(again) if again == *example => {}
                    Ok(again) => broken.push(format!(
                        "{program} {root} {name}: {example} reads back as {again}"
                    )),
                    Err(why) => broken.push(format!("{program} {root} {name}: {example}: {why}")),
                }
            }
        }
    }
    assert!(broken.is_empty(), "{broken:#?}");
}
