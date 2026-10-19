//! The composer's events for one target, run through its draft: the
//! sends it makes, and which drafts are kept.
use crate::composer::{Draft, Event, MentionChoice, Outcome, Send, Target, pending_row};
use crate::names::mention_token;
use crate::{Chat, Mode, words};
use chat::MsgRow;
use chat::message::Attachment;
use view::prelude::*;

impl Chat {
    // Every name a mention may carry, the roster's and the room's members',
    // each marked whether `@` offers it.
    pub(crate) fn mention_choices(&self) -> Vec<MentionChoice> {
        let Some(names) = self.names.ready() else {
            return Vec::new();
        };
        let members: Vec<_> = self
            .roster()
            .into_iter()
            .map(|(principal, _)| principal)
            .collect();
        crate::names::mention_choices(names, &members)
            .into_iter()
            .map(|choice| MentionChoice {
                token: mention_token(&choice.principal),
                hint: match self.outside_harness(&choice.principal) {
                    true => Some(crate::words::OUTSIDE_HARNESS.to_owned()),
                    false => self
                        .unanswered(&choice.principal)
                        .map(|why| format!("Answers {why}")),
                },
                label: choice.label,
                offered: choice.offered,
            })
            .collect()
    }

    pub(crate) fn composer(
        &mut self,
        target: Target,
        event: Event,
        window: &mut view::Window,
        cx: &mut Context<Self>,
    ) {
        let choices = self.mention_choices();
        let key = target.key();
        // the draft the frame drew; none is an event that came the tick
        // after its composer left the screen
        let Some(draft) = self.drafts.get_mut(&key) else {
            return;
        };
        cx.notify();
        if let Event::Dropped(files) = event {
            return self.attach(&target, files, cx);
        }
        let Outcome::Action(tag) = draft.handle(event, &key, &choices, window) else {
            return;
        };
        match tag.as_str() {
            "send" => {
                if let Some(send) = draft.submitted.take() {
                    self.post_draft(target, send, cx);
                }
            }
            "attach" => self.pick_files(target, cx),
            "paste" => self.paste(target, cx),
            tag => {
                let removed = tag.strip_prefix("remove:").and_then(|id| id.parse().ok());
                if let Some(id) = removed {
                    self.remove_file(&target, id, cx);
                }
            }
        }
    }

    /// Makes the drafts agree with the screen, before every frame: each
    /// composer the frame draws has its stored draft, whatever wrote the
    /// room, the thread or the menu, so the draft a frame lowers is the one
    /// that hears what is typed into it. (An edit's is seeded with its
    /// message where its menu opens.) A draft whose composer is off screen
    /// stays while it holds something: the map is saved whole, and must
    /// not grow with every room opened or every edit begun.
    pub(crate) fn seat_drafts(&mut self) {
        let mut shown = Vec::new();
        if let Some(room) = &self.room {
            let post = |thread| {
                let channel = room.id.clone();
                Target::Post { channel, thread }.key()
            };
            shown.push(post(None));
            shown.extend(room.thread.as_ref().map(|thread| post(Some(thread.root))));
        }
        shown.extend(self.editing().map(|target| target.key()));
        self.drafts
            .retain(|key, draft| shown.contains(key) || !spent(key, draft));
        for key in shown {
            self.drafts.entry(key).or_default();
        }
    }

    /// The message the open menu edits, as its composer's target.
    pub(crate) fn editing(&self) -> Option<Target> {
        let menu = self.menu.as_ref().filter(|m| m.mode == Mode::Editing)?;
        Some(Target::Edit {
            channel: self.room_id(),
            seq: menu.seq,
            base_rev: menu.rev,
        })
    }

    // A send waits while a file of its draft is still going up: it is
    // posted once they are up, and handed back when one fails.
    pub(crate) fn post_draft(&mut self, target: Target, send: Send, cx: &mut Context<Self>) {
        let key = target.key();
        let Some(draft) = self.drafts.get_mut(&key) else {
            return;
        };
        if draft.uploading() {
            match &mut draft.queued {
                Some((queued, _)) => {
                    if !queued.body.is_empty() && !send.body.is_empty() {
                        queued.body.push('\n');
                    }
                    queued.body.push_str(&send.body);
                }
                None => draft.queued = Some((send, target)),
            }
            return;
        }
        let (files, message) = draft.take_files(&target);
        draft.in_flight.push(send.clone());
        self.send(key, send, files, message, target, cx);
    }

