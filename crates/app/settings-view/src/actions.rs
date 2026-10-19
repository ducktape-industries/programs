//! What the reader's presses do: reads again, each form's submit, and the
//! invite. A refusal lands on the form it came from.
use view::prelude::*;

use crate::queries::Found;
use crate::state::{Answering, Choice, Form, Problem, Section, Sheet, TTL};
use crate::{Settings, queries};
use access::Access;
use identity::{Identity, Ownership, Party};

impl Settings {
    pub(crate) fn session_changed(&mut self, session: Session, cx: &mut Context<Self>) {
        if session.account.is_some() {
            self.create_account = Form::default();
        }
        self.session = session;
        self.policies_reseated(cx);
        self.read_account(cx);
    }

    /// The menu or the phone's strip opens a section; a sheet over the last
    /// one closes, as Back closes it.
    pub(crate) fn select_section(&mut self, section: Section, cx: &mut Context<Self>) {
        self.section = section;
        self.close_sheet(cx);
        self.close_policy_sheet(cx);
        if section == Section::Policies {
            self.refresh_policies(cx);
        }
    }

    pub(crate) fn open_sheet(&mut self, sheet: Sheet, cx: &mut Context<Self>) {
        self.sheet = Some(sheet);
        cx.notify();
    }

    /// Closes the open sheet (Back, or its Cancel). What it held and did not
    /// send goes with it, so it opens afresh next time: an agent's sheet on
    /// its actions, with no rename, no Specific that listed nobody and no
    /// person typed; an add sheet empty (Expo `Agents.tsx`'s closes). A form
    /// in flight keeps going; its landing settles it.
    pub(crate) fn close_sheet(&mut self, cx: &mut Context<Self>) {
        let unsent = |form: &mut Form| {
            if !form.busy {
                *form = Form::default();
            }
        };
        match self.sheet.take() {
            Some(Sheet::Actions(number) | Sheet::Rename(number) | Sheet::Answers(number)) => {
                self.rename_agent
                    .retain(|agent, form| *agent != number || form.busy);
                if let Some(answering) = self.answering.get_mut(&number) {
                    answering.listing = false;
                    unsent(&mut answering.pick);
                    unsent(&mut answering.form);
                }
            }
            Some(Sheet::CreateAgent) => unsent(&mut self.create_agent),
            Some(Sheet::AddKey) => unsent(&mut self.agent_key),
            None => {}
        }
        cx.notify();
    }

    /// Whom an agent answers, in its sheet: the choice, and under Specific
    /// the field that adds someone.
    pub(crate) fn open_answers(&mut self, number: u64, cx: &mut Context<Self>) {
        self.answering.entry(number).or_default();
        self.sheet = Some(Sheet::Answers(number));
        cx.notify();
    }

    /// Rename from an agent's actions: its sheet turns to the field,
    /// holding the name.
    pub(crate) fn open_rename(&mut self, number: u64, cx: &mut Context<Self>) {
        self.sheet = Some(Sheet::Rename(number));
        self.begin_rename(number, cx);
    }

    /// Who the session's key is, read afresh: the boot, a new session, a
    /// retry.
    pub(crate) fn read_account(&mut self, cx: &mut Context<Self>) {
        let work = self.account_query(cx);
        // another reader's account is not this one's: nothing of it stays
        self.account = Loadable::Idle;
        cx.load(self, work, |view| &mut view.account);
    }

    /// The same account, re-read with what is on screen kept.
    pub(crate) fn refresh_account(&mut self, cx: &mut Context<Self>) {
        let work = self.account_query(cx);
        cx.load(self, work, |view| &mut view.account);
    }

