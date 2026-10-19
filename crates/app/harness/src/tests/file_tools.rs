//! Tool files: a TOML file in /shared/tools or in an agent's own tools folder is a tool on the next
//! model call, checked against the live schemas; one that does not load says why in the system
//! prompt, and a step refused mid-run says which.
use ::program::schema::interface;
use programs::Programs;

use super::forge_tools::{answered, head, refusal, seeded, turn, world as forged};
use super::*;
use crate::schema::tests::{code_with, listed};

const MERGE: &str = r#"name = "forge_merge"
description = "Squash-merge a forge change into its target branch, refusing if either side moved since it was read."

[input]
repo = { type = "string", description = "repository, e.g. programs" }
number = { type = "integer", description = "the change's number" }

[[step]]
query = "forge"
body = { Change = { repo = "$repo", n = "$number", page = {} } }
as = "change"

[[step]]
op = "forge"
body = { SquashMerge = { repo = "$repo", change = "$number", from = "$change.Change.change.from", into = "$change.Change.change.into", expected_from = "$change.Change.source_head", expected_into = "$change.Change.target_head" } }
as = "merged"

[returns]
text = "Merged #$number → $merged.Merged.oid"
"#;

const CHANNEL: &str = r#"name = "channel_name"
description = "A chat channel's name."

[input]
channel = { type = "string", description = "the channel's id" }

[[step]]
query = "chat"
body = { Channel = { channel_id = "$channel" } }
as = "found"

[returns]
text = "$channel is called $found.Channel.channel.name"
"#;

// `forge_tools::world` with forge's and chat's schemas listed, as their code carries them
fn tooled() -> World {
    let mut world = forged();
    world.chain.seat::<Programs>(programs::PROGRAM);
    let codes = [
        (forge::PROGRAM, code_with(&interface::<forge::Forge>())),
        (chat::PROGRAM, code_with(&interface::<chat::Chat>())),
    ];
    listed(&world.chain, &codes);
    world
}

// the tools and the system prompt of the model call open now
fn sent(chain: &MockChain) -> (Vec<Value>, String) {
    let (row, _) = open_request(chain);
    let request = body(chain, &row);
    let tools = request["tools"].as_array().unwrap().clone();
    let blocks = request["system"].as_array().unwrap();
    let texts: Vec<&str> = blocks
        .iter()
        .filter_map(|block| block["text"].as_str())
        .collect();
    (tools, texts.join("\n"))
}

fn agent_tools(world: &World) -> AgentTools {
    let asked = Query::AgentTools { agent: world.agent };
    let Reply::AgentTools(tools) = world.chain.query(PROGRAM, &asked).unwrap() else {
        panic!("AgentTools answers AgentTools")
    };
    tools
}

fn named<'t>(tools: &'t [Value], name: &str) -> Vec<&'t Value> {
    tools.iter().filter(|tool| tool["name"] == name).collect()
}

#[test]
fn a_tool_file_is_a_tool_on_the_next_model_call_and_one_that_does_not_load_says_why() {
    let world = tooled();
    let chain = &world.chain;
    let broken = MERGE
        .replace("forge_merge", "broken")
        .replace("change = \"$number\"", "number = \"$number\"");
    let typo = MERGE
        .replace("forge_merge", "typo")
        .replace("$change.Change.source_head", "$chnge.Change.source_head");
    let files = [
        ("/shared/tools/forge_merge.toml", MERGE.to_owned()),
        ("/shared/tools/broken.toml", broken),
        ("/shared/tools/typo.toml", typo),
        (
            "/shared/tools/files_read.toml",
            MERGE.replace("forge_merge", "files_read"),
        ),
        ("/shared/tools/README.md", "not a tool".to_owned()),
    ];
    for (path, text) in &files {
        commit(chain, ADA, path, text);
    }
    world.opened("m1", "merge things", Some(WINDOW));
    let (tools, system) = sent(chain);

    let [merge] = named(&tools, "forge_merge").try_into().unwrap();
    let description = merge["description"].as_str().unwrap();
    assert!(
        description.ends_with("(tool file /shared/tools/forge_merge.toml)"),
        "{description}"
    );
    assert_eq!(merge["input_schema"]["required"], json!(["number", "repo"]));
    assert_eq!(
        merge["input_schema"]["properties"]["number"]["type"],
        json!("integer")
    );
    assert!(named(&tools, "broken").is_empty() && named(&tools, "typo").is_empty());
    assert_eq!(named(&tools, "files_read").len(), 1);
    let left_out = [
        "- File tools left out: ",
        "tool broken: step 2 (op forge): at op.SquashMerge: there is no field number",
        "tool typo: step 2 (op forge): $chnge.Change.source_head names no input and no earlier step's `as`",
        "tool files_read: files_read is harness's own tool, which a file does not take the place of",
    ];
    for line in left_out {
        assert!(system.contains(line), "{line} in {system}");
    }
    assert!(!system.contains("README"), "{system}");
    let listed = agent_tools(&world);
    let problems: Vec<&str> = listed.problems.iter().map(String::as_str).collect();
    assert_eq!(problems.len(), 3, "{problems:?}");
    assert!(
        left_out[1..]
            .iter()
            .all(|line| problems.iter().any(|problem| problem.starts_with(*line))),
        "{problems:?}"
    );
}

