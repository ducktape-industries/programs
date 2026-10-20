//! The operations the reader issues, and the forms that stage them.
//!
//! An operation is optimistic: the row that issued it says "Submitting…"
//! straight away, keeps saying so while the block that carries it is on its
//! way, and a refusal replaces it with the reason inline. Nothing is guessed
//! into the lists — the next query reconciles them.
use view::methods::Submitted;
use view::prelude::*;

use crate::api::SubmitForge;
use crate::select::Writer;
use crate::state::{Answer, ChangeForm, Forge, NewRepo, Pending, Progress, change_key};
use crate::words;
use forge::{Mergeability, Op, Principal, Revision, Settings, valid_repo_name};
use identity::{Ownership, Party};

fn with_writer(list: &Ownership, party: Party) -> Option<Ownership> {
    match list {
        Ownership::Owner => Some(Ownership::Listed(vec![party])),
        Ownership::Listed(parties) => {
            let listed = parties.contains(&party);
            if listed {
                return None;
            }
            let mut parties = parties.clone();
            parties.push(party);
            Some(Ownership::Listed(parties))
        }
        Ownership::Anyone => None,
    }
}

fn without_writer(list: &Ownership, writer: &Writer) -> Ownership {
    match (list, writer) {
        (Ownership::Anyone, Writer::Everyone) => Ownership::Owner,
        (Ownership::Listed(parties), Writer::Party(party)) => {
            let rest: Vec<Party> = parties
                .iter()
                .filter(|listed| *listed != party)
                .cloned()
                .collect();
            match rest.is_empty() {
                true => Ownership::Owner,
                false => Ownership::Listed(rest),
            }
        }
        (Ownership::Anyone, Writer::Party(_))
        | (Ownership::Listed(_), Writer::Everyone)
        | (Ownership::Owner, _) => list.clone(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MergeBlock {
    Loading,
    NotOpen,
    EndpointGone,
    Comparing,
    UpToDate,
    Unrelated,
    Diverged,
}

impl MergeBlock {
    pub fn sentence(self) -> &'static str {
        match self {
            Self::Loading => "This change has not loaded yet",
            Self::NotOpen => "This change is no longer open",
            Self::EndpointGone => "One of the endpoints of this change no longer exists",
            Self::Comparing => "Comparing the endpoints…",
            Self::UpToDate => "The target already contains this change",
            Self::Unrelated => "The endpoints share no history",
            Self::Diverged => {
                "The endpoints diverged. Merge with git and push the result, then Merge turns on."
            }
        }
    }
}

impl Forge {
    pub(crate) fn submit(&mut self, op: Op, scope: String, label: &str, cx: &mut Context<Self>) {
        let review = matches!(op, Op::ReviewSubmit { .. }).then(|| scope.clone());
        let id = self.issue(scope, label, cx);
        let submit = cx.host().ask::<SubmitForge>(op);
        cx.land(submit, move |forge, answer, cx| {
            cx.notify();
            if forge.settle(id, answer) {
                if let Some(key) = &review {
                    forge.reviews.remove(key);
                }
                forge.refresh(cx);
            }
        })
        .detach();
    }

    fn issue(&mut self, scope: String, label: &str, cx: &mut Context<Self>) -> u64 {
        self.next_pending += 1;
        let id = self.next_pending;
        self.pending.push(Pending {
            id,
            issued: self.block,
            scope,
            label: label.to_owned(),
            progress: Progress::Submitting,
        });
        cx.notify();
        id
    }

    fn settle(&mut self, id: u64, answer: Answer) -> bool {
        let block = self.block;
        let Some(op) = self.pending.iter_mut().find(|op| op.id == id) else {
            return false;
        };
        if !matches!(answer, Ok(Submitted::Applied { .. })) {
            op.progress = Progress::Refused(answer);
            return false;
        }
        if block > op.issued {
            self.pending.retain(|op| op.id != id);
        } else {
            op.progress = Progress::Accepted;
        }
        true
    }

    pub(crate) fn dismiss(&mut self, id: u64, cx: &mut Context<Self>) {
        self.pending
            .retain(|op| op.id != id || !matches!(op.progress, Progress::Refused(_)));
        cx.notify();
    }

    pub(crate) fn create_repo(&mut self, cx: &mut Context<Self>) {
        let Some(form) = &mut self.new_repo else {
            return;
        };
        let name = form.name.text().trim().to_owned();
        if !valid_repo_name(&name) {
            form.error = "A repository name is letters, digits, dot, dash or underscore".to_owned();
            cx.notify();
            return;
        }
        let hash = if form.sha1 {
            abi::HashKind::Sha1
        } else {
            abi::HashKind::Sha256
        };
        self.new_repo = None;
        self.submit(
            Op::Create {
                repo: name.clone(),
                hash,
            },
            "repos".into(),
            &format!("Creating {name}"),
            cx,
        );
    }

    pub(crate) fn start_repo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_repo = Some(NewRepo::default());
        window.focus(crate::id("forge-new-repo-name"));
        cx.notify();
    }

    pub(crate) fn cancel_repo(&mut self, cx: &mut Context<Self>) {
        self.new_repo = None;
        cx.notify();
    }

    pub(crate) fn start_change(
        &mut self,
        from: Vec<u8>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.form = Some(ChangeForm {
            repo: self.repo_name(),
            from,
            into: self.default_head(),
            ..ChangeForm::default()
        });
        window.focus(crate::id("forge-change-title"));
        cx.notify();
    }

    pub(crate) fn start_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((change, _, _, _)) = self.change() else {
            return;
        };
        self.form = Some(ChangeForm {
            repo: self.repo_name(),
            edit: Some(change.n),
            from: match &change.from {
                Revision::Ref(name) => name.clone(),
                Revision::Oid(oid) => oid.clone().into_bytes(),
            },
            into: change.into.clone(),
            title: TextField::new(change.title.clone()),
            body: TextField::new(change.body.clone()),
            reviewers: change.reviewers.clone(),
            error: String::new(),
        });
        window.focus(crate::id("forge-change-title"));
        cx.notify();
    }

    pub(crate) fn cancel_change(&mut self, cx: &mut Context<Self>) {
        self.form = None;
        cx.notify();
    }

    pub(crate) fn submit_change(&mut self, cx: &mut Context<Self>) {
        let Some(form) = &mut self.form else { return };
        if form.title.is_blank() {
            form.error = "A change needs a title".into();
            cx.notify();
            return;
        }
        if form.edit.is_none() && form.from.is_empty() {
            return;
        }
        let form = form.clone();
        self.form = None;
        let repo = form.repo.clone();
        let (op, label, scope) = match form.edit {
            Some(n) => (
                Op::ChangeEdit {
                    repo: repo.clone(),
                    n,
                    title: Some(form.title.text()),
                    body: Some(form.body.text()),
                    reviewers: Some(form.reviewers),
                },
                "Saving the change".to_owned(),
                change_key(&repo, n),
            ),
            None => (
                Op::ChangeOpen {
                    repo,
                    from: Revision::Ref(form.from.clone()),
                    into: form.into.clone(),
                    title: form.title.text(),
                    body: form.body.text(),
                    reviewers: form.reviewers.clone(),
                },
                format!("Opening “{}”", form.title.text().trim()),
                "changes".to_owned(),
            ),
        };
        self.submit(op, scope, &label, cx);
    }

    pub(crate) fn close_change(&mut self, cx: &mut Context<Self>) {
        let (Some(repo), Some(n)) = (self.nav().repo.clone(), self.nav().change) else {
            return;
        };
        let key = change_key(&repo, n);
        if self.closing.take() != Some(key.clone()) {
            self.closing = Some(key);
            cx.notify();
            return;
        }
        self.submit(
            Op::ChangeClose {
                repo: repo.clone(),
                n,
            },
            change_key(&repo, n),
            "Closing this change",
            cx,
        );
    }

    pub(crate) fn merge_block(&self) -> Option<MergeBlock> {
        let Some((change, source, target, _)) = self.change() else {
            return Some(MergeBlock::Loading);
        };
        if change.state != forge::ChangeState::Open {
            return Some(MergeBlock::NotOpen);
        }
        if source.is_none() || target.is_none() {
            return Some(MergeBlock::EndpointGone);
        }
        match self.compare().map(|c| c.mergeability) {
            None => Some(MergeBlock::Comparing),
            Some(Mergeability::FastForward) => None,
            Some(Mergeability::UpToDate) => Some(MergeBlock::UpToDate),
            Some(Mergeability::Unrelated) => Some(MergeBlock::Unrelated),
            Some(Mergeability::Diverged) => Some(MergeBlock::Diverged),
        }
    }

    pub(crate) fn merge(&mut self, cx: &mut Context<Self>) {
        if self.merge_block().is_some() {
            return;
        }
        let Some(repo) = self.nav().repo.clone() else {
            return;
        };
        let Some((change, source, target, _)) = self.change() else {
            return;
        };
        let (Some(source), Some(target)) = (source.clone(), target.clone()) else {
            return;
        };
        let (n, from, into) = (change.n, change.from.clone(), change.into.clone());
        self.submit(
            Op::Merge {
                repo: repo.clone(),
                into,
                from,
                expected_into: target,
                expected_from: source.clone(),
                result: source,
                change: Some(n),
            },
            change_key(&repo, n),
            "Merging this change",
            cx,
        );
    }

    pub(crate) fn configure(&mut self, cx: &mut Context<Self>) {
        let repo = self.repo_name();
        let Some(form) = self.repo_settings.clone() else {
            return;
        };
        self.submit(
            Op::Configure {
                repo,
                settings: Settings {
                    head: form.head,
                    allow_force: form.allow_force,
                    allow_delete: form.allow_delete,
                },
            },
            "settings".into(),
            "Saving these settings",
            cx,
        );
    }

    pub(crate) fn configure_with(
        &mut self,
        change: impl FnOnce(&mut Settings),
        cx: &mut Context<Self>,
    ) {
        let Some((info, _)) = self.repo() else {
            return;
        };
        let mut settings = info.repo.settings.clone();
        change(&mut settings);
        let repo = self.repo_name();
        self.submit(
            Op::Configure { repo, settings },
            "settings".into(),
            "Saving these settings",
            cx,
        );
    }

    pub(crate) fn grant(&mut self, cx: &mut Context<Self>) {
        let typed = self
            .repo_settings
            .as_ref()
            .map(|form| form.grant.text().trim().to_owned())
            .unwrap_or_default();
        let Some(principal) = Principal::parse(&typed) else {
            self.notice = "Grant takes an account number".into();
            cx.notify();
            return;
        };
        if let Some(form) = &mut self.repo_settings {
            form.grant.reset("");
        }
        self.grant_to(principal, cx);
    }

    pub(crate) fn grant_to(&mut self, principal: Principal, cx: &mut Context<Self>) {
        let Some(number) = principal.account() else {
            self.notice = "Grant takes an account number".into();
            cx.notify();
            return;
        };
        let Some(list) = self.writers() else {
            self.notice = words::READING_WRITERS.into();
            cx.notify();
            return;
        };
        let Some(list) = with_writer(list, Party::Account(number)) else {
            self.notice = words::WRITES_ALREADY.into();
            cx.notify();
            return;
        };
        self.set_writers(list, "Granting write access", cx);
    }

    pub(crate) fn revoke(&mut self, writer: Writer, cx: &mut Context<Self>) {
        let Some(list) = self.writers() else {
            self.notice = words::READING_WRITERS.into();
            cx.notify();
            return;
        };
        let list = without_writer(list, &writer);
        self.set_writers(list, "Revoking write access", cx);
    }

    fn set_writers(&mut self, list: Ownership, label: &str, cx: &mut Context<Self>) {
        let op = access::Op::Set {
            program: forge::PROGRAM.to_owned(),
            key: forge::repo_key(&self.repo_name()),
            list: Some(list),
        };
        let id = self.issue("settings".into(), label, cx);
        let submit = cx.host().ask::<Submit<access::Access>>(op);
        cx.land(submit, move |forge, answer, cx| {
            cx.notify();
            if !forge.settle(id, answer) {
                return;
            }
            forge.pending.retain(|op| op.id != id);
            forge.reread_writers(cx);
        })
        .detach();
    }

    // ------------------------------------------------------ conversation

    // A reply in the change's conversation: in a job's room, under the change's opening line.
    // Chat owns every reply; forge owns only the change's own body. It is a write like forge's,
    // its row under [`Forge::reply_scope`]; the box keeps its text until the write lands, so a
    // failure loses nothing, and Send waits while one is out (Expo `change.tsx` `Composer`).
    pub(crate) fn post_reply(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let text = self.reply.text().trim().to_owned();
        let Some(scope) = self.reply_scope() else {
            return;
        };
        if text.is_empty() || self.busy(&scope) {
            return;
        }
        let Some((change, _, _, _)) = self.change() else {
            return;
        };
        let channel = change.channel.clone();
        let thread = self.opening_thread();
        let id = self.issue(scope, words::REPLYING, cx);
        let host = cx.host();
        let sent = text.clone();
        let post = async move {
            let message_id = host.ask::<HostId>("message".into()).await?;
            host.ask::<Submit<::chat::Chat>>(chat::Op::PostMessage {
                channel_id: channel,
                message_id,
                blocks: chat::parse_message(&text),
                thread,
            })
            .await
        };
        cx.land(post, move |forge, answer, cx| {
            cx.notify();
            if !forge.settle(id, answer) {
                return;
            }
            forge.pending.retain(|op| op.id != id);
            if forge.reply.text().trim() == sent {
                forge.reply.reset("");
            }
            forge.refresh(cx);
        })
        .detach();
    }
}
