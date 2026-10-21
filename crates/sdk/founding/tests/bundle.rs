use std::fs;
use std::path::Path;

use wasm_encoder::{CustomSection, Module};
use wasmparser::{Parser, Payload};

fn section(bytes: &[u8], name: &str) -> Vec<Vec<u8>> {
    Parser::new(0)
        .parse_all(bytes)
        .filter_map(|payload| match payload.unwrap() {
            Payload::CustomSection(section) if section.name() == name => {
                Some(section.data().to_vec())
            }
            _ => None,
        })
        .collect()
}

fn module(tag: &str) -> Vec<u8> {
    let mut module = Module::new();
    module.section(&CustomSection {
        name: "tag".into(),
        data: tag.as_bytes().into(),
    });
    module.finish()
}

#[test]
fn embedding_replaces_and_stripping_restores_the_program() {
    let program = module("program");
    let view = Module::new().finish();
    let packed = founding::embed(&program, &view).unwrap();
    assert_eq!(
        section(&packed, founding::VIEW_SECTION),
        std::slice::from_ref(&view)
    );
    assert_eq!(founding::embed(&packed, &view).unwrap(), packed);
    assert_eq!(
        founding::strip_section(&packed, founding::VIEW_SECTION).unwrap(),
        program
    );
    let replaced = founding::embed(&packed, &program).unwrap();
    assert_eq!(
        section(&replaced, founding::VIEW_SECTION),
        std::slice::from_ref(&program)
    );
    let mut duplicate = packed.clone();
    duplicate.extend_from_slice(&packed[program.len()..]);
    assert_eq!(founding::embed(&duplicate, &view).unwrap(), packed);
    assert!(founding::embed(b"broken", &view).is_err());
    assert!(founding::embed(&program, b"broken").is_err());
    assert!(founding::embed(&program, b"\0asm\x0d\0\x01\0").is_err());
}

#[test]
fn a_describe_module_rides_beside_the_view_and_imports_nothing() {
    let program = Module::new().finish();
    let view = module("view");
    let describe = module("describe");
    let packed = founding::embed(&program, &view).unwrap();
    let both = founding::embed_describe(&packed, &describe).unwrap();
    assert_eq!(section(&both, founding::VIEW_SECTION), [view]);
    assert_eq!(
        section(&both, founding::DESCRIBE_SECTION),
        std::slice::from_ref(&describe)
    );
    assert_eq!(founding::embed_describe(&both, &describe).unwrap(), both);
    assert_eq!(
        founding::strip_section(&both, founding::DESCRIBE_SECTION).unwrap(),
        packed
    );
    let mut importing = Module::new();
    let mut types = wasm_encoder::TypeSection::new();
    types.ty().function([], []);
    importing.section(&types);
    let mut imports = wasm_encoder::ImportSection::new();
    imports.import("env", "f", wasm_encoder::EntityType::Function(0));
    importing.section(&imports);
    assert!(founding::embed_describe(&program, &importing.finish()).is_err());
}

#[test]
fn a_schema_rides_beside_the_describe_module_and_names_an_op_a_query_and_a_reply() {
    let program = module("program");
    let chat = schema("chat");
    let packed = founding::embed_schema(&program, &chat).unwrap();
    assert_eq!(
        section(&packed, founding::SCHEMA_SECTION),
        std::slice::from_ref(&chat)
    );
    assert_eq!(founding::embed_schema(&packed, &chat).unwrap(), packed);
    assert!(founding::embed_schema(&program, b"\x05").is_err());
    let bare = borsh::to_vec(&borsh::schema::BorshSchemaContainer::for_type::<u64>()).unwrap();
    assert!(founding::embed_schema(&program, &bare).is_err());
}