    fn send(
        &mut self,
        key: String,
        send: Send,
        files: Vec<Attachment>,
        message: Option<String>,
        target: Target,
        cx: &mut Context<Self>,
    ) {
        let me = self.me();
        let open_dm = self.dm_to_open(&target);
        cx.spawn(async move |this, cx| {
            let host = cx.host();
            let filed = message.clone();
            let minted = async {
                let id = match filed {
                    Some(id) => id,
                    None => host.ask::<HostId>("message".into()).await?,
                };
                crate::composer::op(id, &send, &files, &target)
            }
            .await;
            let held = minted
                .as_ref()
                .ok()
                .zip(me)
                .and_then(|(op, me)| pending_row(op, me));
            if let Some(row) = held.clone() {
                let _ = this.update(cx, |chat, cx| chat.hold(&target, row, cx));
            }
            // why the message did not land, in every view's words; none
            // when it did
            let failed = async {
                let op = match minted {
                    Ok(op) => op,
                    Err(refusal) => return design::failure(&Err(refusal)),
                };
                if let Some(open) = open_dm {
                    let opened = host.ask::<Submit<::chat::Chat>>(open).await;
                    // the DM did not open: the message was never sent
                    if let Some(said) = design::failure(&opened) {
                        return Some(format!("{} · {said}", words::OPENING_DM));
                    }
                }
                design::failure(&host.ask::<Submit<::chat::Chat>>(op).await)
            }
            .await;
            let _ = this.update(cx, |chat, cx| {
                cx.notify();
                // kept since the send, in flight; were it gone, a refused
                // send still needs a draft to wait in
                let draft = chat.drafts.entry(key).or_default();
                draft.complete_send(&send);
                match failed {
                    None => {
                        draft.sent_files(&files);
                        draft.note.clear();
                        if let Target::Edit { seq, .. } = target
                            && chat
                                .menu
                                .as_ref()
                                .is_some_and(|m| m.mode == Mode::Editing && m.seq == seq)
                        {
                            chat.menu = None;
                        }
                        chat.refresh(cx);
                    }
                    // a refusal, and a doubt alike, hold the draft for the
                    // person to send again
                    Some(said) => {
                        draft.note = said;
                        draft.failed(send);
                        draft.unsent_files(&files, message);
                        if let Some(row) = held {
                            chat.unhold(&row.message_id);
                        }
                    }
                }
            });
        })
        .detach();
    }

    fn hold(&mut self, target: &Target, row: MsgRow, cx: &mut Context<Self>) {
        let Some(room) = self
            .room
            .as_mut()
            .filter(|room| room.id == target.channel())
        else {
            return;
        };
        room.pending.push(row);
        room.settle();
        // the reader's own post follows, wherever they had scrolled: the
        // list goes to its end, a room landed off its head goes live first
        let pane = match target {
            Target::Post {
                thread: Some(_), ..
            } => crate::Pane::Thread,
            _ => crate::Pane::Timeline,
        };
        let landed = pane == crate::Pane::Timeline && room.landed;
        let id = room.id.clone();
        self.drawn(pane).borrow_mut().follow = true;
        if landed {
            self.open(id, cx);
        }
        cx.notify();
    }

    fn unhold(&mut self, message_id: &str) {
        let Some(room) = self.room.as_mut() else {
            return;
        };
        room.pending.retain(|row| row.message_id != message_id);
    }

    /// A post into a dm room the channel list doesn't hold yet: the op that
    /// opens it first. A dm id opens only through `CreateDmChannel`, named
    /// for the other person (a no-op once it is open).
    pub(crate) fn dm_to_open(&self, target: &Target) -> Option<chat::Op> {
        let Target::Post { channel, .. } = target else {
            return None;
        };
        if self.info(channel).is_some() {
            return None;
        }
        let peer = crate::names::dm_peer_of(self.my_account()?, channel)?;
        let name = self
            .names
            .ready()
            .and_then(|names| names.name(&chat::Principal::Account(peer)))
            .map(str::to_owned)
            .unwrap_or_else(|| words::account(peer));
        Some(chat::Op::CreateDmChannel {
            counterpart: peer,
            name,
        })
    }
}

/// Nothing a reader would miss in the draft kept under `key`: it holds no
/// send, and shows what a new one would. For an edit that is whatever it
/// holds: opening the edit seeds it with the message again.
fn spent(key: &str, draft: &Draft) -> bool {
    let unwritten =
        draft.field.text().is_empty() && draft.note.is_empty() && draft.attachments.is_empty();
    (unwritten || Target::edits(key))
        && draft.failed_send.is_none()
        && draft.submitted.is_none()
        && draft.in_flight.is_empty()
        && draft.queued.is_none()
}
