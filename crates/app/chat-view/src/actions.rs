//! What a press does: the message menus, the writes (reactions, deletes,
//! the channel's details, new channels), copying and links.
use chat::{MsgRow, Op, PostPolicy, Principal};
use view::methods::Submitted;
use view::prelude::*;

use crate::composer::Target;
use crate::message::{ChatMessage, chat_message, mark_message_groups, zone};
use crate::notifier::Choice;
use crate::room_list::{Chip, chip, chipped};
use crate::{Chat, Control, CopyRange, Folded, Menu, Mode, Pane, links};
use chat::view::Names;
use std::rc::Rc;

/// A refusal the archive gives every reaction, said before it is asked.
const ARCHIVED_REACTIONS: &str = "This channel is archived — reactions are closed. Unarchive it from Channel details to react here again.";

impl Chat {
    // ---------- menus ----------

    /// A press on a message's body: chosen (its actions stay open), or with
    /// shift held (`extend`) the copy range grows to it from the chosen or
    /// the range's anchor.
    pub(crate) fn press_message(&mut self, pane: Pane, seq: u64, extend: bool) {
        if seq == 0 {
            return;
        }
        if !extend {
            self.copy = None;
            self.menu = Some(Menu {
                pane,
                seq,
                rev: 0,
                mode: Mode::Toolbar,
                at: self.layout.press,
            });
            return;
        }
        let anchor = match (self.copy, &self.menu) {
            (Some(range), _) if range.pane == pane => range.anchor,
            (None, Some(menu)) if menu.pane == pane && menu.mode == Mode::Toolbar => menu.seq,
            _ => seq,
        };
        self.menu = None;
        self.copy = Some(CopyRange {
            pane,
            anchor,
            head: seq,
        });
    }

    /// The chosen message and the copy range let go: Escape, a press
    /// anywhere else, the arrows moving to another message. A menu open
    /// over the message (More, the picker, the confirmation, the edit
    /// field) is dismissed by its own overlay, not here.
    pub(crate) fn let_go(&mut self) {
        if self
            .menu
            .as_ref()
            .is_some_and(|menu| menu.mode == Mode::Toolbar)
        {
            self.menu = None;
        }
        self.copy = None;
    }

    /// Whether [`Self::let_go`] has something to let go.
    pub(crate) fn holds(&self) -> bool {
        self.copy.is_some()
            || self
                .menu
                .as_ref()
                .is_some_and(|menu| menu.mode == Mode::Toolbar)
    }

    /// Opens the menu of the message at `seq` in `mode` and hands the keys
    /// to what the desktop draws for it: the edit's field, the profile
    /// card's first button, the delete's Cancel, the menu's own focus.
    pub(crate) fn open_menu(
        &mut self,
        pane: Pane,
        seq: u64,
        rev: u32,
        mode: Mode,
        window: &mut view::Window,
        _cx: &mut Context<Self>,
    ) {
        if !self.show_menu(pane, seq, rev, mode) {
            return;
        }
        match self.editing() {
            // the edit field takes the keys as it opens: what is typed
            // edits, and Enter saves; left in the room's composer, the
            // typing was a new message and Enter sent it
            Some(target) => crate::composer::focus(window, &target.key()),
            None if mode == Mode::Profile => {
                window.focus(crate::ui::menu::card::focus_key(self, pane, seq))
            }
            None => window.focus(crate::ui::menu::focus_key(pane, mode)),
        }
    }