fn release(dir: &Path) {
    for (program, view) in founding::PROGRAMS {
        fs::write(dir.join(format!("{program}.wasm")), module(program)).unwrap();
        fs::write(dir.join(format!("{view}.wasm")), module(view)).unwrap();
        let notify = format!("{view}.notify");
        fs::write(dir.join(format!("{notify}.wasm")), module(&notify)).unwrap();
        let describe = format!("{program}.describe");
        fs::write(dir.join(format!("{describe}.wasm")), module(&describe)).unwrap();
    }
    for program in founding::UNVIEWED.iter().chain(founding::UNFOUNDED) {
        let stem = program.replace('-', "_");
        fs::write(dir.join(format!("{stem}.wasm")), module(program)).unwrap();
        let describe = format!("{stem}.describe");
        fs::write(dir.join(format!("{describe}.wasm")), module(&describe)).unwrap();
    }
    for view in founding::VIEWS {
        fs::write(dir.join(format!("{view}.wasm")), module(view)).unwrap();
    }
    let programs = founding::PROGRAMS
        .iter()
        .map(|(program, _)| *program)
        .chain(founding::UNVIEWED.iter().copied())
        .chain(founding::UNFOUNDED.iter().copied());
    founding::write_schemas(dir, &programs.collect::<Vec<_>>()).unwrap();
}

fn schema(program: &str) -> Vec<u8> {
    borsh::to_vec(&founding::schema(program).unwrap()).unwrap()
}

