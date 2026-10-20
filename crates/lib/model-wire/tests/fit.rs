//! One conversation that moved between Claude and Codex, replayed to each in both directions:
//! what each block becomes depends on who wrote it and who reads the call, by the lane rule's
//! table, with nothing a reader can't take ever sent to it.
use model_wire::{
    Block, Lane, Message, Request, Shown, Target, Thinking, UserBlock, anthropic, codex, fit,
};
use serde_json::{Value, json};

const CLAUDE: &str = "claude-opus-5-5";
const GPT: &str = "gpt-6.1-sol";

fn lane(provider: &str, model: &str, service: &str, account: &str) -> Lane {
    Lane {
        service: Some(service.into()),
        account: Some(account.into()),
        ..Lane::of(provider, model)
    }
}

fn result(call: &str) -> UserBlock {
    UserBlock::Result {
        call: call.into(),
        failed: false,
        content: vec![Shown::Text {
            text: format!("read {call}"),
        }],
    }
}

fn reasoning_item() -> Value {
    json!({ "type": "reasoning", "id": "rs_1", "encrypted_content": "gAAAA-codex-sealed", "summary": [{ "type": "summary_text", "text": "codex plans" }] })
}

fn conversation() -> Vec<Message> {
    let searched = json!({
        "type": "web_search_tool_result", "tool_use_id": "srvtoolu_1",
        "content": [{ "type": "web_search_result", "title": "Ducktape", "url": "https://ducktape.example", "encrypted_content": "Eq-search-sealed" }],
    });
    let cited = json!({ "type": "web_search_result_location", "title": "Ducktape", "url": "https://ducktape.example", "encrypted_index": "Eo-index-sealed" });
    vec![
        Message::User {
            blocks: vec![UserBlock::Text {
                text: "look it up, then read both files".into(),
            }],
        },
        Message::Assistant {
            lane: lane(anthropic::PROVIDER, CLAUDE, "models", "claude-a"),
            blocks: vec![
                Block::Reasoning {
                    text: "claude thinks".into(),
                    sealed: Some("claude-signature".into()),
                },
                Block::Redacted {
                    sealed: "claude-redacted".into(),
                },
                Block::ServerToolUse {
                    id: "srvtoolu_1".into(),
                    name: "web_search".into(),
                    input: r#"{"query":"ducktape"}"#.into(),
                },
                Block::ServerToolResult {
                    sealed: searched.to_string(),
                },
                Block::Text {
                    text: "Ducktape is a network.".into(),
                    citations: vec![cited.to_string()],
                },
                Block::ToolUse {
                    id: "toolu_1".into(),
                    name: "files_read".into(),
                    input: r#"{"path":"/a"}"#.into(),
                },
            ],
        },
        Message::User {
            blocks: vec![result("toolu_1")],
        },
        Message::Assistant {
            lane: lane(codex::PROVIDER, GPT, "zk-codex", "codex-a"),
            blocks: vec![
                Block::Reasoning {
                    text: "codex plans".into(),
                    sealed: Some(reasoning_item().to_string()),
                },
                Block::ToolUse {
                    id: "call_1".into(),
                    name: "files_read".into(),
                    input: r#"{"path":"/b"}"#.into(),
                },
                Block::ToolUse {
                    id: "call|2".into(),
                    name: "files_read".into(),
                    input: r#"{"path":"/c"}"#.into(),
                },
            ],
        },
        Message::User {
            blocks: vec![result("call_1"), result("call|2")],
        },
    ]
}

fn request(model: &str, target: Target) -> Request {
    Request {
        model: model.into(),
        max_tokens: 4096,
        thinking: Some(Thinking::High),
        messages: conversation(),
        target,
        ..Request::default()
    }
}

fn pinned(service: &str, account: &str) -> Target {
    Target::Pinned {
        service: service.into(),
        account: account.into(),
    }
}

fn kinds(message: &Message) -> Vec<String> {
    let Message::Assistant { blocks, .. } = message else {
        return Vec::new();
    };
    let kind = |block: &Block| {
        let kind = match block {
            Block::Text { citations, .. } if citations.is_empty() => "text",
            Block::Text { .. } => "cited",
            Block::Reasoning { .. } => "reasoning",
            Block::Redacted { .. } => "redacted",
            Block::ToolUse { .. } => "tool_use",
            Block::ServerToolUse { .. } => "server_tool_use",
            Block::ServerToolResult { .. } => "server_tool_result",
            Block::Summary { .. } => "summary",
        };
        kind.to_owned()
    };
    blocks.iter().map(kind).collect()
}

fn shown(request: &Request) -> (Vec<String>, Vec<String>) {
    (kinds(&request.messages[1]), kinds(&request.messages[3]))
}

#[test]
fn claude_on_its_own_account_reads_its_own_blocks_whole_and_codexs_tool_calls_only() {
    let fitted = fit::fit(
        &request(CLAUDE, pinned("models", "claude-a")),
        anthropic::PROVIDER,
    );
    let claude = [
        "reasoning",
        "redacted",
        "server_tool_use",
        "server_tool_result",
        "cited",
        "tool_use",
    ];
    assert_eq!(
        shown(&fitted),
        (
            claude.map(String::from).to_vec(),
            vec!["tool_use".into(); 2]
        )
    );
    let body = anthropic::body(
        &request(CLAUDE, pinned("models", "claude-a")),
        anthropic::Ask::Turn,
    );
    let text = body.to_string();
    for sealed in [
        "claude-signature",
        "claude-redacted",
        "Eq-search-sealed",
        "Eo-index-sealed",
    ] {
        assert!(
            text.contains(sealed),
            "{sealed} goes back to its own account"
        );
    }
    for codex in ["gAAAA-codex-sealed", "codex plans"] {
        assert!(!text.contains(codex), "{codex} never reaches Claude");
    }
}