    /// Opens the menu of the message at `seq` in `mode`, its edit's draft
    /// seeded and its picker fresh, and moves no keys: the surface that
    /// draws it does. False when it does not open: no message there yet,
    /// reactions in an archived room (said in the notice), an edit of
    /// nothing.
    pub(crate) fn show_menu(&mut self, pane: Pane, seq: u64, rev: u32, mode: Mode) -> bool {
        if seq == 0 {
            return false;
        }
        if mode == Mode::Reactions && self.room_info().is_some_and(|i| i.channel.archived) {
            self.notice = ARCHIVED_REACTIONS.into();
            return false;
        }
        if mode == Mode::Editing {
            let Some(body) = self.edit_body(pane, seq) else {
                return false;
            };
            let target = Target::Edit {
                channel: self.room_id(),
                seq,
                base_rev: rev,
            };
            let choices = self.mention_choices();
            let kept = self.kept_files(pane, seq);
            let draft = self.drafts.entry(target.key()).or_default();
            draft.seed(&body, &choices);
            draft.kept = kept;
        }
        if mode == Mode::Reactions {
            self.picker = Default::default();
        }
        self.menu = Some(Menu {
            pane,
            seq,
            rev,
            mode,
            at: self.layout.press,
        });
        self.menu_cursor = 0;
        self.menu_at_pointer = false;
        true
    }

    pub(crate) fn close_menu(&mut self) {
        self.menu = None;
    }

    // Who chose `emoji` on the message at `seq`, by name, as the room last
    // read them.
    pub(crate) fn reactor_names(&self, seq: u64, emoji: &str) -> Vec<String> {
        let empty = Names::empty();
        let names = self.names.ready().unwrap_or(&empty);
        let reactors = self.room.as_ref().and_then(|room| room.reactors.get(&seq));
        reactors
            .and_then(|reactors| reactors.get(emoji))
            .into_iter()
            .flatten()
            .map(|principal| names.author(principal))
            .collect()
    }

    /// Presses `control` of the message at `seq`: what its click does.
    pub(crate) fn act(
        &mut self,
        pane: Pane,
        seq: u64,
        rev: u32,
        control: Control,
        window: &mut view::Window,
        cx: &mut Context<Self>,
    ) {
        match control {
            // an agent's face opens its card; a person's, their profile
            Control::Profile(account) => {
                let agent = self.names.ready().is_some_and(|names| {
                    crate::message::agent(names, &Principal::Account(account))
                });
                match agent {
                    true => self.open_menu(pane, seq, rev, Mode::Profile, window, cx),
                    false => self.open_link(account.to_string(), cx),
                }
            }
            Control::Height(link) => cx.host().open_link(&link),
            Control::ProgramOpen(link) => self.open_link(link, cx),
            Control::Reaction { emoji, add } => self.react(seq, emoji, add, cx),
            Control::ThumbsUp => self.react(seq, "👍".into(), true, cx),
            Control::AddReaction | Control::React => {
                self.open_menu(pane, seq, rev, Mode::Reactions, window, cx)
            }
            Control::More => self.open_menu(pane, seq, rev, Mode::More, window, cx),
            Control::Replies | Control::Thread => self.reply_in_thread(seq, cx),
        }
    }

    fn edit_body(&self, pane: Pane, seq: u64) -> Option<String> {
        let names = self.names.ready().cloned().unwrap_or_default();
        let row = self.shown(pane).find(|row| row.seq == seq)?.clone();
        let message = chat_message(row, &names, &links::Rooms::default());
        (!message.edit_body.is_empty()).then_some(message.edit_body)
    }

