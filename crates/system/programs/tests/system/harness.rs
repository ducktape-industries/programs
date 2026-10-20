//! harness on ducktape's host with the egress role bound: founding registers harness with
//! attribution, an agent is a program account whose script calls harness and names its
//! configuration file in files, which only its manager's account number writes, a founder node serves the model's service and the relay, and a
//! mention in chat, the model's turn and the agent's write each land in their own block while
//! the thread's loop waits parked; the next turn streams back through egress one event per
//! block and grows in the thread's partial reply until the stream ends.
use super::*;

use egress::{Answer, RELAY, Request, RequestRow, http};
use harness::{Body, Flow, Op, Partial, Query, Reply, Said, ThreadRow};
use serde_json::{Value, json};

const NODE: u64 = 1;
const ADA: u64 = 11;
const BO: u64 = 12;
const SERVICE: &str = "ccs";
const CONFIG: &str = "/home/Ada/agents/scout.md";

fn apps() -> Vec<Founding> {
    ["egress", "attribution", "harness", "files", "chat"]
        .into_iter()
        .map(|name| founding(name, &program(name)))
        .collect()
}

fn with_forge() -> Vec<Founding> {
    let mut apps = apps();
    apps.push(Founding {
        program: forge::PROGRAM.to_owned(),
        code: program("forge"),
        params: abi::encode(&super::discussion_tests::bounds()),
        roles: Vec::new(),
    });
    apps
}

fn leb(mut value: usize) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        match value {
            0 => {
                out.push(byte);
                return out;
            }
            _ => out.push(byte | 0x80),
        }
    }
}

fn with_schema(mut code: Vec<u8>, schema: &borsh::schema::BorshSchemaContainer) -> Vec<u8> {
    let name = program::schema::SECTION.as_bytes();
    let mut section = leb(name.len());
    section.extend(name);
    section.extend(borsh::to_vec(schema).unwrap());
    code.push(0);
    code.extend(leb(section.len()));
    code.extend(section);
    code
}

fn schemed_apps() -> Vec<Founding> {
    let mut apps = apps();
    for app in &mut apps {
        if app.program == chat::PROGRAM {
            let schema = program::schema::interface::<chat::Chat>();
            app.code = with_schema(app.code.clone(), &schema);
        }
    }
    apps
}

impl Net {
    async fn assigned(&self, service: &str) -> Vec<RequestRow> {
        let asked = egress::Query::Assigned { key: public(NODE) };
        let egress::Reply::Requests(rows) = self.ask(egress::PROGRAM, &asked).await else {
            panic!()
        };
        rows.into_iter()
            .filter(|row| row.request.service() == service)
            .collect()
    }

    async fn joined(&self, row: &RequestRow) -> Value {
        let mut body = Vec::new();
        for part in row.request.parts() {
            let framed = self.blob(part).await.expect("a part is on the network");
            let start = framed.iter().position(|byte| *byte == 0).unwrap() + 1;
            body.extend(&framed[start..]);
        }
        serde_json::from_slice(&body).expect("the parts join into one JSON request")
    }

    async fn answer(&mut self, id: u64, output: Vec<u8>) {
        let op = egress::Op::Answer {
            id,
            answer: Answer::Output(output),
        };
        self.apply(&public(NODE), egress::PROGRAM, &op).await;
    }

    async fn stream(&mut self, id: u64, events: &[Value], seqs: std::ops::Range<usize>) {
        let frame = seqs
            .map(|seq| {
                let event = &events[seq];
                let data = format!("event: {}\ndata: {event}", event["type"].as_str().unwrap());
                egress::Event {
                    id,
                    seq: seq as u64,
                    data: data.into_bytes(),
                }
            })
            .collect();
        self.apply(&public(NODE), egress::PROGRAM, &egress::Op::Events(frame))
            .await;
    }

    async fn end_stream(&mut self, id: u64, events: u64) {
        let op = egress::Op::EndStream {
            id,
            events,
            end: egress::StreamEnd::Complete {
                status: 200,
                headers: vec![("content-type".into(), "text/event-stream".into())],
            },
        };
        self.apply(&public(NODE), egress::PROGRAM, &op).await;
    }

    async fn partial(&self, thread: u64) -> Option<Partial> {
        let Reply::Partial(partial) = self.ask(harness::PROGRAM, &Query::Partial { thread }).await
        else {
            panic!()
        };
        partial
    }

