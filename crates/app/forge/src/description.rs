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
        Op::Grant {
            repo,
            principal: who,
        } => ("Grant writer", repo, vec![field("writer", principal(who))]),
        Op::Revoke {
            repo,
            principal: who,
        } => ("Revoke writer", repo, vec![field("writer", principal(who))]),
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

/// A principal as a describe field shows it: an account, or the system.
fn principal(principal: &Principal) -> Value {
    match principal {
        Principal::Account(number) => Value::Account(*number),
        Principal::Root => Value::text("system"),
    }
}