#[test]
fn forge_merge_reads_a_change_and_squash_merges_it_against_the_heads_it_read() {
    let world = tooled();
    let chain = &world.chain;
    commit(chain, ADA, "/shared/tools/forge_merge.toml", MERGE);
    let thread = world.opened("m1", "merge the change", Some(WINDOW));
    seeded(&world, thread);
    let open = json!({
        "repo": "notes",
        "from": "dev",
        "into": "main",
        "title": "dev's own",
        "body": "Why: a.",
    });
    turn(chain, 7, &[("o1", "forge_change_open", open)]);
    answered(chain, thread, "o1");

    let merge = json!({ "repo": "notes", "number": 1 });
    turn(chain, 9, &[("f1", "forge_merge", merge.clone())]);
    let main = head(chain, "notes", "main");
    assert_eq!(
        answered(chain, thread, "f1"),
        json!({ "ok": true, "text": format!("Merged #1 → {main}") })
    );
    let asked = forge::Query::Change {
        repo: "notes".into(),
        n: 1,
        page: PageRequest::first(1),
    };
    let forge::Reply::Change { change, .. } = chain.query(forge::PROGRAM, &asked).unwrap() else {
        panic!("a change answers Change")
    };
    assert_eq!(change.state, forge::ChangeState::Merged);
    assert_eq!(change.merged_by, Some(Principal::Account(world.agent)));

    turn(chain, 11, &[("f2", "forge_merge", merge)]);
    let again = refusal(chain, thread, "f2");
    assert!(again.starts_with("step 2 (op forge): "), "{again}");
    assert_eq!(head(chain, "notes", "main"), main);
}

#[test]
fn a_call_whose_input_fits_no_field_of_the_file_is_refused_before_it_runs() {
    let world = tooled();
    let chain = &world.chain;
    commit(chain, ADA, "/shared/tools/forge_merge.toml", MERGE);
    let thread = world.opened("m1", "merge", Some(WINDOW));
    chain.at(3, 0);
    let input = json!({ "repo": "notes", "number": "one", "why": "no" });
    model_says(
        chain,
        json!([tool_use("f1", "forge_merge", input)]),
        "tool_use",
    );
    let refused: Vec<String> = frames(chain, thread)
        .into_iter()
        .filter_map(|body| match body {
            Body::Refused { problem, .. } => Some(problem),
            _ => None,
        })
        .collect();
    assert_eq!(
        refused,
        ["forge_merge input: there is no field why; field number is no integer"]
    );
    assert!(assigned(chain, RELAY).is_empty());
}

#[test]
fn an_agents_own_tool_file_takes_the_place_of_a_shared_one_and_its_query_answers_in_place() {
    let world = tooled();
    let chain = &world.chain;
    let shared = CHANNEL.replace("A chat channel's name.", "The shared one.");
    commit(chain, ADA, "/shared/tools/channel_name.toml", &shared);
    let thread = world.opened("m1", "name a channel", Some(WINDOW));
    let own = json!({
        "path": "/home/Scout/tools/channel_name.toml",
        "content": CHANNEL,
        "message": "a tool of my own",
    });
    turn(chain, 3, &[("w1", "files_write", own)]);
    answered(chain, thread, "w1");

    let listed = agent_tools(&world);
    let own: Vec<&AgentTool> = listed
        .tools
        .iter()
        .filter(|tool| tool.name == "channel_name")
        .collect();
    let [own] = own.try_into().unwrap();
    let ToolSource::File { path, .. } = &own.source else {
        panic!("{own:?}")
    };
    assert_eq!(path, "/home/Scout/tools/channel_name.toml");
    assert_eq!(listed.shadowed, ["/shared/tools/channel_name.toml"]);
    let read = AgentTool {
        name: "files_read".into(),
        source: ToolSource::Native,
    };
    assert!(listed.tools.contains(&read), "{listed:?}");

    let (tools, _) = sent(chain);
    let [tool] = named(&tools, "channel_name").try_into().unwrap();
    assert_eq!(
        tool["description"],
        json!("A chat channel's name. (tool file /home/Scout/tools/channel_name.toml)")
    );
    let asked = json!({ "channel": "general" });
    turn(chain, 5, &[("q1", "channel_name", asked)]);
    assert_eq!(
        answered(chain, thread, "q1"),
        json!({ "ok": true, "text": "general is called general" })
    );
    let unknown = json!({ "channel": "nowhere" });
    turn(chain, 7, &[("q2", "channel_name", unknown)]);
    assert_eq!(
        answered(chain, thread, "q2"),
        json!({
            "ok": true,
            "unsaid": "returns: $found.Channel has no channel: it is null",
            "steps": { "found": { "Channel": null } },
        })
    );
}
