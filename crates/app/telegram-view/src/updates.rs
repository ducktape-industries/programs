use crate::words;
use crate::{
    Telegram,
    auth::decode,
    chats::{Dialog, Peer, message_peer},
    history::Message,
    rpc::Rpc,
};
use borsh::{BorshDeserialize, BorshSerialize};
use grammers_tl_types::{self as tl, Serializable};
use std::collections::{BTreeMap, VecDeque};
use view::prelude::*;

#[derive(Clone, Default, BorshDeserialize, BorshSerialize)]
pub(crate) struct Position {
    pub(crate) pts: i32,
    pub(crate) qts: i32,
    pub(crate) date: i32,
    pub(crate) seq: i32,
    pub(crate) channels: BTreeMap<i64, i32>,
}

#[derive(Default)]
pub(crate) struct Sync {
    pub(crate) position: Option<Position>,
    queued: VecDeque<tl::enums::Updates>,
}

#[derive(Clone, Copy)]
enum Gap {
    Global,
    Channel(i64),
}

enum Counter {
    Pts(i32, i32),
    Qts(i32),
    Channel(i64, i32, i32),
}

impl Position {
    fn advance(&mut self, counter: Counter) -> Result<bool, Gap> {
        let (current, next, count, gap) = match counter {
            Counter::Pts(next, count) => (&mut self.pts, next, count, Gap::Global),
            Counter::Qts(next) => (&mut self.qts, next, 1, Gap::Global),
            Counter::Channel(id, next, count) => {
                let current = self
                    .channels
                    .entry(id)
                    .or_insert(next.saturating_sub(count));
                (current, next, count, Gap::Channel(id))
            }
        };
        let expected_pts_qts_or_channel_pts_after_count = i64::from(*current) + i64::from(count);
        let counter_is_negative_or_has_gap =
            count < 0 || expected_pts_qts_or_channel_pts_after_count < i64::from(next);
        if counter_is_negative_or_has_gap {
            return Err(gap);
        }
        if expected_pts_qts_or_channel_pts_after_count > i64::from(next) {
            return Ok(false);
        }
        *current = next;
        Ok(true)
    }

    fn set(&mut self, state: tl::enums::updates::State) {
        let tl::enums::updates::State::State(state) = state;
        self.pts = state.pts;
        self.qts = state.qts;
        self.date = state.date;
        self.seq = state.seq;
    }
}

impl Telegram {
    fn syncing(&self) -> bool {
        self.live.calls.values().any(|call| {
            matches!(
                call.kind,
                Rpc::State | Rpc::Difference | Rpc::ChannelDifference(_)
            )
        })
    }

    fn request_dialog_checkpoint_for_channel_without_counter(&mut self, cx: &mut Context<Self>) {
        self.read_dialogs(false, cx);
    }

    fn establish_fresh_checkpoint_and_clear_pages_after_unrecoverable_gap(&mut self) {
        self.sync.position = None;
        self.chats.dialogs.clear();
    }

    pub(crate) fn start_updates(&mut self, cx: &mut Context<Self>) {
        let updates_request_is_unavailable_or_pending = !self.live.ready
            || self.live.transfer.is_some()
            || self.live.session.user.is_none()
            || self
                .live
                .calls
                .values()
                .any(|call| matches!(call.kind, Rpc::State | Rpc::Difference));
        if updates_request_is_unavailable_or_pending {
            return;
        }
        if let Some(position) = &self.sync.position {
            self.call(
                Rpc::Difference,
                tl::functions::updates::GetDifference {
                    pts: position.pts,
                    date: position.date,
                    qts: position.qts,
                    pts_limit: None,
                    pts_total_limit: None,
                    qts_limit: None,
                }
                .to_bytes(),
                cx,
            );
        } else {
            self.call(
                Rpc::State,
                tl::functions::updates::GetState {}.to_bytes(),
                cx,
            );
        }
    }