    async fn threads(&self, agent: u64) -> Vec<ThreadRow> {
        let asked = Query::Threads {
            agent,
            page: PageRequest::default(),
        };
        let Reply::Threads(page) = self.ask(harness::PROGRAM, &asked).await else {
            panic!()
        };
        page.items
    }

    async fn frames(&self, thread: u64) -> Vec<Body> {
        let asked = Query::Frames {
            thread,
            page: PageRequest::default(),
        };
        let Reply::Frames(page) = self.ask(harness::PROGRAM, &asked).await else {
            panic!()
        };
        page.items.into_iter().map(|frame| frame.body).collect()
    }
}

fn model_reply(content: Value, stop: &str) -> Vec<u8> {
    let body = json!({
        "model": "claude-test",
        "content": content,
        "stop_reason": stop,
        "usage": { "input_tokens": 1, "output_tokens": 1 },
    });
    abi::encode(&http::Response {
        status: 200,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    })
}

// the pin the agent's file names: the provider whose route reaches its model
fn provided() -> harness::Pin {
    harness::Pin {
        service: None,
        provider: Some("anthropic".into()),
    }
}

async fn configured(net: &mut Net, config: &harness::Config) -> Vec<files::Change> {
    vec![files::Change::Put {
        path: CONFIG.into(),
        exec: false,
        meta: Default::default(),
        content: net.chunks(config.write(&provided()).as_bytes()).await,
    }]
}

async fn kept(net: &mut Net, key: &[u8]) {
    let owner_only = access::Op::Set {
        program: files::PROGRAM.into(),
        key: CONFIG.into(),
        list: Some(files::Ownership::Owner),
    };
    net.apply(key, access::PROGRAM, &owner_only).await;
}

struct Scout {
    agent: u64,
    snapshot: BlobId,
    thread: ThreadRow,
    asked: RequestRow,
}

async fn mentioned(net: &mut Net) -> Scout {
    let serve = egress::Op::Serve {
        services: vec![SERVICE.into(), RELAY.into()],
    };
    net.apply(&public(NODE), egress::PROGRAM, &serve).await;
    let create = identity::Op::Create {
        name: "Ada".into(),
        scheme: Scheme::Ed25519,
    };
    net.apply(&public(ADA), identity::PROGRAM, &create).await;
    let register = egress::Op::Register {
        service: SERVICE.into(),
    };
    net.apply(&public(ADA), egress::PROGRAM, &register).await;
    let assign = egress::Op::Assign {
        service: SERVICE.into(),
        nodes: vec![public(NODE)],
    };
    net.apply(&public(ADA), egress::PROGRAM, &assign).await;
    let create = identity::Op::CreateAgent {
        name: "Scout".into(),
    };
    let agent: u64 =
        abi::decode(&net.apply(&public(ADA), identity::PROGRAM, &create).await).unwrap();
    let config = harness::Config {
        model: "claude-test".into(),
        max_tokens: 512,
        wait: None,
        thinking: None,
        server_tools: Vec::new(),
        listen: Vec::new(),
        settle: None,
        compact_at: None,
        system: "Keep notes in /shared.".into(),
    };
    let commit = files::Op::Commit {
        base: None,
        message: "configure Scout".into(),
        changes: configured(net, &config).await,
    };
    let snapshot: BlobId =
        abi::decode(&net.apply(&public(ADA), files::PROGRAM, &commit).await).unwrap();
    kept(net, &public(ADA)).await;
    let script = identity::Op::SetScript {
        account: agent,
        script: Some(identity::Script {
            calls: harness::PROGRAM.into(),
            config: CONFIG.into(),
        }),
    };
    net.apply(&public(ADA), identity::PROGRAM, &script).await;
    let route = Op::SetRoute {
        provider: "anthropic".into(),
        services: vec![harness::RouteService {
            service: SERVICE.into(),
            wire: harness::Wire::AnthropicMessages,
        }],
    };
    net.apply(&public(ADA), harness::PROGRAM, &route).await;
    let general = chat::Op::CreateChannel {
        channel_id: "general".into(),
        name: "general".into(),
        post_policy: chat::PostPolicy::Open,
    };
    net.apply(&public(ADA), chat::PROGRAM, &general).await;
    let mention = chat::Op::PostMessage {
        channel_id: "general".into(),
        message_id: "m1".into(),
        blocks: chat::parse_message(&format!("<@{agent}> note that the network is up")),
        thread: None,
    };
    net.apply(&public(ADA), chat::PROGRAM, &mention).await;

    let [thread] = net.threads(agent).await.try_into().unwrap();
    let [models] = net.assigned(SERVICE).await.try_into().unwrap();
    let Request::Http { head, .. } = &models.request else {
        panic!("the model is asked over http");
    };
    assert_eq!(head.target, "/v1/models/claude-test");
    let info = json!({ "type": "model", "id": "claude-test", "max_input_tokens": 200_000 });
    let window = abi::encode(&http::Response {
        status: 200,
        headers: Vec::new(),
        body: info.to_string().into_bytes(),
    });
    net.answer(models.id, window).await;
    let [asked] = net.assigned(SERVICE).await.try_into().unwrap();
    Scout {
        agent,
        snapshot,
        thread,
        asked,
    }
}