#[test]
fn the_bundle_names_every_file_it_holds_and_packs_each_program_with_its_own_view() {
    let built = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    release(built.path());
    founding::bundle(built.path(), out.path(), None).unwrap();

    for (program, view) in founding::PROGRAMS {
        let packed = fs::read(out.path().join(format!("{program}.wasm"))).unwrap();
        assert_eq!(section(&packed, "tag"), [program.as_bytes().to_vec()]);
        assert_eq!(section(&packed, founding::VIEW_SECTION), [module(view)]);
        let describe = module(&format!("{program}.describe"));
        assert_eq!(section(&packed, founding::DESCRIBE_SECTION), [describe]);
        assert_eq!(
            section(&packed, founding::SCHEMA_SECTION),
            [schema(program)]
        );
        let notifier = founding::NOTIFIERS
            .iter()
            .find(|(name, _)| *name == program)
            .map(|(_, view)| module(&format!("{view}.notify")));
        assert_eq!(
            section(&packed, founding::NOTIFY_SECTION),
            Vec::from_iter(notifier)
        );
    }
    for program in founding::UNVIEWED {
        let packed = fs::read(out.path().join(format!("{program}.wasm"))).unwrap();
        assert_eq!(section(&packed, "tag"), [program.as_bytes().to_vec()]);
        assert!(section(&packed, founding::VIEW_SECTION).is_empty());
        assert!(section(&packed, founding::NOTIFY_SECTION).is_empty());
        let describe = module(&format!("{program}.describe"));
        assert_eq!(section(&packed, founding::DESCRIBE_SECTION), [describe]);
        assert_eq!(
            section(&packed, founding::SCHEMA_SECTION),
            [schema(program)]
        );
    }
    let params = fs::read(out.path().join("forge.params")).unwrap();
    assert_eq!(
        abi::decode::<forge::Bounds>(&params).unwrap(),
        founding::forge_bounds()
    );
    let params = fs::read(out.path().join("files.params")).unwrap();
    let roots = files::paths::Roots::of(&params).unwrap();
    assert_eq!(roots, files::paths::Roots::new(founding::files_roots()));
    let hex: String = params.iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(
        hex,
        "0300000004000000686f6d6501010006000000736861726564000004000000636861740201010400000063686174"
    );
    let home = ["home".to_owned(), "ada".to_owned()];
    assert!(roots.is_namespace_root(&home));
    let params = fs::read(out.path().join("halo.params")).unwrap();
    let lanes = abi::decode::<abi::role::halo::Genesis>(&params).unwrap();
    assert_eq!(lanes, founding::halo_lanes());
    let pinned: Vec<(&str, bool)> = lanes
        .lanes
        .iter()
        .map(|lane| (lane.name.as_str(), lane.pinned))
        .collect();
    assert_eq!(pinned, [("ducktape/blobs", true), ("ducktape/live", false)]);

    let text = fs::read_to_string(out.path().join("founding.toml")).unwrap();
    let file: toml::Table = toml::from_str(&text).unwrap();
    let mut named: Vec<String> = ["programs", "views"]
        .iter()
        .flat_map(|list| file[*list].as_array().unwrap())
        .flat_map(|entry| ["code", "params"].map(|key| entry.get(key)))
        .flatten()
        .map(|path| path.as_str().unwrap().to_string())
        .collect();
    named.push("founding.toml".into());
    let founded: Vec<&str> = file["programs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|program| program["id"].as_str().unwrap())
        .collect();
    for hook in file["hooks"].as_array().unwrap() {
        let hook = hook.as_str().unwrap();
        assert!(founded.contains(&hook), "{hook} hooks but is not founded");
    }
    named.sort();
    let mut held: Vec<String> = fs::read_dir(out.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    held.sort();
    assert_eq!(named, held);
}

#[test]
fn one_program_packs_to_the_bytes_the_bundle_holds_for_it() {
    let built = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    release(built.path());
    let source = stamp(COMMIT);
    founding::bundle(built.path(), out.path(), Some(&source)).unwrap();
    let viewed = founding::PROGRAMS.iter().map(|(program, _)| *program);
    for program in viewed.chain(founding::UNVIEWED.iter().copied()) {
        let bundled = fs::read(out.path().join(format!("{program}.wasm"))).unwrap();
        let packed = founding::pack(built.path(), program, Some(&source)).unwrap();
        assert_eq!(packed, bundled);
    }
    assert_eq!(
        founding::modules("chat"),
        Some((Some("chat_view"), Some("chat_view")))
    );
    assert_eq!(founding::modules("files"), Some((Some("files_view"), None)));
    assert_eq!(founding::modules("egress"), Some((None, None)));
    for program in founding::UNFOUNDED {
        assert_eq!(founding::modules(program), Some((None, None)));
        let packed = founding::pack(built.path(), program, None).unwrap();
        assert_eq!(section(&packed, "tag"), [program.as_bytes().to_vec()]);
        assert!(!out.path().join(format!("{program}.wasm")).exists());
    }
    assert_eq!(founding::modules("explorer_view"), None);
    assert!(founding::pack(built.path(), "explorer_view", None).is_err());
}

const COMMIT: &str = "4cd69803b2faa3996ce1ddadbbc9b1d6e7641133";

fn stamp(commit: &str) -> founding::Source {
    founding::Source {
        repo: founding::REPO.into(),
        commit: commit.into(),
    }
}

#[test]
fn a_stamp_names_the_commit_and_one_commit_packs_one_set_of_bytes() {
    let built = tempfile::tempdir().unwrap();
    release(built.path());
    let at = |commit: &str| founding::pack(built.path(), "chat", Some(&stamp(commit))).unwrap();
    let stamped = at(COMMIT);
    assert_eq!(at(COMMIT), stamped);
    assert_eq!(
        section(&stamped, founding::SOURCE_SECTION),
        [abi::encode(&stamp(COMMIT))]
    );
    let read = founding::interface(&stamped).unwrap();
    assert_eq!((read.code, read.source), (true, Some(stamp(COMMIT))));
    let unstamped = founding::pack(built.path(), "chat", None).unwrap();
    assert!(section(&unstamped, founding::SOURCE_SECTION).is_empty());
    assert_eq!(founding::interface(&unstamped).unwrap().source, None);
    let other = at("1bb4744a329b8848e797b299687df15fc8cd76b7");
    assert_ne!(other, stamped);
    for code in [&stamped, &other] {
        let stripped = founding::strip_section(code, founding::SOURCE_SECTION).unwrap();
        assert_eq!(stripped, unstamped, "the stamp is the only difference");
    }
    let upper = COMMIT.to_uppercase();
    for commit in ["4cd69803", "dev", upper.as_str(), ""] {
        let refused = founding::embed_source(&unstamped, &stamp(commit));
        assert!(refused.is_err(), "{commit:?}");
    }
}