    fn channel_difference(&mut self, id: i64, cx: &mut Context<Self>) {
        if self
            .live
            .calls
            .values()
            .any(|call| matches!(call.kind, Rpc::ChannelDifference(peer) if peer == id))
        {
            return;
        }
        let channel = match self.input_peer(Peer::Channel(id)) {
            Ok(tl::enums::InputPeer::Channel(channel)) => {
                tl::enums::InputChannel::Channel(tl::types::InputChannel {
                    channel_id: id,
                    access_hash: channel.access_hash,
                })
            }
            _ => {
                self.failed = Some(words::NO_CHANNEL_HASH.into());
                self.read_dialogs(false, cx);
                return;
            }
        };
        let Some(pts) = self
            .sync
            .position
            .as_ref()
            .unwrap()
            .channels
            .get(&id)
            .copied()
        else {
            self.request_dialog_checkpoint_for_channel_without_counter(cx);
            return;
        };
        self.call(
            Rpc::ChannelDifference(id),
            tl::functions::updates::GetChannelDifference {
                force: true,
                channel,
                filter: tl::enums::ChannelMessagesFilter::Empty,
                pts,
                limit: {
                    const LOCAL_CHANNEL_EVENTS_PER_SLICE: i32 = 100;
                    LOCAL_CHANNEL_EVENTS_PER_SLICE
                },
            }
            .to_bytes(),
            cx,
        );
    }

    pub(crate) fn receive_updates(
        &mut self,
        mut updates: tl::enums::Updates,
        cx: &mut Context<Self>,
    ) {
        if self.live.session.user.is_none() {
            return;
        }
        let updates_must_wait_for_sync_or_checkpoint =
            self.syncing() || self.sync.position.is_none();
        if updates_must_wait_for_sync_or_checkpoint {
            self.sync.queued.push_back(updates);
            if !self.syncing() {
                self.start_updates(cx);
            }
            return;
        }
        match self.apply_updates(updates.clone(), cx) {
            Ok(()) => {
                cx.store()
                    .set("updates", self.sync.position.as_ref().unwrap());
                cx.notify();
            }
            Err(gap) => {
                let content_to_replay_without_sync_markers = &mut updates;
                match content_to_replay_without_sync_markers {
                    tl::enums::Updates::TooLong => {}
                    tl::enums::Updates::UpdateShort(batch)
                        if matches!(
                            batch.update,
                            tl::enums::Update::PtsChanged | tl::enums::Update::ChannelTooLong(_)
                        ) => {}
                    tl::enums::Updates::Updates(batch) => {
                        batch.updates.retain(|update| {
                            !matches!(
                                update,
                                tl::enums::Update::PtsChanged
                                    | tl::enums::Update::ChannelTooLong(_)
                            )
                        });
                        self.sync.queued.push_back(updates);
                    }
                    tl::enums::Updates::Combined(batch) => {
                        batch.updates.retain(|update| {
                            !matches!(
                                update,
                                tl::enums::Update::PtsChanged
                                    | tl::enums::Update::ChannelTooLong(_)
                            )
                        });
                        self.sync.queued.push_back(updates);
                    }
                    _ => self.sync.queued.push_back(updates),
                }
                match gap {
                    Gap::Global => self.start_updates(cx),
                    Gap::Channel(id) => self.channel_difference(id, cx),
                }
            }
        }
    }

