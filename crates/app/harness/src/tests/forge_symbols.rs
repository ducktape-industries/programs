//! `forge_read` by symbol with forge seated: one Rust item read by its name at a path or by id, the
//! file's items listed with their lines, a markdown section read by its heading, and a symbol
//! asked of a file that is no Rust, or beside lines, refused.
use super::forge_tools::{answered, refusal, turn, world};
use super::*;

const LIB: &str = "//! The lib.\nuse std::fmt;\n\npub struct Config {\n    name: String,\n}\n\nimpl Config {\n    /// A new one.\n    pub fn new(name: &str) -> Config {\n        Config { name: name.into() }\n    }\n}\n\nimpl fmt::Display for Config {\n    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {\n        f.write_str(\"}\")\n    }\n}\n";

const NOTES: &str = "# Notes\nintro\n## Plan\none\n## Later\ntwo\n";

#[test]
fn a_rust_item_is_read_by_its_name_and_a_markdown_section_by_its_heading() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "read items", Some(WINDOW));
    let files = json!({
        "repo": "notes",
        "files": [
            { "path": "src/lib.rs", "content": LIB },
            { "path": "NOTES.md", "content": NOTES },
        ],
        "message": "start",
    });
    turn(chain, 3, &[("c1", "forge_commit", files)]);
    answered(chain, thread, "c1");
    let read = |extra: Value| {
        let mut asked = json!({ "repo": "notes", "path": "src/lib.rs" });
        asked
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        asked
    };
    turn(
        chain,
        5,
        &[
            ("r1", "forge_read", read(json!({ "symbol": "Config::new" }))),
            ("r2", "forge_read", read(json!({ "symbols": true }))),
            (
                "r3",
                "forge_read",
                json!({ "repo": "notes", "path": "NOTES.md", "heading": "## Plan" }),
            ),
            (
                "r4",
                "forge_read",
                json!({ "repo": "notes", "path": "NOTES.md", "symbol": "Plan" }),
            ),
            (
                "r5",
                "forge_read",
                read(json!({ "symbol": "Config", "lines": [1, 2] })),
            ),
            ("r6", "forge_read", read(json!({ "symbol": "fmt" }))),
        ],
    );
    let new = answered(chain, thread, "r1");
    assert_eq!(
        (
            new["lines"].clone(),
            new["total_lines"].clone(),
            new["text"].clone()
        ),
        (
            json!([9, 12]),
            json!(19),
            json!(
                "    /// A new one.\n    pub fn new(name: &str) -> Config {\n        Config { name: name.into() }\n    }\n"
            )
        )
    );
    let outline = answered(chain, thread, "r2");
    let names: Vec<(Value, Value)> = outline["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| (item["name"].clone(), item["lines"].clone()))
        .collect();
    assert_eq!(
        names,
        [
            (json!("Config"), json!([4, 6])),
            (json!("impl Config"), json!([8, 13])),
            (json!("Config::new"), json!([9, 12])),
            (json!("impl fmt::Display for Config"), json!([15, 19])),
            (json!("Config::fmt"), json!([16, 18])),
        ]
    );
    let id = outline["id"].as_str().unwrap().to_owned();
    assert_eq!(
        answered(chain, thread, "r3")["text"],
        json!("## Plan\none\n")
    );
    assert!(refusal(chain, thread, "r4").contains("no .rs file"));
    assert!(refusal(chain, thread, "r5").contains("each go alone"));
    assert!(refusal(chain, thread, "r6").contains("Config::fmt (lines 16-18)"));

    let by_id = json!({ "repo": "notes", "id": id, "symbol": "impl fmt::Display for Config" });
    turn(chain, 7, &[("r7", "forge_read", by_id)]);
    assert_eq!(answered(chain, thread, "r7")["lines"], json!([15, 19]));
}