#[test]
fn an_agents_configuration_is_its_managers_to_change_whoever_takes_the_managers_name() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found_with(context, dir.path(), apps(), Vec::new(), MEMBER_CAP).await;
        let named = |name: &str| identity::Op::Create {
            name: name.into(),
            scheme: Scheme::Ed25519,
        };
        let ada: u64 = abi::decode(
            &net.apply(&public(ADA), identity::PROGRAM, &named("Ada"))
                .await,
        )
        .unwrap();
        let bo: u64 = abi::decode(
            &net.apply(&public(BO), identity::PROGRAM, &named("Bo"))
                .await,
        )
        .unwrap();
        let config = harness::Config {
            model: "claude-test".into(),
            max_tokens: 512,
            wait: None,
            thinking: None,
            server_tools: Vec::new(),
            listen: Vec::new(),
            settle: None,
            compact_at: None,
            system: "Mine.".into(),
        };
        let commit = |base, changes| files::Op::Commit {
            base,
            message: "configure Scout".into(),
            changes,
        };
        let changes = configured(&mut net, &config).await;
        let first: BlobId = abi::decode(
            &net.apply(&public(ADA), files::PROGRAM, &commit(None, changes))
                .await,
        )
        .unwrap();
        kept(&mut net, &public(ADA)).await;
        let asked = access::Query::Writers {
            program: files::PROGRAM.into(),
            key: CONFIG.into(),
        };
        let access::Reply::Writers(writers) = net.ask(access::PROGRAM, &asked).await else {
            panic!()
        };
        assert_eq!(
            writers,
            ::program::Writers::Listed {
                at: CONFIG.into(),
                owner: Some(files::Party::Account(ada)),
                list: files::Ownership::Owner,
            }
        );

        let rename = |account, name: &str| identity::Op::SetName {
            account,
            name: name.into(),
        };
        net.apply(&public(ADA), identity::PROGRAM, &rename(ada, "Ada L"))
            .await;
        net.apply(&public(BO), identity::PROGRAM, &rename(bo, "Ada"))
            .await;
        let theirs = net.chunks(b"theirs").await;
        let theirs = commit(
            Some(first),
            vec![files::Change::Put {
                path: CONFIG.into(),
                exec: false,
                meta: Default::default(),
                content: theirs,
            }],
        );
        assert_eq!(
            net.refuse(&public(BO), files::PROGRAM, &theirs).await,
            reason::UNAUTHORIZED
        );
        let changes = configured(&mut net, &config).await;
        net.apply(&public(ADA), files::PROGRAM, &commit(Some(first), changes))
            .await;
    });
}

