//! [`describe`]: an op as a person reads it, a title and its fields. The
//! source of the `ducktape.describe` module this program ships
//! (`make wasm-describes`); the explorer shows it for every chat op.
use describe::{Description, Field, Value, field};

use crate::{Op, PostPolicy, Principal, dm_peers, plain_text};

pub fn describe(op: &Op) -> Description {
    let seq = |seq: &u64| field("seq", Value::text(seq.to_string()));
    let (title, channel, fields) = match op {
        Op::CreateChannel {
            channel_id,
            name,
            post_policy,
        } => (
            "Create channel",
            channel_id,
            vec![
                field("name", Value::text(name)),
                field(
                    "posting",
                    Value::text(match post_policy {
                        PostPolicy::Open => "open",
                        PostPolicy::MembersOnly => "members only",
                    }),
                ),
            ],
        ),
        Op::CreateDmChannel { counterpart, name } => {
            return Description {
                title: "Open a DM".into(),
                fields: vec![
                    field("with", Value::Account(*counterpart)),
                    field("name", Value::text(name)),
                ],
            };
        }
        Op::RenameChannel { channel_id, name } => (
            "Rename channel",
            channel_id,
            vec![field("name", Value::text(name))],
        ),
        Op::SetChannelArchived {
            channel_id,
            archived,
        } => (
            if *archived {
                "Archive channel"
            } else {
                "Unarchive channel"
            },
            channel_id,
            vec![],
        ),
        Op::PostMessage {
            channel_id,
            message_id,
            blocks,
            thread,
        } => {
            let mut fields = place(channel_id);
            fields.extend([
                field("text", Value::Text(plain_text(blocks))),
                field("message", Value::text(message_id)),
                field(
                    "thread",
                    Value::Text(thread.map_or_else(|| "—".into(), |t| t.to_string())),
                ),
            ]);
            return Description {
                title: match dm_peers(channel_id) {
                    Some(_) => "Direct message".into(),
                    None => format!("Post in #{channel_id}"),
                },
                fields,
            };
        }
        Op::EditMessage {
            channel_id,
            seq: at,
            blocks,
            ..
        } => (
            "Edit message",
            channel_id,
            vec![seq(at), field("text", Value::Text(plain_text(blocks)))],
        ),
        Op::DeleteMessage {
            channel_id,
            seq: at,
        } => ("Delete message", channel_id, vec![seq(at)]),
        Op::AddReaction {
            channel_id,
            seq: at,
            emoji,
        } => (
            "React",
            channel_id,
            vec![seq(at), field("emoji", Value::text(emoji))],
        ),
        Op::RemoveReaction {
            channel_id,
            seq: at,
            emoji,
        } => (
            "Remove reaction",
            channel_id,
            vec![seq(at), field("emoji", Value::text(emoji))],
        ),
        Op::SetMembership {
            channel_id,
            principal: who,
            member,
        } => (
            if *member {
                "Add member"
            } else {
                "Remove member"
            },
            channel_id,
            vec![field("member", principal(who))],
        ),
    };
    let mut all = place(channel);
    all.extend(fields);
    // a created channel's id is opaque; its name is what a person picked
    let shown = match op {
        Op::CreateChannel { name, .. } => format!("#{name}"),
        _ => room(channel),
    };
    Description {
        title: format!("{title} · {shown}"),
        fields: all,
    }
}

describe::export!(Op, describe);

/// A channel as `describe` titles it: `#design`, or `DM` for a dm room,
/// whose id is no name a person picked. Its two accounts are the
/// `between` field, drawn as accounts (a name, an avatar), never numbers.
fn room(channel_id: &str) -> String {
    match dm_peers(channel_id) {
        Some(_) => "DM".into(),
        None => format!("#{channel_id}"),
    }
}

/// The fields that say where an op happened: its `channel`, and for a dm
/// room the two accounts it is `between`.
fn place(channel_id: &str) -> Vec<Field> {
    let mut fields = vec![field("channel", Value::Text(room(channel_id)))];
    fields.extend(dm_peers(channel_id).map(|(a, b)| {
        field(
            "between",
            Value::List(vec![Value::Account(a), Value::Account(b)]),
        )
    }));
    fields
}

/// A principal as a describe field shows it: an account, or the system.
fn principal(principal: &Principal) -> Value {
    match principal {
        Principal::Account(number) => Value::Account(*number),
        Principal::Root => Value::text("system"),
    }
}