    fn apply_updates(
        &mut self,
        updates: tl::enums::Updates,
        cx: &mut Context<Self>,
    ) -> Result<(), Gap> {
        let mut position = self.sync.position.as_ref().unwrap().clone();
        let mut short = None;
        let (changes, date, start, seq) = match updates {
            tl::enums::Updates::TooLong => return Err(Gap::Global),
            tl::enums::Updates::Updates(batch) => {
                self.cache_peers(batch.users, batch.chats, cx);
                (batch.updates, batch.date, batch.seq, batch.seq)
            }
            tl::enums::Updates::Combined(batch) => {
                self.cache_peers(batch.users, batch.chats, cx);
                (batch.updates, batch.date, batch.seq_start, batch.seq)
            }
            tl::enums::Updates::UpdateShort(batch) => (vec![batch.update], batch.date, 0, 0),
            tl::enums::Updates::UpdateShortMessage(message) => {
                if position.advance(Counter::Pts(message.pts, message.pts_count))? {
                    let peer = Peer::User(message.user_id);
                    short = Some((
                        peer,
                        Message {
                            id: message.id,
                            sender: if message.out {
                                Peer::User(self.live.session.user.unwrap())
                            } else {
                                peer
                            },
                            author: None,
                            date: message.date,
                            text: message.message,
                        },
                    ));
                }
                (vec![], message.date, 0, 0)
            }
            tl::enums::Updates::UpdateShortChatMessage(message) => {
                if position.advance(Counter::Pts(message.pts, message.pts_count))? {
                    short = Some((
                        Peer::Group(message.chat_id),
                        Message {
                            id: message.id,
                            sender: Peer::User(message.from_id),
                            author: None,
                            date: message.date,
                            text: message.message,
                        },
                    ));
                }
                (vec![], message.date, 0, 0)
            }
            tl::enums::Updates::UpdateShortSentMessage(message) => {
                position.advance(Counter::Pts(message.pts, message.pts_count))?;
                (vec![], message.date, 0, 0)
            }
        };
        let expected_batch_sequence = i64::from(position.seq) + 1;
        let seq_gap = i64::from(start) > expected_batch_sequence;
        let apply_seq = start == 0 || i64::from(start) == expected_batch_sequence;
        let mut batch_preflight_accepted_changes = Vec::new();
        for change in changes {
            let counter = counter(&change);
            let counterless_change_has_no_applicable_batch_sequence =
                counter.is_none() && !apply_seq;
            if counterless_change_has_no_applicable_batch_sequence {
                continue;
            }
            match &change {
                tl::enums::Update::PtsChanged => return Err(Gap::Global),
                tl::enums::Update::ChannelTooLong(channel)
                    if channel.pts.is_none_or(|pts| {
                        position
                            .channels
                            .get(&channel.channel_id)
                            .is_none_or(|current| pts > *current)
                    }) =>
                {
                    return Err(Gap::Channel(channel.channel_id));
                }
                _ => {}
            }
            let message_box_counter_or_batch_sequence_allows_change =
                counter.map_or(Ok(apply_seq), |counter| position.advance(counter))?;
            if message_box_counter_or_batch_sequence_allows_change {
                batch_preflight_accepted_changes.push(change);
            }
        }
        position.date = position.date.max(date);
        if apply_seq {
            position.seq = position.seq.max(seq);
        }
        self.sync.position = Some(position);
        for change in batch_preflight_accepted_changes {
            self.change(change, cx);
        }
        if let Some((peer, row)) = short {
            self.upsert_dialog(Dialog {
                peer,
                id: row.id,
                date: row.date,
                text: row.text.clone(),
            });
            if let Some(history) = &mut self.history {
                let history_is_for_updated_peer = history.peer == peer;
                if history_is_for_updated_peer {
                    history.messages.insert(row.id, row);
                }
            }
        }
        let batch_sequence_gap_after_applying_message_box_counters = seq_gap;
        if batch_sequence_gap_after_applying_message_box_counters {
            return Err(Gap::Global);
        }
        Ok(())
    }

    fn change(&mut self, change: tl::enums::Update, cx: &mut Context<Self>) {
        match change {
            tl::enums::Update::NewMessage(change) => self.show_message(change.message),
            tl::enums::Update::NewChannelMessage(change) => self.show_message(change.message),
            tl::enums::Update::EditMessage(change) => self.show_message(change.message),
            tl::enums::Update::EditChannelMessage(change) => self.show_message(change.message),
            tl::enums::Update::DeleteMessages(change) => {
                self.delete_messages(None, change.messages, cx)
            }
            tl::enums::Update::DeleteChannelMessages(change) => {
                self.delete_messages(Some(Peer::Channel(change.channel_id)), change.messages, cx)
            }
            tl::enums::Update::ChannelTooLong(change) => {
                self.channel_difference(change.channel_id, cx)
            }
            _ => {}
        }
    }