#[test]
fn a_mentioned_agent_answers_through_elected_nodes_and_writes_as_itself() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found_with(context, dir.path(), apps(), Vec::new(), MEMBER_CAP).await;
        let attribution::Reply::Registered(registered) = net
            .ask(attribution::PROGRAM, &attribution::Query::Registered)
            .await
        else {
            panic!()
        };
        assert_eq!(
            registered,
            [harness::PROGRAM.to_owned()],
            "founding ran harness's init, which registered it with attribution"
        );
        let Scout {
            agent,
            snapshot,
            thread,
            asked,
        } = mentioned(&mut net).await;
        let sent = net.joined(&asked).await;
        assert_eq!(sent["max_tokens"], 512);
        let text = sent["messages"][0]["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("mentioned you in chat channel general"),
            "{text}"
        );
        assert!(text.ends_with("note that the network is up"), "{text}");
        assert_eq!(sent["system"][0]["text"], "Keep notes in /shared.");
        assert!(
            sent["system"][1]["text"]
                .as_str()
                .unwrap()
                .starts_with("ducktape resource manual\n- Account: Scout, account ")
        );

        let write = json!([{
            "type": "tool_use",
            "id": "toolu_1",
            "name": "files_write",
            "input": { "path": "/shared/status.md", "content": "up" },
        }]);
        net.answer(asked.id, model_reply(write, "tool_use")).await;
        let frames = net.frames(thread.id).await;
        assert_eq!(frames.len(), 2);
        assert!(
            matches!(&frames[1], Body::Turn { config, .. } if *config == snapshot),
            "the turn names the configuration snapshot its call read"
        );
        let [relay] = net.assigned(RELAY).await.try_into().unwrap();
        assert!(net.assigned(SERVICE).await.is_empty());
        assert_eq!(net.parked(), 1, "the thread's loop waits parked");

        net.answer(relay.id, Vec::new()).await;
        let asked = files::Query::Read {
            path: "/shared/status.md".into(),
            at: None,
            offset: 0,
            len: 16,
        };
        let files::Reply::Contents(contents) = net.ask(files::PROGRAM, &asked).await else {
            panic!()
        };
        assert_eq!(contents.bytes, b"up");
        let files::Reply::Head(Some(head)) = net.ask(files::PROGRAM, &files::Query::Head).await
        else {
            panic!()
        };
        assert_eq!(
            head.author,
            program::Principal::Account(agent),
            "the agent is the writer"
        );

        let [next] = net.assigned(SERVICE).await.try_into().unwrap();
        let messages = net.joined(&next).await["messages"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(messages.len(), 3);
        let result = &messages[2]["content"][0];
        assert_eq!(
            (&result["type"], &result["tool_use_id"], &result["is_error"]),
            (&json!("tool_result"), &json!("toolu_1"), &json!(false))
        );
        assert_eq!(
            next.delivery,
            egress::Delivery::Events {
                deliver: harness::streamed_as(thread.id)
            }
        );
        let start = json!({
            "type": "message_start",
            "message": {
                "model": "claude-test",
                "content": [],
                "usage": { "input_tokens": 1, "output_tokens": 1 },
            },
        });
        let block = json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": { "type": "text", "text": "" },
        });
        let delta = |text: &str| {
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": { "type": "text_delta", "text": text },
            })
        };
        let events = [
            start,
            block,
            delta("Not"),
            delta("ed."),
            json!({ "type": "content_block_stop", "index": 0 }),
            json!({
                "type": "message_delta",
                "delta": { "stop_reason": "end_turn" },
                "usage": { "output_tokens": 2 },
            }),
            json!({ "type": "message_stop" }),
        ];
        net.stream(next.id, &events, 0..3).await;
        let growing = net.partial(thread.id).await.unwrap();
        assert_eq!(
            (growing.said, growing.flow),
            (vec![Said::Text("Not".into())], Flow::Open)
        );
        net.stream(next.id, &events, 3..events.len()).await;
        assert_eq!(net.parked(), 1, "the loop waits until the stream ends");
        net.end_stream(next.id, events.len() as u64).await;
        assert_eq!(net.partial(thread.id).await, None);
        let frames = net.frames(thread.id).await;
        let Some(Body::Turn { said, usage, .. }) = frames.last() else {
            panic!("the streamed reply is the thread's last turn: {frames:?}");
        };
        assert_eq!(said, &[Said::Text("Noted.".into())]);
        assert_eq!((usage.input, usage.output), (1, 2));
        assert!(net.assigned(SERVICE).await.is_empty());
        assert_eq!(net.parked(), 0, "the loop finished");
    });
}

