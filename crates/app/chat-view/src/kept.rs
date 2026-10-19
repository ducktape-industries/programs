//! What chat keeps on this device between runs (`store`), per network by the host and per
//! reader by the key: the read cursors (the unread dots, the notifier's counts), the frequent
//! reactions, the room list's folders the reader closed, and the rooms opened here.
use std::collections::{BTreeMap, BTreeSet};

use view::prelude::*;
use view::store;

use crate::Chat;

const EMOJI: &str = "emoji";
const OPENED: &str = "opened";
const FOLDERS: &str = "folders";

impl Chat {
    /// The key the reader's cursors are kept under; none with no key seated.
    fn reads_key(&self) -> Option<String> {
        (!self.session.signer.is_empty()).then(|| crate::notifier::reads_key(&self.session.signer))
    }

    /// The reader's cursors and reactions off the device. Until they land,
    /// nothing is written back: a room first seen now is read to its head,
    /// and that must not overwrite what was kept.
    pub(crate) fn load_kept(&mut self, cx: &mut Context<Self>) {
        self.reads.kept = None;
        let Some(key) = self.reads_key() else {
            return;
        };
        let emoji = store::get::<Vec<String>>(&cx.host(), EMOJI);
        cx.land(emoji, move |chat, emoji, cx| {
            cx.notify();
            let emoji = emoji.unwrap_or_else(|refusal| {
                cx.log_refused("the kept reactions", &refusal);
                None
            });
            for kept in emoji.unwrap_or_default() {
                if !chat.recent_emoji.contains(&kept) {
                    chat.recent_emoji.push(kept);
                }
            }
            chat.recent_emoji.truncate(crate::emoji::RECENT);
        })
        .detach();
        let folders = store::get::<BTreeSet<String>>(&cx.host(), FOLDERS);
        cx.land(folders, |chat, folders, cx| match folders {
            Ok(closed) => {
                chat.closed_folders = closed.unwrap_or_default();
                cx.notify();
            }
            Err(refusal) => cx.log_refused("the closed folders", &refusal),
        })
        .detach();
        self.load_opened(cx);
        self.load_reads(key, cx);
    }

    // The key the rooms the reader opened here are kept under.
    fn opened_key(&self) -> Option<String> {
        self.reads_key().map(|key| format!("{key}/{OPENED}"))
    }

    // The rooms the reader opened on this device, off it. One opened before
    // they land joins them, and the whole set is written back.
    fn load_opened(&mut self, cx: &mut Context<Self>) {
        self.opened.clear();
        self.opened_kept = None;
        let Some(key) = self.opened_key() else {
            return;
        };
        let opened = store::get::<BTreeSet<String>>(&cx.host(), &key);
        cx.land(opened, move |chat, opened, cx| {
            if chat.opened_key().as_ref() != Some(&key) {
                return;
            }
            let kept = opened.unwrap_or_else(|refusal| {
                cx.log_refused("the rooms opened here", &refusal);
                None
            });
            let kept = kept.unwrap_or_default();
            let grew = chat.opened.iter().any(|room| !kept.contains(room));
            chat.opened.extend(kept);
            chat.opened_kept = Some(key);
            if grew {
                chat.save_opened(cx);
            }
            cx.notify();
        })
        .detach();
    }

    // The room on screen joins the rooms opened here: settled each frame,
    // since the list, a link and a search all open rooms.
    pub(crate) fn settle_opened(&mut self, cx: &mut Context<Self>) {
        let Some(room) = &self.room else {
            return;
        };
        if self.opened.insert(room.id.clone()) {
            self.save_opened(cx);
        }
    }

    fn save_opened(&mut self, cx: &mut Context<Self>) {
        let key = self.opened_key();
        if key.is_none() || key != self.opened_kept {
            return;
        }
        let key = key.unwrap_or_default();
        store::set(&cx.host(), &key, Some(&self.opened));
    }

    fn load_reads(&mut self, key: String, cx: &mut Context<Self>) {
        let reads = store::get::<BTreeMap<String, u64>>(&cx.host(), &key);
        cx.land(reads, move |chat, reads, cx| {
            cx.notify();
            match reads {
                Ok(reads) => chat.reads_landed(key, reads.unwrap_or_default(), cx),
                // the reader changed meanwhile: their own load is under way
                Err(_) if chat.reads_key().as_ref() != Some(&key) => {}
                Err(refusal) => {
                    cx.host().log(format!(
                        "read cursors not loaded, starting from this session: {refusal}"
                    ));
                    chat.reads_landed(key, BTreeMap::new(), cx);
                    // nothing of this session is on the device yet
                    chat.reads.written.clear();
                    chat.save_reads(cx);
                }
            }
        })
        .detach();
    }

    /// The kept cursors stand for every room but the one on screen.
    fn reads_landed(&mut self, key: String, kept: BTreeMap<String, u64>, cx: &mut Context<Self>) {
        if self.reads_key().as_ref() != Some(&key) {
            return;
        }
        let viewing = self.viewing();
        for (room, seq) in kept {
            if Some(&room) != viewing.as_ref() {
                self.reads.cursors.insert(room, seq);
            }
        }
        self.reads.written = self.reads.cursors.clone();
        self.reads.kept = Some(key);
        if let Some(list) = self.channels.ready().cloned() {
            self.channels_landed(list, cx);
        }
    }

    /// The cursors onto the device when they moved.
    pub(crate) fn save_reads(&mut self, cx: &mut Context<Self>) {
        let key = self.reads_key();
        if key.is_none() || key != self.reads.kept || self.reads.written == self.reads.cursors {
            return;
        }
        store::set(
            &cx.host(),
            key.as_deref().unwrap_or_default(),
            Some(&self.reads.cursors),
        );
        self.reads.written = self.reads.cursors.clone();
    }

    pub(crate) fn save_emoji(&self, cx: &mut Context<Self>) {
        store::set(&cx.host(), EMOJI, Some(&self.recent_emoji));
    }

    pub(crate) fn save_folders(&self, cx: &mut Context<Self>) {
        store::set(&cx.host(), FOLDERS, Some(&self.closed_folders));
    }
}