#[test]
fn claude_on_another_account_keeps_its_thinking_and_reads_the_search_as_words() {
    let fitted = fit::fit(
        &request(CLAUDE, pinned("models", "claude-b")),
        anthropic::PROVIDER,
    );
    let claude = ["reasoning", "redacted", "text", "text", "text", "tool_use"];
    assert_eq!(shown(&fitted).0, claude.map(String::from).to_vec());
    let body = anthropic::body(
        &request(CLAUDE, pinned("models", "claude-b")),
        anthropic::Ask::Turn,
    );
    let text = body.to_string();
    assert!(
        text.contains("claude-signature"),
        "a signature reads on any account of its model"
    );
    for sealed in ["Eq-search-sealed", "Eo-index-sealed"] {
        assert!(
            !text.contains(sealed),
            "{sealed} reads only on its own account"
        );
    }
    assert!(
        text.contains("https://ducktape.example"),
        "the search's pages go as words"
    );
    let unpinned = fit::fit(&request(CLAUDE, Target::Unpinned), anthropic::PROVIDER);
    assert_eq!(shown(&unpinned).0, claude.map(String::from).to_vec());
}

#[test]
fn another_claude_model_reads_no_thinking_and_codex_ids_fit_anthropics_pattern() {
    let fitted = fit::fit(
        &request("claude-sonnet-5", pinned("models", "claude-a")),
        anthropic::PROVIDER,
    );
    let claude = ["server_tool_use", "server_tool_result", "cited", "tool_use"];
    assert_eq!(shown(&fitted).0, claude.map(String::from).to_vec());
    let Message::Assistant { blocks, .. } = &fitted.messages[3] else {
        panic!("codex's turn");
    };
    let ids: Vec<&str> = blocks
        .iter()
        .filter_map(|block| match block {
            Block::ToolUse { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(ids, ["call_1", "call_7c2"]);
    let Message::User { blocks } = &fitted.messages[4] else {
        panic!("the results");
    };
    let calls: Vec<&str> = blocks
        .iter()
        .filter_map(|block| match block {
            UserBlock::Result { call, .. } => Some(call.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(calls, ids, "a result keeps pairing with its call");
}

#[test]
fn codex_on_its_own_account_reads_its_reasoning_and_claudes_turn_as_words_and_calls() {
    let asked = request(GPT, pinned("zk-codex", "codex-a"));
    let fitted = fit::fit(&asked, codex::PROVIDER);
    let claude = ["text", "text", "text", "tool_use"];
    let gpt = ["reasoning", "tool_use", "tool_use"];
    assert_eq!(
        shown(&fitted),
        (
            claude.map(String::from).to_vec(),
            gpt.map(String::from).to_vec()
        )
    );
    let body = codex::body(&asked);
    let text = body.to_string();
    assert!(
        text.contains("gAAAA-codex-sealed"),
        "its own reasoning goes back sealed"
    );
    let replayed = body["input"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| *item == reasoning_item());
    assert!(replayed, "{body}");
    for claude in [
        "claude-signature",
        "claude-redacted",
        "claude thinks",
        "Eq-search-sealed",
        "Eo-index-sealed",
    ] {
        assert!(!text.contains(claude), "{claude} never reaches Codex");
    }
    assert!(text.contains("Ducktape is a network."));
    let calls: Vec<&str> = body["input"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["type"] == "function_call")
        .filter_map(|item| item["call_id"].as_str())
        .collect();
    assert_eq!(calls, ["toolu_1", "call_1", "call|2"]);
}

#[test]
fn codex_on_another_account_or_unpinned_reads_no_encrypted_reasoning() {
    for target in [pinned("zk-codex", "codex-b"), Target::Unpinned] {
        let asked = request(GPT, target);
        let fitted = fit::fit(&asked, codex::PROVIDER);
        assert_eq!(shown(&fitted).1, ["tool_use", "tool_use"]);
        let text = codex::body(&asked).to_string();
        assert!(!text.contains("gAAAA-codex-sealed"), "{text}");
        assert!(!text.contains("codex plans"), "{text}");
    }
}

#[test]
fn a_harness_that_names_no_target_keeps_what_it_judged_and_no_account_scoped_reasoning() {
    let fitted = fit::fit(&request(CLAUDE, Target::Unnamed), anthropic::PROVIDER);
    let claude = [
        "reasoning",
        "redacted",
        "server_tool_use",
        "server_tool_result",
        "cited",
        "tool_use",
    ];
    assert_eq!(shown(&fitted).0, claude.map(String::from).to_vec());
    let asked = request(GPT, Target::Unnamed);
    assert_eq!(
        shown(&fit::fit(&asked, codex::PROVIDER)).1,
        ["tool_use", "tool_use"]
    );
    let text = codex::body(&asked).to_string();
    assert!(
        !text.contains("gAAAA-codex-sealed"),
        "no harness judged its account: {text}"
    );
}
