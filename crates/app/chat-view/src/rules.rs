//! A room's rules, its `rules` meta: the text itself, or a files path whose file holds them. The
//! details show them, reading a path's file while they are open and again on each files block that
//! moves it, and set and clear them; the composer says the room has some.
use abi::BlobId;
use view::prelude::*;
use view::{Host, Task};

use crate::{Chat, words};

#[derive(Default)]
pub(crate) struct Rules {
    path: String,
    file: Option<BlobId>,
    read: Option<Result<Read, String>>,
    reading: Option<Task<()>>,
    following: Option<Task<()>>,
}

#[derive(Clone, PartialEq, Debug)]
enum Read {
    File(String),
    NotAFile,
}

pub(crate) enum Said<'a> {
    Text(&'a str),
    Reading(&'a str),
    File { path: &'a str, text: &'a str },
    Failed { path: &'a str, why: &'a str },
}

type Landed = Option<(Option<BlobId>, Result<Read, String>)>;

impl Chat {
    pub(crate) fn room_rules(&self) -> Option<&str> {
        let rules = self.room_info()?.meta.get(chat::RULES)?.trim();
        (!rules.is_empty()).then_some(rules)
    }

    pub(crate) fn rules_said(&self) -> Option<Said<'_>> {
        let rules = self.room_rules()?;
        if !rules.starts_with('/') {
            return Some(Said::Text(rules));
        }
        let read = self
            .rules
            .read
            .as_ref()
            .filter(|_| self.rules.path == rules);
        Some(match read {
            None => Said::Reading(rules),
            Some(Ok(Read::File(text))) => Said::File { path: rules, text },
            Some(Ok(Read::NotAFile)) => Said::Text(rules),
            Some(Err(why)) => Said::Failed { path: rules, why },
        })
    }

    pub(crate) fn open_details(&mut self, cx: &mut Context<Self>) {
        if self.details.is_none() {
            self.toggle_details(cx);
        }
    }

    pub(crate) fn set_rules(&mut self, cx: &mut Context<Self>) {
        let Some(details) = &self.details else { return };
        let value = details.rules_draft.text().trim().to_owned();
        if value.is_empty() || self.room_rules() == Some(value.as_str()) {
            return;
        }
        let channel_id = self.room_id();
        let key = chat::RULES.to_owned();
        self.submit(
            chat::Op::SetChannelMeta {
                channel_id,
                key,
                value,
            },
            cx,
        );
    }

    pub(crate) fn clear_rules(&mut self, cx: &mut Context<Self>) {
        let channel_id = self.room_id();
        if channel_id.is_empty() {
            return;
        }
        if let Some(details) = &mut self.details {
            details.rules_draft.reset("");
        }
        let key = chat::RULES.to_owned();
        self.submit(chat::Op::RemoveChannelMeta { channel_id, key }, cx);
    }

    pub(crate) fn settle_rules(&mut self, cx: &mut Context<Self>) {
        let named = self.details.as_ref().and(self.room_rules());
        let Some(path) = named
            .filter(|rules| rules.starts_with('/'))
            .map(str::to_owned)
        else {
            self.rules = Rules::default();
            return;
        };
        if self.rules.path != path {
            self.rules = Rules {
                path,
                ..Rules::default()
            };
            self.read_rules(cx);
        }
        if self.rules.following.is_none() {
            let follow = cx.follow::<Changes<files::Files>>((), |chat, change, cx| match change {
                Ok(_) => chat.read_rules(cx),
                Err(refusal) => cx.log_refused(words::RULES_FILES, &refusal),
            });
            self.rules.following = Some(follow);
        }
    }

    fn read_rules(&mut self, cx: &mut Context<Self>) {
        let (host, path, known) = (cx.host(), self.rules.path.clone(), self.rules.file);
        let task = cx.spawn(async move |this, cx| {
            let landed = read(&host, &path, known).await;
            let _ = this.update(cx, |chat, cx| chat.rules_landed(&path, landed, cx));
        });
        self.rules.reading = Some(task);
    }

    fn rules_landed(&mut self, path: &str, landed: Landed, cx: &mut Context<Self>) {
        let Some((file, read)) = landed else { return };
        if self.rules.path != path {
            return;
        }
        self.rules.file = file;
        self.rules.read = Some(read);
        cx.notify();
    }
}

async fn read(host: &Host, path: &str, known: Option<BlobId>) -> Landed {
    let stat = files::ask::Stat {
        path: path.to_owned(),
        at: None,
    };
    let (snapshot, file) = match host.query(stat).await {
        Ok((snapshot, Some(entry))) => match entry.node {
            files::Node::File { file, .. } => (snapshot, file),
            files::Node::Dir { .. } | files::Node::Symlink { .. } => {
                return Some((None, Ok(Read::NotAFile)));
            }
        },
        Ok((_, None)) => return Some((None, Ok(Read::NotAFile))),
        Err(refusal) => return Some((None, Err(design::sentence(&refusal)))),
    };
    if known == Some(file) {
        return None;
    }
    let bytes = crate::widget::file_bytes(host, path, snapshot).await;
    let read = bytes.map(|bytes| Read::File(String::from_utf8_lossy(&bytes).into_owned()));
    Some((Some(file), read))
}
