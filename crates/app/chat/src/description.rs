//! [`describe`]: an op as a person reads it, a title and its fields. The
//! source of the `ducktape.describe` module this program ships
//! (`make wasm-describes`); the explorer shows it for every chat op.
use std::collections::BTreeMap;

use describe::{Description, Field, Value, field};

use crate::{
    Notices, Op, PeerMessage, PeerWrite, PostPolicy, Principal, dm_peers, hex, notice_text,
    plain_text,
};

fn noticed(
    channel_id: &str,
    message_id: &str,
    thread: &Option<u64>,
    notice: &crate::Notice,
    text: Option<&str>,
) -> Description {
    let body = text
        .map(|text| plain_text(&crate::parse_message(text)))
        .filter(|body| !body.is_empty());
    let said = match body {
        Some(body) => format!("{} {body}", notice_text(notice)),
        None => notice_text(notice),
    };
    let mut described = posted(channel_id, message_id, said, thread);
    described.title = format!("Notice in {}", room_title(channel_id));
    let concerns = notice
        .concerns
        .iter()
        .map(|account| Value::Account(*account));
    described
        .fields
        .push(field("for", Value::List(concerns.collect())));
    described
}

pub fn describe(op: &Op) -> Description {
    let seq = |seq: &u64| field("seq", Value::text(seq.to_string()));
    let (title, channel, fields) = match op {
        Op::CreateChannel {
            channel_id,
            name,
            post_policy,
        } => ("Create channel", channel_id, created(name, post_policy)),
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
        } => return posted(channel_id, message_id, plain_text(blocks), thread),
        Op::Post {
            channel_id,
            message_id,
            text,
            thread,
        } => {
            let text = plain_text(&crate::parse_message(text));
            return posted(channel_id, message_id, text, thread);
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
            vec![field("member", principal_description(who))],
        ),
        Op::CreateChannelWithMeta {
            channel_id,
            name,
            post_policy,
            meta,
        } => {
            let mut fields = created(name, post_policy);
            fields.push(field("meta", entries(meta)));
            ("Create channel", channel_id, fields)
        }
        Op::SetChannelMeta {
            channel_id,
            key,
            value,
        } => (
            "Set channel meta",
            channel_id,
            vec![
                field("key", Value::text(key)),
                field("value", Value::text(value)),
            ],
        ),
        Op::RemoveChannelMeta { channel_id, key } => (
            "Remove channel meta",
            channel_id,
            vec![field("key", Value::text(key))],
        ),
        Op::Listen {
            channel_id,
            listening,
        } => (
            if *listening {
                "Listen to channel"
            } else {
                "Stop listening to channel"
            },
            channel_id,
            vec![],
        ),
        Op::SetNotices {
            channel_id,
            notices,
        } => (
            match notices {
                None => "Default notices for channel",
                Some(Notices::Mentions) => "Notices of mentions only for channel",
                Some(Notices::Off) => "Turn off notices for channel",
            },
            channel_id,
            vec![],
        ),
        Op::AdoptOwners { kind, .. } => {
            return Description {
                title: format!("Record the owners of {kind} items"),
                fields: vec![field("kind", Value::text(kind))],
            };
        }
        Op::Watch {
            channel_id,
            watching,
        } => (
            if *watching {
                "Watch channel for a wait"
            } else {
                "Stop watching channel"
            },
            channel_id,
            vec![],
        ),
        Op::Drafting {
            channel_id,
            text,
            doing,
            ..
        } => {
            let mut fields = vec![field("text", Value::text(text))];
            fields.extend(doing.iter().map(|doing| field("doing", Value::text(doing))));
            ("Answering", channel_id, fields)
        }
        Op::Undraft { channel_id } => ("Stop answering", channel_id, vec![]),
        Op::ChooseForMany {
            accounts,
            channel_id,
            listening,
            notices,
        } => (
            match (*listening, notices) {
                (true, _) => "Listen to channel for accounts",
                (false, None) => "Default notices for accounts in channel",
                (false, Some(Notices::Mentions)) => {
                    "Notices of mentions only for accounts in channel"
                }
                (false, Some(Notices::Off)) => "Turn off notices for accounts in channel",
            },
            channel_id,
            vec![field(
                "accounts",
                Value::List(
                    accounts
                        .iter()
                        .map(|account| Value::Account(*account))
                        .collect(),
                ),
            )],
        ),
        Op::Notify {
            channel_id,
            message_id,
            thread,
            notice,
        } => return noticed(channel_id, message_id, thread, notice, None),
        Op::NotifyWith {
            channel_id,
            message_id,
            thread,
            notice,
            by,
            text,
        } => {
            let mut described = noticed(channel_id, message_id, thread, notice, text.as_deref());
            if let Some(by) = by {
                described
                    .fields
                    .push(field("by", principal_description(by)));
            }
            return described;
        }
        Op::Take { channel_id, thread } => ("Hold place", channel_id, vec![under(*thread)]),
        Op::Release { channel_id, thread } => ("Let go of place", channel_id, vec![under(*thread)]),
        Op::Break { channel_id, thread } => ("Clear place", channel_id, vec![under(*thread)]),
        Op::SetPostPolicy {
            channel_id,
            post_policy,
        } => (
            "Set posting",
            channel_id,
            vec![field("posting", Value::text(posting(post_policy)))],
        ),
        Op::WatchPlaces { deliver } => {
            let title = match deliver.is_empty() {
                true => "Stop hearing when places clear",
                false => "Hear when places clear",
            };
            return Description {
                title: title.into(),
                fields: Vec::new(),
            };
        }
        Op::Link { channel_id, peer } => match peer {
            Some(peer) => (
                "Link channel",
                channel_id,
                vec![
                    field("network", Value::text(hex(&peer.network))),
                    field("peer", Value::text(format!("#{}", peer.channel_id))),
                ],
            ),
            None => ("Unlink channel", channel_id, vec![]),
        },
        Op::PublishDeviceKey { key, .. } => {
            return Description {
                title: "Publish a device key".into(),
                fields: vec![field("key", Value::text(hex(key)))],
            };
        }
        Op::RetireDeviceKey { key } => {
            return Description {
                title: "Retire a device key".into(),
                fields: vec![field("key", Value::text(hex(key)))],
            };
        }
        Op::Seal { channel_id, wraps } => (
            "Seal conversation",
            channel_id,
            vec![field("devices", Value::text(wraps.len().to_string()))],
        ),
        Op::PostSealed {
            channel_id,
            message_id,
            thread,
            epoch,
            ..
        } => (
            "Sealed message",
            channel_id,
            vec![
                field("message", Value::text(message_id)),
                field("epoch", Value::text(epoch.to_string())),
                under(*thread),
            ],
        ),
        Op::Rekey {
            channel_id,
            epoch,
            wraps,
        } => (
            "Rekey conversation",
            channel_id,
            vec![
                field("epoch", Value::text(epoch.to_string())),
                field("devices", Value::text(wraps.len().to_string())),
            ],
        ),
        Op::Wrap { channel_id, wraps } => (
            "Give devices the key",
            channel_id,
            vec![field("wraps", Value::text(wraps.len().to_string()))],
        ),
        Op::Unopenable {
            channel_id,
            epoch,
            device,
        } => (
            "Key did not open",
            channel_id,
            vec![
                field("epoch", Value::text(epoch.to_string())),
                field("device", Value::text(hex(device))),
            ],
        ),
        Op::FromPeer { channel_id, write } => {
            let (title, fields) = from_peer(write);
            (title, channel_id, fields)
        }
    };
    let mut all = channel_description_fields(channel);
    all.extend(fields);
    let shown = match op {
        Op::CreateChannel { name, .. } | Op::CreateChannelWithMeta { name, .. } => {
            format!("#{name}")
        }
        _ => room_title(channel),
    };
    Description {
        title: format!("{title} · {shown}"),
        fields: all,
    }
}