    /// The rows one pane shows, fetched and pending, as the index served them.
    pub(crate) fn shown(&self, pane: Pane) -> Box<dyn Iterator<Item = &MsgRow> + '_> {
        let Some(room) = &self.room else {
            return Box::new(std::iter::empty());
        };
        let fetched = room.messages.ready().into_iter().flatten();
        match pane {
            Pane::Timeline => Box::new(
                fetched
                    .chain(&room.pending)
                    .filter(|row| row.thread.is_none()),
            ),
            Pane::Thread => {
                let Some(thread) = &room.thread else {
                    return Box::new(std::iter::empty());
                };
                let root = fetched.filter(move |row| row.seq == thread.root).take(1);
                let replies = thread.replies.ready().into_iter().flatten();
                let pending = room
                    .pending
                    .iter()
                    .filter(move |row| row.thread == Some(thread.root));
                Box::new(root.chain(replies).chain(pending))
            }
        }
    }

    /// Rows to messages, named by the directory the reader has.
    pub(crate) fn messages(&self, pane: Pane) -> Rc<[ChatMessage]> {
        let empty = Names::empty();
        let names = self.names.ready().unwrap_or(&empty);
        let boundary = (pane == Pane::Timeline).then_some(self.reads.boundary);
        let zone = zone();
        let rooms = self.rooms();
        let mut drawn = self.drawn(pane).borrow_mut();
        let same = |folded: &&Folded| {
            folded.boundary == boundary
                && folded.zone == zone
                && folded.names == *names
                && folded.rooms == rooms
                && folded.rows.iter().eq(self.shown(pane))
        };
        if let Some(folded) = drawn.folded.as_ref().filter(same) {
            return folded.messages.clone();
        }
        let rows: Vec<MsgRow> = self.shown(pane).cloned().collect();
        let mut messages: Vec<ChatMessage> = rows
            .iter()
            .map(|row| chat_message(row.clone(), names, &rooms))
            .collect();
        mark_message_groups(&mut messages, boundary);
        let messages: Rc<[ChatMessage]> = messages.into();
        drawn.folded = Some(Folded {
            rows,
            names: names.clone(),
            rooms,
            boundary,
            zone,
            messages: messages.clone(),
        });
        messages
    }

    /// Who wrote the message at `seq`.
    pub(crate) fn author(&self, pane: Pane, seq: u64) -> Option<Principal> {
        let row = self.shown(pane).find(|row| row.seq == seq)?;
        Some(row.author.clone())
    }

    // Whether the reader wrote the message at `seq`: who edits it before policies (`may_edit`).
    pub(crate) fn wrote(&self, pane: Pane, seq: u64) -> bool {
        let Some(me) = self.me() else { return false };
        self.shown(pane)
            .any(|row| row.seq == seq && row.author == me)
    }

    // Whether the reader may delete the message at `seq`: its author's delete is the message's
    // policy's to decide, anyone else's the channel's, as chat decides them (`policies.rs`).
    pub(crate) fn may_delete(&self, pane: Pane, seq: u64) -> bool {
        match self.wrote(pane, seq) {
            true => self.may_edit(pane, seq),
            false => self.may_change_room(),
        }
    }

    // ---------- writes ----------

    pub(crate) fn react(&mut self, seq: u64, emoji: String, add: bool, cx: &mut Context<Self>) {
        let channel_id = self.room_id();
        if channel_id.is_empty() || seq == 0 {
            return;
        }
        if self.room_info().is_some_and(|i| i.channel.archived) {
            self.notice = ARCHIVED_REACTIONS.into();
            return;
        }
        if self
            .menu
            .as_ref()
            .is_some_and(|m| m.mode == Mode::Reactions)
        {
            self.menu = None;
        }
        if add {
            crate::emoji::remember(&mut self.recent_emoji, &emoji);
            self.save_emoji(cx);
        }
        let op = if add {
            Op::AddReaction {
                channel_id,
                seq,
                emoji,
            }
        } else {
            Op::RemoveReaction {
                channel_id,
                seq,
                emoji,
            }
        };
        self.submit(op, cx);
    }

    pub(crate) fn delete_armed(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.menu.take() else { return };
        if menu.mode != Mode::Delete {
            return;
        }
        let channel_id = self.room_id();
        self.submit(
            Op::DeleteMessage {
                channel_id,
                seq: menu.seq,
            },
            cx,
        );
    }

    pub(crate) fn rename(&mut self, cx: &mut Context<Self>) {
        let Some(details) = &self.details else { return };
        let name = details.name_draft.text().trim().to_owned();
        let channel_id = self.room_id();
        // the name it has already is no rename (Expo `details.tsx`, `rename`)
        let unchanged = self
            .info(&channel_id)
            .is_some_and(|info| info.channel.name == name);
        if name.is_empty() || unchanged {
            return;
        }
        self.submit(Op::RenameChannel { channel_id, name }, cx);
    }

    pub(crate) fn set_archived(&mut self, archived: bool, cx: &mut Context<Self>) {
        let channel_id = self.room_id();
        if channel_id.is_empty() {
            return;
        }
        self.submit(
            Op::SetChannelArchived {
                channel_id,
                archived,
            },
            cx,
        );
    }

    pub(crate) fn choose_notices(&mut self, choice: Choice, cx: &mut Context<Self>) {
        let channel_id = self.room_id();
        if channel_id.is_empty() {
            return;
        }
        if let Some(details) = &mut self.details {
            details.notices_at = None;
        }
        cx.notify();
        self.submit(choice.op(channel_id), cx);
    }

    pub(crate) fn step_notices(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(details) = &mut self.details {
            details.notices_at = Some(index);
            cx.notify();
        }
    }

    pub(crate) fn choice(&self, room: &str) -> Option<Choice> {
        let choices = self.choices.ready()?;
        Some(choices.get(room).copied().unwrap_or_default())
    }

    pub(crate) fn agents_choice(&self, whose: Option<u64>) -> Option<Choice> {
        let chosen = self.managed_choices.ready()?;
        let mut of = chosen
            .iter()
            .filter(|(account, _)| whose.is_none_or(|whose| whose == *account))
            .map(|(_, choice)| *choice);
        let first = of.next()?;
        of.all(|choice| choice == first).then_some(first)
    }

    pub(crate) fn choose_for_agents(
        &mut self,
        whose: Option<u64>,
        choice: Choice,
        cx: &mut Context<Self>,
    ) {
        let channel_id = self.room_id();
        let managed = self.managed_choices.ready().into_iter().flatten();
        let accounts: Vec<u64> = managed
            .map(|(account, _)| *account)
            .filter(|account| whose.is_none_or(|whose| whose == *account))
            .collect();
        if channel_id.is_empty() || accounts.is_empty() {
            return;
        }
        cx.notify();
        self.submit(choice.op_for(accounts, channel_id), cx);
    }

    pub(crate) fn open_notices_menu(&mut self, whose: Option<u64>, cx: &mut Context<Self>) {
        let held = self.agents_choice(whose);
        let mut offered = crate::ui::notices::OFFERED.iter();
        let active = offered
            .position(|choice| Some(*choice) == held)
            .unwrap_or_default();
        self.notices_menu = Some(crate::NoticesMenu {
            whose,
            at: None,
            active,
        });
        cx.notify();
    }

    pub(crate) fn place_notices_menu(
        &mut self,
        button: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = &mut self.notices_menu else {
            return;
        };
        if menu.at.is_none() {
            menu.at = Some(button.bottom_left());
            window.focus(crate::ui::notices::MENU);
            cx.notify();
        }
    }

    pub(crate) fn move_in_notices_menu(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(menu) = &mut self.notices_menu {
            menu.active = index;
            cx.notify();
        }
    }

    pub(crate) fn close_notices_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.notices_menu.take() {
            window.focus(crate::ui::notices::pick_id(menu.whose));
            cx.notify();
        }
    }

    pub(crate) fn pick_in_notices_menu(
        &mut self,
        whose: Option<u64>,
        choice: Choice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_notices_menu(window, cx);
        self.choose_for_agents(whose, choice, cx);
    }

    /// The member the details' field names, if what it holds is an
    /// account number.
    pub(crate) fn typed_member(&self) -> Option<Principal> {
        let typed = self.details.as_ref()?.member_draft.text();
        Principal::parse(&typed)
    }

    /// The member typed in the details pane joins the room.
    pub(crate) fn add_member(&mut self, cx: &mut Context<Self>) {
        let Some(principal) = self.typed_member() else {
            self.notice = "A member is an account number".into();
            return;
        };
        if let Some(details) = &mut self.details {
            details.member_draft.reset("");
        }
        self.set_member(principal, true, cx);
    }

    /// The phone's Add sheet, from Add or the field's return: an account
    /// number joins the room and the sheet closes; anything else leaves
    /// the sheet open and does nothing (Expo `details.tsx`, `addTyped`).
    pub(crate) fn add_typed(&mut self, cx: &mut Context<Self>) {
        if self.typed_member().is_none() {
            return;
        }
        self.close_details_sheet();
        self.add_member(cx);
    }

    /// The phone's Rename sheet, from Rename or the field's return: it
    /// closes, and the name goes as [`Chat::rename`] sends it (Expo
    /// `details.tsx`, `rename`).
    pub(crate) fn rename_from_sheet(&mut self, cx: &mut Context<Self>) {
        self.close_details_sheet();
        self.rename(cx);
    }

    pub(crate) fn set_member(
        &mut self,
        principal: Principal,
        member: bool,
        cx: &mut Context<Self>,
    ) {
        let channel_id = self.room_id();
        if channel_id.is_empty() {
            return;
        }
        self.submit(
            Op::SetMembership {
                channel_id,
                principal,
                member,
            },
            cx,
        );
    }

    /// The open room's details close, or open with the reader's choices
    /// for its notices read.
    pub(crate) fn toggle_details(&mut self, cx: &mut Context<Self>) {
        match self.details.take() {
            Some(_) => {}
            None => {
                let info = self.room_info();
                let name = info
                    .map(|info| info.channel.name.clone())
                    .unwrap_or_default();
                let rules = self.room_rules().unwrap_or_default().to_owned();
                self.details = Some(crate::Details {
                    name_draft: view::TextField::new(name),
                    rules_draft: view::TextField::new(rules),
                    member_draft: Default::default(),
                    notices_at: None,
                    sheet: None,
                });
                self.load_choices(cx);
            }
        }
    }

    // ---------- copying and links ----------

    pub(crate) fn copy_text(&mut self, text: String, label: &str, cx: &mut Context<Self>) {
        if !text.is_empty() {
            cx.host().notify::<ClipboardWrite>(text);
            self.confirmation = format!("Copied {label}");
        }
    }

    /// The copy range as one run of text, to the clipboard.
    pub(crate) fn copy_range(&mut self, cx: &mut Context<Self>) {
        let Some(range) = self.copy else { return };
        let lines: Vec<String> = self
            .messages(range.pane)
            .iter()
            .filter(|m| range.holds(range.pane, m.seq))
            .map(|m| format!("{}: {}", m.author, m.body))
            .collect();
        if lines.is_empty() {
            return;
        }
        let label = view::design::plural(lines.len() as u64, "message", "messages");
        self.copy_text(lines.join("\n"), &label, cx);
    }

    pub(crate) fn copy_count(&self) -> usize {
        let Some(range) = self.copy else { return 0 };
        self.shown(range.pane)
            .filter(|row| range.holds(range.pane, row.seq))
            .count()
    }

    /// None before the session names a chain: Copy link is not offered.
    pub(crate) fn message_link(&self, seq: u64) -> Option<String> {
        links::channel_link(&self.session.chain_id, &self.room_id(), Some(seq))
    }

    /// Where the program the agent `author`'s script calls shows its work
    /// in this room ([`links::work_record`]). None for an author that is no
    /// agent with a script, or before the session names a chain.
    pub(crate) fn work_record(&self, author: &Principal) -> Option<String> {
        let names = self.names.ready()?;
        let Some(chat::Kind::Program {
            category: chat::Category::Agent,
            script: Some(script),
            ..
        }) = names.kind(author)
        else {
            return None;
        };
        let agent = author.account()?;
        links::work_record(
            &self.session.chain_id,
            &script.calls,
            agent,
            &self.room_id(),
        )
    }

    // The rooms a message names: the list's, but the direct ones.
    pub(crate) fn rooms(&self) -> links::Rooms {
        let listed = self
            .channels
            .ready()
            .into_iter()
            .flatten()
            .filter(|info| chat::dm_peers(&info.channel.id).is_none())
            .map(|info| (info.channel.id.clone(), info.channel.name.clone()));
        links::Rooms::new(&self.session.chain_id, listed)
    }

    // A link pressed in a message: a room of this chain opens here, at the
    // message it names; any other link as `open_link` opens it.
    pub(crate) fn open_pressed(&mut self, link: String, cx: &mut Context<Self>) {
        match links::room_route(&self.session.chain_id, &link) {
            Some((room, seq)) => {
                self.search_clear();
                self.open_at(room, seq, cx);
            }
            None => self.open_link(link, cx),
        }
    }

    pub(crate) fn open_link(&mut self, link: String, cx: &mut Context<Self>) {
        self.create = None;
        match links::pressed_link(link, &self.session.chain_id) {
            Some(url) => cx.host().open_link(&url),
            None => cx.host().log("no link to open: the session names no chain"),
        }
    }

    // ---------- submitting ----------

    /// One op to the chat program; a refusal lands in the banner.
    pub(crate) fn submit(&mut self, op: Op, cx: &mut Context<Self>) {
        self.notice.clear();
        let submit = cx.host().ask::<Submit<::chat::Chat>>(op);
        cx.land(submit, |chat, result, cx| {
            cx.notify();
            match design::failure(&result) {
                None => chat.refresh(cx),
                Some(said) => chat.notice = said,
            }
        })
        .detach();
    }

    // The create form's name field `field` changed: its folder chip made, or taken back to text.
    pub(crate) fn create_name_changed(
        &mut self,
        field: &'static str,
        change: &view::wire::TextChange,
        window: &mut Window,
    ) {
        let Some(create) = &mut self.create else {
            return;
        };
        let had = std::mem::replace(&mut create.chip, chipped(change).map(str::to_owned));
        let edit = match chip(change, had.as_deref()) {
            Chip::Leave => return,
            Chip::Make { end, id } => {
                let text = change.text[..end].to_owned();
                let caret = change.cursor.end as usize;
                create.name.replace(field, 0..end, text, Some(id), caret)
            }
            Chip::Unmake { text } => {
                let caret = text.len();
                create.name.replace(field, 0..0, text, None, caret)
            }
        };
        window.dispatch(edit);
    }

    /// The create dialog's channel: the host names it, chat opens it.
    pub(crate) fn create_channel(&mut self, cx: &mut Context<Self>) {
        let ready = self.holds_account() && self.session.connected;
        let Some(create) = self.create.as_mut().filter(|create| ready && !create.busy) else {
            return;
        };
        let name = create.name.text().trim().to_string();
        if name.is_empty() || name.contains('\0') {
            create.error = "Enter a channel name".to_string();
            return;
        }
        create.error.clear();
        create.busy = true;
        let members_only = create.members_only;
        let host = cx.host();
        let created = async move {
            let channel_id = host.ask::<HostId>("channel".into()).await?;
            let op = new_channel(channel_id.clone(), name, members_only);
            let submitted = host.ask::<Submit<::chat::Chat>>(op).await?;
            Ok::<_, Error>((channel_id, submitted))
        };
        cx.land(created, |chat, result, cx| {
            cx.notify();
            let problem = match result {
                Ok((id, Submitted::Applied { .. })) => {
                    chat.create = None;
                    chat.reread_channels(cx);
                    chat.choose(id, cx);
                    return;
                }
                // why not, in every view's words, under the name (Expo
                // `new.tsx` `problem={said}`)
                answer => design::failure(&answer.map(|(_, submitted)| submitted))
                    .expect("only a landed write has no failure"),
            };
            if let Some(create) = &mut chat.create {
                create.busy = false;
                create.error = problem;
            }
        })
        .detach();
    }
}

fn new_channel(channel_id: String, name: String, members_only: bool) -> Op {
    let post_policy = match members_only {
        true => PostPolicy::MembersOnly,
        false => PostPolicy::Open,
    };
    Op::CreateChannel {
        channel_id,
        name,
        post_policy,
    }
}