    fn account_query(
        &self,
        cx: &mut Context<Self>,
    ) -> impl Future<Output = Result<Option<queries::Seat>, view::host::Error>> + 'static {
        let (signer, number) = (self.session.signer.clone(), self.session.account);
        queries::account(cx.host(), signer, number)
    }

    pub(crate) fn submit_create_account(&mut self, cx: &mut Context<Self>) {
        if self.create_account.busy {
            return;
        }
        let name = self.create_account.text.text().trim().to_string();
        if name.is_empty() {
            self.create_account.problem = Some(Problem::Empty);
            cx.notify();
            return;
        }
        self.create_account.problem = None;
        self.create_account.busy = true;
        cx.notify();
        let op = identity::Op::Create {
            name,
            scheme: abi::Scheme::Ed25519,
        };
        let ask = cx.host().ask::<Submit<Identity>>(op);
        cx.land(ask, |view, result, cx| {
            // created: the form stays busy until the host's session names
            // the new account, which re-reads it
            if let Some(said) = design::failure(&result) {
                view.create_account.busy = false;
                view.create_account.problem = Some(Problem::Refused(said));
            }
            cx.notify();
        })
        .detach();
    }

    /// A rename begins: the agent's name becomes a field holding it. A
    /// rename already begun keeps what was typed.
    pub(crate) fn begin_rename(&mut self, number: u64, cx: &mut Context<Self>) {
        let name = self.agent_name(number).unwrap_or_default();
        self.rename_agent.entry(number).or_insert_with(|| Form {
            text: TextField::new(name),
            ..Form::default()
        });
        cx.notify();
    }

    /// Sends the agent's new name, as its rename field holds it.
    pub(crate) fn submit_rename_agent(&mut self, number: u64, cx: &mut Context<Self>) {
        let form = self.rename_agent.entry(number).or_default();
        let name = form.text.text().trim().to_string();
        if name.is_empty() {
            form.problem = Some(Problem::Empty);
            cx.notify();
            return;
        }
        let op = identity::Op::SetName {
            account: number,
            name,
        };
        let from = self.sheet;
        self.submit_agent_op(
            op,
            move |v| v.rename_agent.entry(number).or_default(),
            from,
            cx,
        );
    }

    pub(crate) fn cancel_rename_agent(&mut self, number: u64, cx: &mut Context<Self>) {
        self.rename_agent.remove(&number);
        // the sheet goes back to the agent's actions
        if self.sheet == Some(Sheet::Rename(number)) {
            self.sheet = Some(Sheet::Actions(number));
        }
        cx.notify();
    }

    fn agent_name(&self, number: u64) -> Option<String> {
        self.agent(number).map(|agent| agent.name.clone())
    }

    pub(crate) fn agent(&self, number: u64) -> Option<&queries::Agent> {
        match self.account.ready() {
            Some(Some(queries::Seat::Account(account))) => {
                account.agents.iter().find(|agent| agent.number == number)
            }
            _ => None,
        }
    }

    fn listed(&self, number: u64) -> Vec<Party> {
        self.agent(number)
            .map(|agent| agent.ownership.listed().to_vec())
            .unwrap_or_default()
    }

    pub(crate) fn pick_ownership(&mut self, number: u64, choice: Choice, cx: &mut Context<Self>) {
        match choice {
            Choice::Manager => self.answer(number, Ownership::Owner, cx),
            Choice::Specific => self.open_people(number, cx),
            Choice::Anyone => self.answer(number, Ownership::Anyone, cx),
        }
    }

    fn open_people(&mut self, number: u64, cx: &mut Context<Self>) {
        let answering = self.answering.entry(number).or_default();
        if answering.busy() {
            return;
        }
        answering.listing = true;
        cx.notify();
    }

    fn answer(&mut self, number: u64, ownership: Ownership, cx: &mut Context<Self>) {
        let answering = self.answering.entry(number).or_default();
        if answering.busy() {
            return;
        }
        answering.listing = false;
        answering.pick.problem = None;
        answering.form.problem = None;
        let unchanged = self
            .agent(number)
            .is_some_and(|agent| agent.ownership == ownership);
        if unchanged {
            cx.notify();
            return;
        }
        let from = self.sheet;
        self.submit_ownership(number, ownership, |answering| &mut answering.pick, from, cx);
    }

    pub(crate) fn submit_listed_person(&mut self, number: u64, cx: &mut Context<Self>) {
        let Some(answering) = self.answering.get_mut(&number) else {
            let opened = Answering {
                listing: true,
                ..Answering::default()
            };
            self.answering.insert(number, opened);
            cx.notify();
            return;
        };
        if answering.busy() {
            return;
        }
        answering.pick.problem = None;
        let form = &mut answering.form;
        let typed = form.text.text().trim().to_string();
        if typed.is_empty() {
            form.problem = Some(Problem::Empty);
            cx.notify();
            return;
        }
        form.busy = true;
        form.problem = None;
        cx.notify();
        // the sheet Add was pressed on, not the one open once the person is
        // found
        let from = self.sheet;
        let find = queries::find(cx.host(), typed);
        cx.land(find, move |view, found, cx| {
            view.found(number, from, found, cx)
        })
        .detach();
    }

    fn found(
        &mut self,
        number: u64,
        from: Option<Sheet>,
        found: Result<Found, view::host::Error>,
        cx: &mut Context<Self>,
    ) {
        let form = &mut self.answering.entry(number).or_default().form;
        form.busy = false;
        let problem = match found {
            Ok(Found::Party(party)) => {
                let mut listed = self.listed(number);
                listed.push(party);
                let ownership = Ownership::Listed(listed);
                return self.submit_ownership(
                    number,
                    ownership,
                    |answering| &mut answering.form,
                    from,
                    cx,
                );
            }
            Ok(Found::Nobody) => Problem::NoSuchAccount,
            Ok(Found::Several) => Problem::SeveralAccounts,
            // the look-up the write needs, worded as the write would be
            // (Expo `Agents.tsx:410`)
            Err(refusal) => Problem::Refused(
                design::failure(&Err(refusal)).expect("an error is never a landed write"),
            ),
        };
        form.problem = Some(problem);
        cx.notify();
    }

    pub(crate) fn remove_listed_person(
        &mut self,
        number: u64,
        party: &Party,
        cx: &mut Context<Self>,
    ) {
        let mut listed = self.listed(number);
        listed.retain(|listed| listed != party);
        let ownership = Ownership::Listed(listed);
        let from = self.sheet;
        self.submit_ownership(number, ownership, |answering| &mut answering.form, from, cx);
    }

    /// Sends whom an agent answers, pressed on sheet `from`; `which` of its
    /// forms carries the send (the choice's, or the person field's). Every
    /// change goes through here: one at a time, as dev's one form had it,
    /// so a Remove pressed while a choice is out cannot undo the choice.
    fn submit_ownership(
        &mut self,
        number: u64,
        ownership: Ownership,
        which: fn(&mut Answering) -> &mut Form,
        from: Option<Sheet>,
        cx: &mut Context<Self>,
    ) {
        let answering = self.answering.entry(number).or_default();
        if answering.busy() {
            return;
        }
        // the last change's problem goes with the next
        answering.pick.problem = None;
        answering.form.problem = None;
        let list = match ownership {
            Ownership::Owner => None,
            Ownership::Listed(_) | Ownership::Anyone => Some(ownership),
        };
        let op = access::Op::Set {
            program: harness::PROGRAM.into(),
            key: harness::agent_key(number),
            list,
        };
        self.land_agent_op(
            move |host| host.ask::<Submit<Access>>(op),
            move |v| which(v.answering.entry(number).or_default()),
            from,
            Some(number),
            cx,
        );
    }

    pub(crate) fn submit_create_agent(&mut self, cx: &mut Context<Self>) {
        let name = self.create_agent.text.text().trim().to_string();
        if name.is_empty() {
            self.create_agent.problem = Some(Problem::Empty);
            cx.notify();
            return;
        }
        let op = identity::Op::CreateAgent { name };
        self.submit_agent_op(op, |v| &mut v.create_agent, self.sheet, cx);
    }

    /// The agent's key request: the hex of an `AddKey` whose consent the
    /// new key signed, for one of the reader's agents; this key submits it
    /// as the manager.
    pub(crate) fn submit_agent_key(&mut self, cx: &mut Context<Self>) {
        let mine = |account: u64| {
            matches!(self.account.ready(), Some(Some(queries::Seat::Account(a)))
            if a.agents.iter().any(|agent| {
                agent.number == account && agent.standing() != identity::Standing::Revoked
            }))
        };
        let op = abi::unhex(self.agent_key.text.text().trim())
            .and_then(|bytes| abi::decode::<identity::Op>(&bytes).ok())
            .filter(
                |op| matches!(op, identity::Op::AddKey { consent, .. } if mine(consent.account)),
            );
        let Some(op) = op else {
            self.agent_key.problem = Some(Problem::NotAKeyRequest);
            cx.notify();
            return;
        };
        self.submit_agent_op(op, |v| &mut v.agent_key, self.sheet, cx);
    }

    /// Suspends, resumes or revokes an agent: one press. Suspend and
    /// Revoke are confirmed by the host, natively, before the key signs
    /// them, so the view asks nothing of its own.
    pub(crate) fn set_standing(&mut self, op: identity::Op, cx: &mut Context<Self>) {
        self.submit_agent_op(op, |v| &mut v.agent_standing, self.sheet, cx);
    }

    /// Submits `op` from `form`, pressed on sheet `from`; done, the form
    /// clears and the account is read again. A landing settles `from` only
    /// while it is still the sheet open: another opened meanwhile is not
    /// this op's to move.
    fn submit_agent_op(
        &mut self,
        op: identity::Op,
        form: impl Fn(&mut Settings) -> &mut Form + 'static,
        from: Option<Sheet>,
        cx: &mut Context<Self>,
    ) {
        self.land_agent_op(
            move |host| host.ask::<Submit<Identity>>(op),
            form,
            from,
            None,
            cx,
        );
    }

    fn land_agent_op<Asked>(
        &mut self,
        submit: impl FnOnce(&Host) -> Asked,
        form: impl Fn(&mut Settings) -> &mut Form + 'static,
        from: Option<Sheet>,
        answered: Option<u64>,
        cx: &mut Context<Self>,
    ) where
        Asked: Future<Output = Result<Submitted, view::host::Error>> + 'static,
    {
        if form(self).busy {
            return;
        }
        let pending = form(self);
        pending.busy = true;
        pending.problem = None;
        cx.notify();
        let ask = submit(&cx.host());
        cx.land(ask, move |view, result, cx| {
            let problem = design::failure(&result);
            let landed = problem.is_none();
            let form = form(view);
            form.busy = false;
            match problem {
                None => form.text.reset(""),
                Some(said) => form.problem = Some(Problem::Refused(said)),
            }
            // a rename that landed closes its field
            view.rename_agent.retain(|_, form| {
                form.busy || form.problem.is_some() || !form.text.text().is_empty()
            });
            if let Some(answering) = answered
                .filter(|_| landed)
                .and_then(|number| view.answering.get_mut(&number))
            {
                answering.form.text.reset("");
            }
            if landed && view.sheet == from {
                view.sheet = from.and_then(Sheet::after_landing);
            }
            view.refresh_account(cx);
            cx.notify();
        })
        .detach();
    }

    pub(crate) fn mint_invite(&mut self, cx: &mut Context<Self>) {
        self.copied = Loadable::Idle;
        let ask = cx.host().ask::<InviteCreate>(CreateInvite {
            ttl_days: TTL[self.ttl],
        });
        // a new invite: the one shown is no longer the one being minted
        self.invite = Loadable::Idle;
        cx.load(self, ask, |view| &mut view.invite);
    }

    pub(crate) fn copy_invite(&mut self, cx: &mut Context<Self>) {
        let Some(invite) = self.invite.ready() else {
            return;
        };
        let ask = cx.host().ask::<ClipboardWrite>(invite.invite.clone());
        // each press copies again and says so again
        self.copied = Loadable::Idle;
        cx.load(self, ask, |view| &mut view.copied);
    }
}