    fn delete_messages(&mut self, peer: Option<Peer>, ids: Vec<i32>, cx: &mut Context<Self>) {
        if let Some(history) = &mut self.history {
            let deletion_applies_to_history = peer
                .map_or(!matches!(history.peer, Peer::Channel(_)), |peer| {
                    peer == history.peer
                });
            if deletion_applies_to_history {
                history.messages.retain(|id, _| !ids.contains(id));
            }
        }
        if self.chats.dialogs.iter().any(|dialog| {
            peer.map_or(!matches!(dialog.peer, Peer::Channel(_)), |peer| {
                peer == dialog.peer
            }) && ids.contains(&dialog.id)
        }) {
            self.chats.dialogs.retain(|dialog| {
                !(peer.map_or(!matches!(dialog.peer, Peer::Channel(_)), |peer| {
                    peer == dialog.peer
                }) && ids.contains(&dialog.id))
            });
            self.read_dialogs(false, cx);
        }
    }

    pub(crate) fn state_reply(
        &mut self,
        body: &[u8],
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        self.sync
            .position
            .get_or_insert_default()
            .set(decode(body)?);
        self.updates_ready(cx);
        Ok(())
    }

    pub(crate) fn difference_reply(
        &mut self,
        body: &[u8],
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let (messages, changes, users, chats, state, more) =
            match decode::<tl::enums::updates::Difference>(body)? {
                tl::enums::updates::Difference::Empty(diff) => {
                    let position = self.sync.position.as_mut().unwrap();
                    position.date = diff.date;
                    position.seq = diff.seq;
                    self.updates_ready(cx);
                    return Ok(());
                }
                tl::enums::updates::Difference::TooLong(_) => {
                    self.establish_fresh_checkpoint_and_clear_pages_after_unrecoverable_gap();
                    if let Some(history) = &mut self.history {
                        history.messages.clear();
                    }
                    self.start_updates(cx);
                    return Ok(());
                }
                tl::enums::updates::Difference::Difference(diff) => (
                    diff.new_messages,
                    diff.other_updates,
                    diff.users,
                    diff.chats,
                    diff.state,
                    false,
                ),
                tl::enums::updates::Difference::Slice(diff) => (
                    diff.new_messages,
                    diff.other_updates,
                    diff.users,
                    diff.chats,
                    diff.intermediate_state,
                    true,
                ),
            };
        self.cache_peers(users, chats, cx);
        for message in messages {
            self.show_message(message);
        }
        for change in changes {
            if let Some(Counter::Channel(id, pts, _)) = counter(&change) {
                self.channel_position(id, pts, cx);
            }
            self.change(change, cx);
        }
        self.sync.position.as_mut().unwrap().set(state);
        cx.store()
            .set("updates", self.sync.position.as_ref().unwrap());
        if more {
            self.start_updates(cx);
        } else {
            self.updates_ready(cx);
        }
        Ok(())
    }