#[test]
fn an_agent_queries_and_calls_a_program_through_the_schema_its_code_carries() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net =
            Net::found_with(context, dir.path(), schemed_apps(), Vec::new(), MEMBER_CAP).await;
        let Scout {
            agent,
            thread,
            asked,
            ..
        } = mentioned(&mut net).await;
        let post = json!({ "PostMessage": {
            "channel_id": "general",
            "message_id": "net-1",
            "blocks": [{ "Paragraph": [{ "text": "hello", "marks": [] }] }],
            "thread": null,
        } });
        let calls = json!([
            { "type": "tool_use", "id": "toolu_s", "name": "network_schema", "input": { "program": "chat", "whole": true } },
            { "type": "tool_use", "id": "toolu_q", "name": "network_query", "input": { "program": "chat", "query": { "Channel": { "channel_id": "general" } } } },
            { "type": "tool_use", "id": "toolu_c", "name": "network_call", "input": { "program": "chat", "op": post } },
            { "type": "tool_use", "id": "toolu_f", "name": "network_schema", "input": { "program": "files" } },
        ]);
        net.answer(asked.id, model_reply(calls, "tool_use")).await;
        for relay in net.assigned(RELAY).await {
            net.answer(relay.id, Vec::new()).await;
        }
        let results: Vec<(String, String, bool)> = net
            .frames(thread.id)
            .await
            .into_iter()
            .filter_map(|body| match body {
                Body::Result {
                    call, text, failed, ..
                } => Some((call, text, failed)),
                _ => None,
            })
            .collect();
        let result = |id: &str| {
            results
                .iter()
                .find(|(call, _, _)| call == id)
                .map(|(_, text, failed)| (text.clone(), *failed))
                .unwrap_or_else(|| panic!("{id} has a result: {results:?}"))
        };
        let (shape, failed) = result("toolu_s");
        assert!(!failed, "{shape}");
        let shape: Value = serde_json::from_str(&shape).unwrap();
        assert_eq!(shape["types"]["Op"]["enum"]["PostMessage"]["struct"]["thread"], json!({ "option": "u64" }));
        let (channel, failed) = result("toolu_q");
        assert!(!failed, "{channel}");
        let channel: Value = serde_json::from_str(&channel).unwrap();
        assert_eq!(channel["reply"]["Channel"]["channel"]["name"], "general");
        let (posted, failed) = result("toolu_c");
        assert!(!failed, "{posted}");
        let asked = chat::Query::MessageById {
            message_id: "net-1".into(),
        };
        let chat::Reply::Message(Some(row)) = net.ask(chat::PROGRAM, &asked).await else {
            panic!("the agent's post landed")
        };
        assert_eq!(row.author, program::Principal::Account(agent));
        assert_eq!(row.text, "hello");
        let (unschemed, failed) = result("toolu_f");
        assert!(failed && unschemed.starts_with(reason::UNSUPPORTED), "{unschemed}");
    });
}

#[test]
fn a_mentioned_agent_commits_files_to_forge_as_itself() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net =
            Net::found_with(context, dir.path(), with_forge(), Vec::new(), MEMBER_CAP).await;
        let Scout {
            agent,
            thread,
            asked,
            ..
        } = mentioned(&mut net).await;
        let commit = json!([{
            "type": "tool_use",
            "id": "toolu_1",
            "name": "forge_commit",
            "input": {
                "repo": "notes",
                "files": [{ "path": "status.md", "content": "up\n" }],
                "message": "Status",
            },
        }]);
        net.answer(asked.id, model_reply(commit, "tool_use")).await;
        let [relay] = net.assigned(RELAY).await.try_into().unwrap();
        net.answer(relay.id, Vec::new()).await;
        let frames = net.frames(thread.id).await;
        let Some(Body::Result { text, failed, .. }) = frames.last() else {
            panic!("the call's result is the thread's last frame: {frames:?}");
        };
        assert!(!failed, "{text}");

        let repo = forge::Query::Repo {
            repo: "notes".into(),
        };
        let forge::Reply::Repo { repo, .. } = net.ask(forge::PROGRAM, &repo).await else {
            panic!()
        };
        assert_eq!(
            repo.repo.owner,
            program::Principal::Account(agent),
            "the agent owns the repository its commit created"
        );
        let log = forge::Query::Log {
            repo: "notes".into(),
            from: forge::Revision::Ref(b"refs/heads/main".to_vec()),
            exclude: None,
            page: forge::PageRequest::first(4),
        };
        let forge::Reply::Log { page, .. } = net.ask(forge::PROGRAM, &log).await else {
            panic!()
        };
        let [commit] = page.items.try_into().unwrap();
        assert_eq!(
            (commit.author.name.as_slice(), commit.message.as_slice()),
            (b"Scout".as_slice(), b"Status\n".as_slice())
        );
        let tree = forge::Query::Tree {
            repo: "notes".into(),
            at: commit.oid,
            path: Vec::new(),
            page: forge::PageRequest::first(4),
        };
        let forge::Reply::Tree { page, .. } = net.ask(forge::PROGRAM, &tree).await else {
            panic!()
        };
        let [entry] = page.items.try_into().unwrap();
        assert_eq!(entry.name, b"status.md");
        let blob = forge::Query::Blob {
            repo: "notes".into(),
            oid: entry.oid,
            range: None,
        };
        let forge::Reply::Blob { blob, .. } = net.ask(forge::PROGRAM, &blob).await else {
            panic!()
        };
        assert_eq!(blob.bytes, b"up\n");
    });
}

