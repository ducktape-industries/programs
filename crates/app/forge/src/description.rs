//! [`describe`]: an op as a person reads it, a title and its fields. The
//! source of the `ducktape.describe` module this program ships
//! (`make wasm-describes`); people show as their accounts.
use describe::{Description, Value, field};

use crate::{Op, Principal, Revision, Verdict};

/// An op as a person reads it: a title and its fields. The source of the
/// `ducktape.describe` module this program ships (`make wasm-describes`).
pub fn describe(op: &Op) -> Description {
    let text = |bytes: &[u8]| Value::Text(String::from_utf8_lossy(bytes).into_owned());
    let revision = |revision: &Revision| match revision {
        Revision::Ref(name) => text(name),
        Revision::Oid(oid) => Value::text(oid),
    };
    let change = |n: &u64| field("change", Value::Text(format!("#{n}")));
    let (verb, repo, fields) = match op {
        Op::Create { repo, hash } => (
            "Create",
            repo,
            vec![field(
                "hash",
                Value::text(match hash {
                    abi::HashKind::Sha256 => "SHA-256",
                    abi::HashKind::Sha1 => "SHA-1",
                }),
            )],
        ),
        Op::Configure { repo, settings } => (
            "Configure",
            repo,
            vec![
                field("head", text(&settings.head)),
                field("allow force", Value::Text(settings.allow_force.to_string())),
                field(
                    "allow delete",
                    Value::Text(settings.allow_delete.to_string()),
                ),
            ],
        ),
        Op::Push {
            repo,
            request,
            pack,
        } => {
            let mut fields = vec![field("request", Value::bytes(request))];
            if let Some(pack) = pack {
                fields.push(field("pack", Value::Hash(pack.digest().to_vec())));
            }
            ("Push", repo, fields)
        }
        Op::Merge {
            repo,
            into,
            from,
            result,
            change: n,
            ..
        } => (
            "Merge",
            repo,
            vec![
                field("from", revision(from)),
                field("into", text(into)),
                field("result", Value::text(result)),
                n.as_ref()
                    .map_or_else(|| field("change", Value::text("—")), change),
            ],
        ),
        Op::ChangeOpen {
            repo,
            from,
            into,
            title,
            reviewers,
            ..
        } => (
            "Open change",
            repo,
            vec![
                field("title", Value::text(title)),
                field("from", revision(from)),
                field("into", text(into)),
                field(
                    "reviewers",
                    Value::List(reviewers.iter().map(principal).collect()),
                ),
            ],
        ),
        Op::ChangeEdit {
            repo,
            n,
            title,
            reviewers,
            ..
        } => {
            let mut fields = vec![
                change(n),
                field(
                    "title",
                    Value::Text(title.clone().unwrap_or_else(|| "—".into())),
                ),
            ];
            if let Some(reviewers) = reviewers {
                let reviewers = reviewers.iter().map(principal).collect();
                fields.push(field("reviewers", Value::List(reviewers)));
            }
            ("Edit change", repo, fields)
        }
        Op::ChangeClose { repo, n } => ("Close change", repo, vec![change(n)]),
        Op::Stage { repo, pack } => (
            "Stage objects",
            repo,
            vec![field("pack", Value::Hash(pack.digest().to_vec()))],
        ),
        Op::SquashMerge {
            repo,
            into,
            from,
            change: n,
            ..
        } => (
            "Squash merge",
            repo,
            vec![
                field("from", revision(from)),
                field("into", text(into)),
                n.as_ref()
                    .map_or_else(|| field("change", Value::text("—")), change),
            ],
        ),
        Op::Rebase {
            repo,
            n,
            onto,
            expected_head,
        } => (
            "Rebase change",
            repo,
            vec![
                change(n),
                field("onto", revision(onto)),
                field("head", Value::text(expected_head)),
            ],
        ),
        Op::ChangeRepoint { repo, n, from } => (
            "Re-point change",
            repo,
            vec![change(n), field("from", revision(from))],
        ),
        Op::DeleteBranch {
            repo,
            branch,
            expected,
        } => (
            "Delete branch",
            repo,
            vec![
                field("branch", text(branch)),
                field("at", Value::text(expected)),
            ],
        ),
        Op::Protect {
            repo,
            branch,
            protected,
        } => (
            match protected {
                true => "Protect branch",
                false => "Unprotect branch",
            },
            repo,
            vec![field("branch", text(branch))],
        ),
        Op::Watch { deliver } => return watching(deliver),
        Op::ArchiveEnded { repo, after, limit } => {
            let after = match after {
                Some(n) => format!("#{n}"),
                None => "the first change".to_owned(),
            };
            (
                "Archive ended changes' chats",
                repo,
                vec![
                    field("after", Value::Text(after)),
                    field("changes", Value::count(*limit)),
                ],
            )
        }
        Op::Preview { repo, n } => ("Preview change", repo, vec![change(n)]),
        Op::Status {
            repo,
            commit,
            context,
            state,
            ..
        } => (
            "Report status",
            repo,
            vec![
                field("commit", Value::text(commit)),
                field("context", Value::text(context)),
                field("state", Value::text(state.word())),
            ],
        ),
        Op::RequireChecks {
            repo,
            branch,
            required,
        } => {
            let mut fields = vec![field("branch", text(branch))];
            if let Some(required) = required {
                fields.push(field("reporter", principal(&required.reporter)));
                let contexts = required.contexts.iter().map(Value::text).collect();
                fields.push(field("contexts", Value::List(contexts)));
            }
            let verb = match required {
                Some(_) => "Require checks",
                None => "Drop required checks",
            };
            (verb, repo, fields)
        }
        Op::Enqueue {
            repo,
            n,
            expected_head,
        } => (
            "Queue change",
            repo,
            vec![change(n), field("head", Value::text(expected_head))],
        ),
        Op::Dequeue { repo, n } => ("Take change out of the queue", repo, vec![change(n)]),
        Op::Dispatch {
            repo, event_type, ..
        } => (
            "Dispatch",
            repo,
            vec![field("event", Value::text(event_type))],
        ),
        Op::Archive { repo, archived } => {
            let verb = match archived {
                true => "Archive repository",
                false => "Un-archive repository",
            };
            (verb, repo, Vec::new())
        }
        Op::AdoptOwners { kind, .. } => {
            return Description {
                title: format!("Record the owners of {kind} items"),
                fields: vec![field("kind", Value::text(kind))],
            };
        }
        Op::ReviewSubmit { repo, n, review } => (
            "Review",
            repo,
            vec![
                change(n),
                field(
                    "verdict",
                    Value::text(match review.verdict {
                        Verdict::Approve => "approve",
                        Verdict::RequestChanges => "request changes",
                        Verdict::Comment => "comment",
                    }),
                ),
                field("commit", Value::text(&review.commit_oid)),
                field("comments", Value::Text(review.comments.len().to_string())),
            ],
        ),
    };
    let mut all = vec![field("repo", Value::text(repo))];
    all.extend(fields);
    Description {
        title: format!("{verb} · {repo}"),
        fields: all,
    }
}

describe::export!(Op, describe);

fn watching(deliver: &[u8]) -> Description {
    let title = match deliver.is_empty() {
        true => "Stop watching forge",
        false => "Watch forge",
    };
    Description {
        title: title.to_owned(),
        fields: vec![field("prefix", Value::count(deliver.len() as u64))],
    }
}

fn principal(principal: &Principal) -> Value {
    match principal {
        Principal::Account(number) => Value::Account(*number),
        Principal::Root => Value::text("system"),
    }
}