    pub(crate) fn channel_reply(
        &mut self,
        id: i64,
        body: &[u8],
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let (messages, changes, users, chats, pts, final_) =
            match decode::<tl::enums::updates::ChannelDifference>(body)? {
                tl::enums::updates::ChannelDifference::Empty(diff) => {
                    (vec![], vec![], vec![], vec![], diff.pts, diff.r#final)
                }
                tl::enums::updates::ChannelDifference::Difference(diff) => (
                    diff.new_messages,
                    diff.other_updates,
                    diff.users,
                    diff.chats,
                    diff.pts,
                    diff.r#final,
                ),
                tl::enums::updates::ChannelDifference::TooLong(diff) => {
                    let tl::enums::Dialog::Dialog(dialog) = diff.dialog else {
                        return Err(words::NO_CHANNEL_STATE.into());
                    };
                    let pts = dialog.pts.ok_or(words::NO_CHANNEL_POSITION)?;
                    if let Some(history) = &mut self.history {
                        let history_is_for_channel = history.peer == Peer::Channel(id);
                        if history_is_for_channel {
                            history.messages.clear();
                        }
                    }
                    (
                        diff.messages,
                        vec![],
                        diff.users,
                        diff.chats,
                        pts,
                        diff.r#final,
                    )
                }
            };
        self.cache_peers(users, chats, cx);
        for message in messages {
            self.show_message(message);
        }
        for change in changes {
            self.change(change, cx);
        }
        self.channel_position(id, pts, cx);
        if final_ {
            self.updates_ready(cx);
        } else {
            self.channel_difference(id, cx);
        }
        Ok(())
    }

    pub(crate) fn channel_position(&mut self, id: i64, pts: i32, cx: &mut Context<Self>) {
        if let Some(position) = &mut self.sync.position {
            let current = position.channels.entry(id).or_insert(pts);
            *current = (*current).max(pts);
            cx.store().set("updates", position);
        }
    }

    fn updates_ready(&mut self, cx: &mut Context<Self>) {
        cx.store()
            .set("updates", self.sync.position.as_ref().unwrap());
        if !self.syncing() {
            let queued = std::mem::take(&mut self.sync.queued);
            for updates in queued {
                self.receive_updates(updates, cx);
            }
        }
        self.read_dialogs(false, cx);
        self.read_history(false, cx);
        cx.notify();
    }
}

fn counter(update: &tl::enums::Update) -> Option<Counter> {
    use tl::enums::Update::*;
    let channel = |message: &tl::enums::Message, pts, count| match message_peer(message, None) {
        Some(Peer::Channel(id)) => Some(Counter::Channel(id, pts, count)),
        _ => None,
    };
    Some(match update {
        NewMessage(u) => Counter::Pts(u.pts, u.pts_count),
        DeleteMessages(u) => Counter::Pts(u.pts, u.pts_count),
        ReadHistoryInbox(u) => Counter::Pts(u.pts, u.pts_count),
        ReadHistoryOutbox(u) => Counter::Pts(u.pts, u.pts_count),
        WebPage(u) => Counter::Pts(u.pts, u.pts_count),
        ReadMessagesContents(u) => Counter::Pts(u.pts, u.pts_count),
        EditMessage(u) => Counter::Pts(u.pts, u.pts_count),
        FolderPeers(u) => Counter::Pts(u.pts, u.pts_count),
        PinnedMessages(u) => Counter::Pts(u.pts, u.pts_count),
        NewChannelMessage(u) => return channel(&u.message, u.pts, u.pts_count),
        EditChannelMessage(u) => return channel(&u.message, u.pts, u.pts_count),
        DeleteChannelMessages(u) => Counter::Channel(u.channel_id, u.pts, u.pts_count),
        ChannelWebPage(u) => Counter::Channel(u.channel_id, u.pts, u.pts_count),
        PinnedChannelMessages(u) => Counter::Channel(u.channel_id, u.pts, u.pts_count),
        NewEncryptedMessage(u) => Counter::Qts(u.qts),
        MessagePollVote(u) => Counter::Qts(u.qts),
        ChatParticipant(u) => Counter::Qts(u.qts),
        ChannelParticipant(u) => Counter::Qts(u.qts),
        BotStopped(u) => Counter::Qts(u.qts),
        BotChatInviteRequester(u) => Counter::Qts(u.qts),
        BotChatBoost(u) => Counter::Qts(u.qts),
        BotMessageReaction(u) => Counter::Qts(u.qts),
        BotMessageReactions(u) => Counter::Qts(u.qts),
        BotBusinessConnect(u) => Counter::Qts(u.qts),
        BotNewBusinessMessage(u) => Counter::Qts(u.qts),
        BotEditBusinessMessage(u) => Counter::Qts(u.qts),
        BotDeleteBusinessMessage(u) => Counter::Qts(u.qts),
        BotPurchasedPaidMedia(u) => Counter::Qts(u.qts),
        ManagedBot(u) => Counter::Qts(u.qts),
        BotGuestChatQuery(u) => Counter::Qts(u.qts),
        BotStarsSubscription(u) => Counter::Qts(u.qts),
        _ => return None,
    })
}
