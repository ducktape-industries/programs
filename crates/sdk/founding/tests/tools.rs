use borsh_json::Interface;
use borsh_json::tools::{check, read};

// Programs whose ops and queries are not described yet. Each leaves the list as its
// `Program::TOOLS` lands, and from then on every op and query it has is held to its schema.
const UNDESCRIBED: &[&str] = &[
    "programs",
    "valset",
    "identity",
    "blobs",
    "awaits",
    "halo",
    "scheduler",
    "access",
    "ownership",
    "chat",
    "forge",
    "files",
    "activity",
    "egress",
    "attribution",
    "harness",
    "actions",
    "actions-pods",
];

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
        let described = read(&program::tools::json(&tools)).unwrap();
        let problems = check(&interface, &described);
        assert!(problems.is_empty(), "{program}: {problems:#?}");
    }
}

#[test]
fn undescribed_names_only_programs_a_founding_packs() {
    for program in UNDESCRIBED {
        assert!(every().any(|known| known == *program), "{program}");
    }
}