describe::export!(Op, describe);

fn posted(channel_id: &str, message_id: &str, text: String, thread: &Option<u64>) -> Description {
    let mut fields = channel_description_fields(channel_id);
    fields.extend([
        field("text", Value::Text(text)),
        field("message", Value::text(message_id)),
        field(
            "thread",
            Value::Text(thread.map_or_else(|| "—".into(), |t| t.to_string())),
        ),
    ]);
    Description {
        title: match dm_peers(channel_id) {
            Some(_) => "Direct message".into(),
            None => format!("Post in #{channel_id}"),
        },
        fields,
    }
}

fn posting(post_policy: &PostPolicy) -> &'static str {
    match post_policy {
        PostPolicy::Open => "open",
        PostPolicy::MembersOnly => "members only",
    }
}

fn created(name: &str, post_policy: &PostPolicy) -> Vec<Field> {
    vec![
        field("name", Value::text(name)),
        field("posting", Value::text(posting(post_policy))),
    ]
}

fn entries(meta: &BTreeMap<String, String>) -> Value {
    Value::List(
        meta.iter()
            .map(|(key, value)| Value::text(format!("{key}: {value}")))
            .collect(),
    )
}

fn room_title(channel_id: &str) -> String {
    match dm_peers(channel_id) {
        Some(_) => "DM".into(),
        None => format!("#{channel_id}"),
    }
}

fn channel_description_fields(channel_id: &str) -> Vec<Field> {
    let mut fields = vec![field("channel", Value::Text(room_title(channel_id)))];
    fields.extend(dm_peers(channel_id).map(|(a, b)| {
        field(
            "between",
            Value::List(vec![Value::Account(a), Value::Account(b)]),
        )
    }));
    fields
}

fn principal_description(principal: &Principal) -> Value {
    match principal {
        Principal::Account(number) => Value::Account(*number),
        Principal::Root => Value::text("system"),
        Principal::Foreign(_) => Value::text(crate::view::unnamed(principal)),
    }
}

fn named(message: &PeerMessage) -> Field {
    let named = match message {
        PeerMessage::Here(id) => format!("{id}, here"),
        PeerMessage::There(id) => format!("{id}, there"),
    };
    field("message", Value::text(named))
}

fn from_peer(write: &PeerWrite) -> (&'static str, Vec<Field>) {
    match write {
        PeerWrite::Post {
            message_id, blocks, ..
        } => (
            "Post from a linked channel",
            vec![
                field("text", Value::Text(plain_text(blocks))),
                field("message", Value::text(message_id)),
            ],
        ),
        PeerWrite::Notify {
            message_id, notice, ..
        } => (
            "Notice from a linked channel",
            vec![
                field("text", Value::Text(notice_text(notice))),
                field("message", Value::text(message_id)),
            ],
        ),
        PeerWrite::Edit {
            message, blocks, ..
        } => (
            "Edit from a linked channel",
            vec![
                named(message),
                field("text", Value::Text(plain_text(blocks))),
            ],
        ),
        PeerWrite::Delete { message } => ("Delete from a linked channel", vec![named(message)]),
        PeerWrite::React { message, emoji, on } => (
            match on {
                true => "Reaction from a linked channel",
                false => "Reaction taken back from a linked channel",
            },
            vec![named(message), field("emoji", Value::text(emoji))],
        ),
    }
}

fn under(thread: Option<u64>) -> Field {
    let root = thread.map_or_else(|| "—".into(), |root| root.to_string());
    field("thread", Value::Text(root))
}
