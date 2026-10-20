//! The reader's own typing, told to the room as chat's empty draft: a keystroke that takes a
//! composer on screen from empty to words sends it, that composer emptying or leaving the screen
//! takes it down (a post does itself), and each start takes down what this device left up.
use std::collections::{BTreeMap, BTreeSet};

use chat::Op;
use view::prelude::*;
use view::store;

use crate::Chat;
use crate::composer::{Draft, Target};

const KEPT: &str = "typing";
const DOING: &str = "typing";

type Place = (String, Option<u64>);

#[derive(Default)]
pub(crate) struct Typing {
    pub(crate) began: Option<Place>,
    pub(crate) words: BTreeMap<String, bool>,
    at: Option<Place>,
    rooms: BTreeSet<String>,
    kept: Option<String>,
}

fn sending(draft: &Draft) -> bool {
    !draft.in_flight.is_empty() || draft.submitted.is_some() || draft.queued.is_some()
}

fn post_key(channel: &str, thread: Option<u64>) -> String {
    let channel = channel.to_owned();
    Target::Post { channel, thread }.key()
}

impl Chat {
    fn typed_key(&self) -> Option<String> {
        let signer = &self.session.signer;
        (!signer.is_empty()).then(|| format!("{}/{KEPT}", crate::notifier::reads_key(signer)))
    }

    fn typed_place(&self) -> Option<Place> {
        let (channel, thread) = self.typing.began.clone()?;
        let room = self.room.as_ref().filter(|room| room.id == channel)?;
        let open_thread = room.thread.as_ref().map(|thread| thread.root);
        let shown = thread.is_none() || thread == open_thread;
        let draft = self.drafts.get(&post_key(&channel, thread));
        let words = draft.is_some_and(|draft| !draft.body().is_empty() && !sending(draft));
        let listed = self.info(&channel).is_some();
        let typing = shown && words && listed && self.may_write();
        typing.then_some((channel, thread))
    }

    fn seed_words(&mut self) {
        let Some(room) = &self.room else {
            return;
        };
        let thread = room.thread.as_ref().map(|thread| Some(thread.root));
        let shown = std::iter::once(None).chain(thread);
        let shown: Vec<String> = shown.map(|thread| post_key(&room.id, thread)).collect();
        for key in shown {
            let draft = self.drafts.get(&key);
            let words = draft.is_some_and(|draft| !draft.body().is_empty());
            self.typing.words.entry(key).or_insert(words);
        }
        let drafts = &self.drafts;
        self.typing.words.retain(|key, _| drafts.contains_key(key));
    }

    pub(crate) fn settle_typing(&mut self, cx: &mut Context<Self>) {
        self.seed_words();
        let now = self.typed_place();
        if now.is_none() {
            self.typing.began = None;
        }
        if now == self.typing.at {
            return;
        }
        if let Some((channel, thread)) = self.typing.at.take() {
            let stays = now.as_ref().is_some_and(|(room, _)| *room == channel);
            let sent = self
                .drafts
                .get(&post_key(&channel, thread))
                .is_some_and(sending);
            let taken_down = !stays && !sent;
            if taken_down {
                self.undraft(channel, cx);
            }
        }
        if let Some((channel_id, thread)) = now.clone() {
            self.typing.rooms.insert(channel_id.clone());
            let op = Op::Drafting {
                channel_id,
                thread,
                text: String::new(),
                anew: true,
                doing: Some(DOING.to_owned()),
            };
            let told = cx.host().ask::<Submit<::chat::Chat>>(op);
            cx.land(told, |_, told, cx| {
                if let Some(said) = design::failure(&told) {
                    cx.host().log(format!("the reader's typing: {said}"));
                }
            })
            .detach();
        }
        self.typing.at = now;
        self.save_typed(cx);
    }

    fn undraft(&mut self, channel_id: String, cx: &mut Context<Self>) {
        let key = self.typed_key();
        let op = Op::Undraft {
            channel_id: channel_id.clone(),
        };
        let told = cx.host().ask::<Submit<::chat::Chat>>(op);
        cx.land(told, move |chat, told, cx| {
            if let Some(said) = design::failure(&told) {
                let said = format!("the reader's typing stays up in {channel_id}: {said}");
                return cx.host().log(said);
            }
            let typing_there =
                (chat.typing.at.as_ref()).is_some_and(|(room, _)| *room == channel_id);
            let same_reader = chat.typed_key() == key;
            let still_needed = typing_there || !same_reader;
            if still_needed {
                return;
            }
            if chat.typing.rooms.remove(&channel_id) {
                chat.save_typed(cx);
            }
        })
        .detach();
    }

    pub(crate) fn typing_sent(&mut self, target: &Target, cx: &mut Context<Self>) {
        let Target::Post { channel, .. } = target else {
            return;
        };
        let here = self
            .typing
            .at
            .as_ref()
            .is_some_and(|(room, _)| room == channel);
        if here {
            return;
        }
        if self.typing.rooms.remove(channel) {
            self.save_typed(cx);
        }
    }

    pub(crate) fn load_typed(&mut self, cx: &mut Context<Self>) {
        self.typing.kept = None;
        let Some(key) = self.typed_key() else {
            return;
        };
        let read = store::get::<BTreeSet<String>>(&cx.host(), &key);
        cx.land(read, move |chat, read, cx| {
            if chat.typed_key().as_ref() != Some(&key) {
                return;
            }
            let left = read.unwrap_or_else(|refusal| {
                cx.log_refused("the rooms typed in", &refusal);
                None
            });
            let left = left.unwrap_or_default();
            chat.typing.rooms.extend(left.iter().cloned());
            chat.typing.kept = Some(key);
            chat.save_typed(cx);
            let here = chat.typing.at.as_ref().map(|(room, _)| room.clone());
            for room in left {
                if Some(&room) != here.as_ref() {
                    chat.undraft(room, cx);
                }
            }
        })
        .detach();
    }

    fn save_typed(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.typed_key() else {
            return;
        };
        if self.typing.kept.as_ref() != Some(&key) {
            return;
        }
        store::set(&cx.host(), &key, Some(&self.typing.rooms));
    }
}
