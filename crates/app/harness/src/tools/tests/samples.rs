//! One or more inputs for each tool of the catalog, as a model writes them: objects, arrays and
//! nested values, optional fields given as null, and text with quotes, apostrophes, backslashes,
//! newlines, control and astral characters, and braces that look like JSON.
use serde_json::{Value, json};

const PROSE: &str =
    "it's \"done\", isn't it? {\"no\": 'json'} \\ back\\slash\n\tnext line \u{1} 🦆 é";
const CODE: &str = "fn main() {\n    println!(\"{}\", 'x');\n    let s = \"a\\\"b\";\n}\n";
const SNAPSHOT: &str = "98a5fde1bf22a27388b9315268fdf09513cbc3f645c46e6fe1d2ea2a0951c1d1";
const COMMIT: &str = "c1e03779c882b9fdeeb3eff411653c54c23fdd37";

pub(super) fn all() -> Vec<(&'static str, Vec<Value>)> {
    vec![
        (
            "files_list",
            vec![
                json!({ "path": "/shared/it's a dir/" }),
                json!({ "path": "/", "snapshot": SNAPSHOT, "cursor": "00ff" }),
                json!({ "path": "/home", "snapshot": null, "cursor": null }),
            ],
        ),
        (
            "files_read",
            vec![
                json!({ "path": "/home/tech/\"quoted\".md", "offset": 0, "length": 1048576 }),
                json!({ "path": "/a", "snapshot": SNAPSHOT, "offset": null, "length": null }),
            ],
        ),
        (
            "files_find",
            vec![json!({ "prefix": "/shared/jobs/J-066", "snapshot": null })],
        ),
        (
            "files_grep",
            vec![
                json!({ "pattern": PROSE, "prefix": "/shared/", "cursor": "0a0b" }),
                json!({ "pattern": "{\"Repo\":", "prefix": null }),
            ],
        ),
        ("files_history", vec![json!({}), json!({ "cursor": null })]),
        (
            "files_write",
            vec![
                json!({ "path": "/home/tech/x.rs", "content": CODE, "executable": true, "message": PROSE }),
                json!({
                    "path": "/home/tech/list",
                    "content": "",
                    "write": { "who": "listed", "accounts": [17, 21], "programs": ["harness"] },
                    "base": SNAPSHOT,
                    "executable": null,
                }),
            ],
        ),
        (
            "files_edit",
            vec![json!({
                "path": "/home/tech/x.rs",
                "edits": [
                    { "replace": "'x'", "with": "\"y\"" },
                    { "after": "{\n", "insert": "    // it's\n" },
                    { "before": "}", "insert": "\\" },
                    { "append": "\n🦆\n" },
                ],
                "base": null,
                "message": "edit: \"quotes\" and 'apostrophes'",
            })],
        ),
        (
            "files_set_list",
            vec![
                json!({ "path": "/shared/jobs", "write": { "who": "anyone" } }),
                json!({ "path": "/shared/jobs", "write": { "who": "listed", "accounts": [], "programs": [] } }),
                json!({ "path": "/shared/jobs", "write": null }),
            ],
        ),
        (
            "files_remove",
            vec![json!({ "path": "/home/tech/old 'one'", "base": SNAPSHOT, "message": null })],
        ),
        (
            "files_move",
            vec![json!({ "from": "/a/\"b\"", "to": "/a/c's", "base": null, "message": PROSE })],
        ),
        ("chat_channels", vec![json!({}), json!({ "cursor": "ab" })]),
        (
            "chat_messages",
            vec![json!({ "channel": "channel-18df9127f1a3eef4-1792310773949584", "cursor": null })],
        ),
        (
            "chat_thread",
            vec![json!({ "channel": "j-066", "root": 1, "cursor": null })],
        ),
        (
            "chat_search",
            vec![json!({ "text": "won't \"quote\"", "channel": null })],
        ),
        (
            "chat_post",
            vec![
                json!({ "channel": "j-066", "text": PROSE }),
                json!({ "channel": "j-066", "reply_to": 18446744073709551615u64, "text": "**md** `code` <@17> @chief\n\n```json\n{\"op\": [1, 2]}\n```" }),
                json!({ "channel": "j-066", "reply_to": null, "text": "" }),
            ],
        ),
        (
            "chat_react",
            vec![json!({ "channel": "j-066", "seq": 2, "emoji": "👀" })],
        ),
        ("forge_repos", vec![json!({ "cursor": null })]),
        (
            "forge_log",
            vec![
                json!({ "repo": "programs", "rev": "dev", "limit": 1, "cursor": null }),
                json!({ "repo": "programs", "rev": COMMIT }),
            ],
        ),
        (
            "forge_tree",
            vec![
                json!({ "repo": "programs", "rev": "refs/heads/j-066/it's", "path": "crates/app/harness/src/", "cursor": null }),
            ],
        ),
        (
            "forge_read",
            vec![
                json!({ "repo": "programs", "path": "README.md", "rev": "dev", "lines": [1, 40] }),
                json!({ "repo": "programs", "id": COMMIT, "offset": 0, "length": 100, "rev": null, "path": null, "lines": null }),
            ],
        ),
        (
            "forge_find",
            vec![json!({ "repo": "programs", "rev": null, "prefix": "crates/app/" })],
        ),
        (
            "forge_grep",
            vec![
                json!({ "repo": "programs", "rev": "dev", "pattern": "fn \"main\"('x')\\n", "prefix": null }),
            ],
        ),
        (
            "forge_changes",
            vec![
                json!({ "repo": "programs", "state": "open" }),
                json!({ "repo": "programs", "state": null, "cursor": null }),
            ],
        ),
        (
            "forge_change",
            vec![json!({ "repo": "programs", "number": 66 })],
        ),
        (
            "forge_commit",
            vec![
                json!({
                    "repo": "programs",
                    "branch": "j-066/catalog-test",
                    "from": "dev",
                    "files": [
                        { "path": "src/lib.rs", "content": CODE, "executable": false },
                        { "path": "docs", "source": "/home/tech/work/j-066/docs" },
                        { "path": "README.md", "edits": [
                            { "replace": "it's", "with": "it is" },
                            { "append": "\n\"end\"\n" },
                        ] },
                    ],
                    "remove": ["old/a's", "old/\"b\""],
                    "message": format!("j-066: {PROSE}"),
                    "base": COMMIT,
                    "snapshot": SNAPSHOT,
                }),
                json!({ "repo": "new", "files": [], "remove": ["x"], "message": "", "branch": null }),
            ],
        ),
        (
            "forge_change_open",
            vec![
                json!({
                    "repo": "programs",
                    "from": "j-066/catalog-test",
                    "into": "dev",
                    "title": "harness: \"every\" tool's input",
                    "body": PROSE,
                    "reviewers": [17],
                }),
                json!({ "repo": "programs", "from": COMMIT, "into": "refs/heads/dev", "title": "t" }),
            ],
        ),
        (
            "forge_change_comment",
            vec![json!({ "repo": "programs", "number": 1, "body": CODE })],
        ),
        (
            "forge_change_close",
            vec![json!({ "repo": "tools", "number": 1 })],
        ),
        ("network_accounts", vec![json!({ "cursor": null })]),
        ("network_account", vec![json!({ "number": 21 })]),
        (
            "network_activity",
            vec![json!({ "account": 21, "cursor": "00" })],
        ),
        ("network_schema", vec![json!({ "program": "forge" })]),
        (
            "network_query",
            vec![
                json!({ "program": "board", "query": { "Jobs": { "page": { "limit": 1 } } } }),
                json!({ "program": "files", "query": "Head" }),
                json!({ "program": "forge", "query": { "Tree": {
                "repo": "programs", "at": COMMIT, "path": { "utf8": "crates/it's" }, "page": { "after": null, "limit": null },
            } } }),
                json!({ "program": "chat", "query": { "Search": { "text": PROSE, "channel": null } } }),
                json!({ "program": "forge", "query": { "Log": {
                    "repo": "programs",
                    "from": { "Ref": { "utf8": "refs/heads/j-066/it's" } },
                    "exclude": { "Oid": COMMIT },
                    "page": { "after": null, "limit": 1 },
                } } }),
            ],
        ),
        (
            "network_call",
            vec![
                json!({ "program": "forge", "op": { "ChangeClose": { "repo": "tech-wake", "n": 999999 } } }),
                json!({ "program": "egress", "op": { "Request": {
                "request": { "Http": {
                    "service": "pods",
                    "head": { "method": "Post", "target": "/v1/pods", "headers": [["content-type", "application/json"]] },
                    "body": [{ "Sha256": SNAPSHOT }],
                } },
                "delivery": "Whole",
                "wait": null,
            } } }),
                json!({ "program": "chat", "op": { "PostMessage": { "channel_id": "j-066", "text": PROSE, "reply_to": null } } }),
                json!({ "program": "harness", "op": { "Book": { "height": 1, "target": "forge", "payload": [], "thread": null } } }),
                json!({ "program": "forge", "op": { "ChangeOpen": {
                    "repo": "programs",
                    "from": { "Ref": { "utf8": "refs/heads/j-066/utf8" } },
                    "into": { "utf8": "refs/heads/dev" },
                    "title": "nested \"utf8\" bytes",
                    "body": PROSE,
                    "reviewers": [{ "Account": 17 }],
                } } }),
            ],
        ),
        (
            "blob_put",
            vec![
                json!({ "kind": "part", "content": "{\"command\":[\"sh\",\"-c\",\"echo 'hi' \\\"there\\\"\"]}" }),
                json!({ "kind": "chunk", "source": "/home/tech/work/j-066/now.sh", "snapshot": SNAPSHOT, "hash": "sha1" }),
            ],
        ),
        (
            "pod_exec",
            vec![
                json!({ "pod": "duckpond-19154", "command": ["sh", "-c", "echo \"it's\" | sed 's/\\x27/\\\\/g'"] }),
                json!({ "pod": "duckpond-19154", "script": CODE, "container": "out", "every": 20 }),
                json!({ "pod": "duckpond-19154", "command": [], "script": null, "every": null }),
            ],
        ),
        (
            "pod_read",
            vec![
                json!({ "pod": "duckpond-19154", "path": "/out/it's \"x\".log", "offset": 0, "length": 4096, "container": null }),
            ],
        ),
        (
            "pod_push",
            vec![
                json!({ "pod": "duckpond-19154", "path": "/work/programs", "repo": "programs", "branch": "j-066/catalog-test", "ref": "HEAD", "container": "run" }),
            ],
        ),
        (
            "thread_search",
            vec![json!({ "pattern": "\"op\": \"{\\\"Repo\\\"", "frame": 3, "around": 80 })],
        ),
        (
            "thread_read",
            vec![json!({ "frame": 1, "offset": 0, "length": null })],
        ),
        (
            "thread_topic",
            vec![json!({ "topic": "J-066: it's \"streamed\" – 🦆" })],
        ),
        (
            "schedule_run",
            vec![
                json!({ "height": 106505, "program": "forge", "op": { "ChangeClose": { "repo": "tech-wake", "n": 999999 } } }),
                json!({ "height": 1, "program": "files", "op": { "Commit": {
                "base": null, "message": PROSE, "changes": [{ "Remove": { "path": "/x" } }],
            } } }),
            ],
        ),
        ("schedule_list", vec![json!({}), json!({ "cursor": null })]),
        ("schedule_cancel", vec![json!({ "entry": 82013 })]),
        (
            "wait_for",
            vec![
                json!({ "event": { "height": 106505 } }),
                json!({ "event": { "pod": { "name": "duckpond-32359" } } }),
                json!({ "event": { "message": { "channel": "j-051", "from": 17, "mentions_me": true } } }),
                json!({ "event": { "message": { "channel": "j-051" } } }),
                json!({ "event": { "change": { "repo": "programs", "n": 88 } } }),
                json!({ "event": { "ref": { "repo": "programs", "ref": "refs/heads/dev" } } }),
            ],
        ),
        ("wait_list", vec![json!({}), json!({ "cursor": null })]),
        ("wait_cancel", vec![json!({ "wait": 3 })]),
    ]
}