async fn drafts(net: &Net, channel_id: &str) -> Vec<chat::Draft> {
    let asked = chat::Query::Drafts {
        channel_id: channel_id.into(),
    };
    let chat::Reply::Drafts(drafts) = net.ask(chat::PROGRAM, &asked).await else {
        panic!()
    };
    drafts
}

async fn message(net: &Net, message_id: &str) -> Option<chat::MsgRow> {
    let asked = chat::Query::MessageById {
        message_id: message_id.into(),
    };
    let chat::Reply::Message(row) = net.ask(chat::PROGRAM, &asked).await else {
        panic!()
    };
    row
}

#[test]
fn a_streamed_post_forms_in_the_draft_lands_whole_and_a_mention_in_it_is_heard_once() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found_with(context, dir.path(), apps(), Vec::new(), MEMBER_CAP).await;
        let Scout {
            agent,
            thread,
            asked,
            ..
        } = mentioned(&mut net).await;
        let create = identity::Op::CreateAgent {
            name: "Echo".into(),
        };
        let echo: u64 =
            abi::decode(&net.apply(&public(ADA), identity::PROGRAM, &create).await).unwrap();
        let script = identity::Op::SetScript {
            account: echo,
            script: Some(identity::Script {
                calls: harness::PROGRAM.into(),
                config: CONFIG.into(),
            }),
        };
        net.apply(&public(ADA), identity::PROGRAM, &script).await;

        let piece = |json: String| {
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": { "type": "input_json_delta", "partial_json": json },
            })
        };
        let events = [
            json!({
                "type": "message_start",
                "message": {
                    "model": "claude-test",
                    "content": [],
                    "usage": { "input_tokens": 1, "output_tokens": 1 },
                },
            }),
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": { "type": "tool_use", "id": "toolu_p", "name": "chat_post", "input": {} },
            }),
            piece(format!(r#"{{"channel":"general","text":"<@{echo}> pl"#)),
            piece(r#"ease check"}"#.into()),
            json!({ "type": "content_block_stop", "index": 0 }),
            json!({
                "type": "message_delta",
                "delta": { "stop_reason": "tool_use" },
                "usage": { "output_tokens": 2 },
            }),
            json!({ "type": "message_stop" }),
        ];
        let id = format!("harness-{}-toolu_p", thread.id);
        net.stream(asked.id, &events, 0..3).await;
        assert_eq!(message(&net, &id).await, None, "nothing posts while it forms");
        let [draft] = &drafts(&net, "general").await[..] else {
            panic!("the agent's draft in the room");
        };
        assert_eq!(draft.author, program::Principal::Account(agent));
        assert!(draft.text.contains(&format!("<@{echo}> pl")), "{draft:?}");
        assert!(net.threads(echo).await.is_empty(), "a post still forming wakes no one");

        net.stream(asked.id, &events, 3..5).await;
        let [relay] = net.assigned(RELAY).await.try_into().unwrap();
        net.stream(asked.id, &events, 5..events.len()).await;
        net.end_stream(asked.id, events.len() as u64).await;
        net.answer(relay.id, Vec::new()).await;
        let whole = message(&net, &id).await.expect("the post lands once its call runs");
        assert_eq!(whole.author, program::Principal::Account(agent));
        assert_eq!(
            whole.blocks,
            chat::parse_message(&format!("<@{echo}> please check"))
        );
        let frames = net.frames(thread.id).await;
        let [.., Body::Turn { .. }, Body::Result { call, failed, .. }] = &frames[..] else {
            panic!("the turn, then its result: {frames:?}");
        };
        assert_eq!((call.as_str(), *failed), ("toolu_p", false));

        let [heard] = net.threads(echo).await.try_into().unwrap();
        let posts: Vec<String> = net
            .frames(heard.id)
            .await
            .into_iter()
            .filter_map(|body| match body {
                Body::Post { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        let [post] = &posts[..] else {
            panic!("heard once: {posts:?}");
        };
        assert!(post.ends_with("please check"), "{post}");
    });
}
