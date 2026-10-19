//! What the Policies pane's presses do: its reads (the reader's policies, and the items on one
//! policy for its Remove and Move sheets), its sheets, and each op it sends the ownership program.
//! A refusal lands on the sheet it came from.
use ownership::{Ownership, Party, Scope, Writers};
use view::methods::Change;
use view::prelude::*;

use crate::queries::{self, Found};
use crate::state::{Choice, Form, PolicyForm, PolicySheet, Problem, Section};
use crate::{Settings, words};

impl Settings {
    // The reader as ownership knows an owner: their account.
    fn owner(&self) -> Option<Party> {
        self.session.account.map(Party::Account)
    }

    // The reader's policies and defaults, read again with what is on screen kept: the pane
    // opened, ownership wrote, an op landed, a retry.
    pub(crate) fn refresh_policies(&mut self, cx: &mut Context<Self>) {
        let Some(owner) = self.owner() else {
            self.holdings = Loadable::Idle;
            return;
        };
        let work = policy_view::holdings(cx.host(), owner);
        cx.load(self, work, |view| &mut view.holdings);
    }

    // Another reader is seated: nothing of the last one's policies stays.
    pub(crate) fn policies_reseated(&mut self, cx: &mut Context<Self>) {
        self.holdings = Loadable::Idle;
        self.policy_sheet = None;
        if self.section == Section::Policies {
            self.refresh_policies(cx);
        }
    }

    // Ownership wrote: the open pane reads again when the block touched what it shows.
    pub(crate) fn ownership_moved(&mut self, change: Option<Change>, cx: &mut Context<Self>) {
        if self.shown() != Section::Policies {
            return;
        }
        let Some(owner) = self.owner() else {
            return;
        };
        let touched = match change {
            None => true,
            Some(change) => [
                ownership::Query::Policies {
                    owner: owner.clone(),
                    page: ownership::PageRequest::default(),
                },
                ownership::Query::Defaults {
                    owner: owner.clone(),
                    page: ownership::PageRequest::default(),
                },
                ownership::Query::Open { owner },
            ]
            .iter()
            .any(|query| change.touches::<Ownership, _>(query)),
        };
        if touched {
            self.refresh_policies(cx);
        }
    }

    pub(crate) fn open_policy_sheet(&mut self, sheet: PolicySheet, cx: &mut Context<Self>) {
        if !self.policy_write.busy {
            self.policy_write = Form::default();
        }
        match &sheet {
            PolicySheet::Edit(id) => self.policy_form = self.form_for(*id),
            PolicySheet::Remove(id) | PolicySheet::Move(id) => {
                let work = policy_view::items(cx.host(), *id);
                self.policy_items = Loadable::Idle;
                cx.load(self, work, |view| &mut view.policy_items);
            }
            PolicySheet::Actions(_) | PolicySheet::Default(_) => {}
        }
        self.policy_sheet = Some(sheet);
        cx.notify();
    }

    // A new policy's form (no `id`), or one holding policy `id` as it is.
    fn form_for(&self, id: Option<u64>) -> PolicyForm {
        let Some(holdings) = self.holdings.ready() else {
            return PolicyForm::default();
        };
        let names = holdings.names.clone();
        let Some(policy) = id.and_then(|id| holdings.policy(id)) else {
            return PolicyForm {
                names,
                ..PolicyForm::default()
            };
        };
        let (grant, listed) = match &policy.write {
            Writers::Anyone => (Choice::Anyone, Vec::new()),
            Writers::Owner => (Choice::Manager, Vec::new()),
            Writers::Listed(parties) => (Choice::Specific, parties.clone()),
        };
        PolicyForm {
            name: Form {
                text: TextField::new(policy.name.clone()),
                ..Form::default()
            },
            grant,
            listed,
            names,
            person: Form::default(),
        }
    }

    // Closes the policy sheet (its Cancel, a dismiss, Back); what it held and did not send goes
    // with it. A write in flight keeps going, and its landing settles it.
    pub(crate) fn close_policy_sheet(&mut self, cx: &mut Context<Self>) {
        self.policy_sheet = None;
        if !self.policy_form.name.busy && !self.policy_form.person.busy {
            self.policy_form = PolicyForm::default();
        }
        if !self.policy_write.busy {
            self.policy_write = Form::default();
        }
        self.policy_items = Loadable::Idle;
        cx.notify();
    }

    // Back on the phone: the policy sheet closes, else the Agents pane's.
    pub(crate) fn back(&mut self, cx: &mut Context<Self>) {
        match self.policy_sheet {
            Some(_) => self.close_policy_sheet(cx),
            None => self.close_sheet(cx),
        }
    }

    pub(crate) fn pick_grant(&mut self, grant: Choice, cx: &mut Context<Self>) {
        self.policy_form.grant = grant;
        cx.notify();
    }

    // The person typed in the policy form, found by name or number, joins its list.
    pub(crate) fn add_policy_person(&mut self, cx: &mut Context<Self>) {
        let person = &mut self.policy_form.person;
        if person.busy {
            return;
        }
        let typed = person.text.text().trim().to_string();
        if typed.is_empty() {
            person.problem = Some(Problem::Empty);
            cx.notify();
            return;
        }
        person.busy = true;
        person.problem = None;
        cx.notify();
        let find = queries::find(cx.host(), typed.clone());
        cx.land(find, move |view, found, cx| {
            let form = &mut view.policy_form;
            form.person.busy = false;
            form.person.problem = match found {
                Ok(Found::Party(identity::Party::Account(number))) => {
                    // a name typed is the name it goes by until the policies are read again
                    if typed.trim_start_matches('#').parse::<u64>().is_err() {
                        form.names.insert(number, typed);
                    }
                    form.listed(Party::Account(number));
                    None
                }
                Ok(Found::Party(identity::Party::Program(program))) => {
                    form.listed(Party::Program(program));
                    None
                }
                Ok(Found::Nobody) => Some(Problem::NoSuchAccount),
                Ok(Found::Several) => Some(Problem::SeveralAccounts),
                Err(refusal) => Some(Problem::Refused(
                    design::failure(&Err(refusal)).expect("an error is never a landed write"),
                )),
            };
            cx.notify();
        })
        .detach();
    }

    pub(crate) fn remove_policy_person(&mut self, party: &Party, cx: &mut Context<Self>) {
        self.policy_form.listed.retain(|listed| listed != party);
        cx.notify();
    }

    // Saves what the policy form holds: a new policy, or the one its sheet edits.
    pub(crate) fn save_policy(&mut self, cx: &mut Context<Self>) {
        let Some(PolicySheet::Edit(editing)) = self.policy_sheet.clone() else {
            return;
        };
        let form = &mut self.policy_form;
        let name = form.name.text.text().trim().to_string();
        if name.is_empty() {
            form.name.problem = Some(Problem::Empty);
            cx.notify();
            return;
        }
        let write = match form.grant {
            Choice::Anyone => Writers::Anyone,
            Choice::Manager => Writers::Owner,
            Choice::Specific => Writers::Listed(form.listed.clone()),
        };
        let op = match editing {
            Some(policy) => ownership::Op::Edit {
                policy,
                name,
                write,
            },
            None => ownership::Op::Create { name, write },
        };
        self.land_policy_op(op, |view| &mut view.policy_form.name, cx);
    }

    pub(crate) fn retire_policy(&mut self, policy: u64, cx: &mut Context<Self>) {
        let op = ownership::Op::Retire { policy };
        self.land_policy_op(op, |view| &mut view.policy_write, cx);
    }

    pub(crate) fn switch_policy(&mut self, from: u64, to: u64, cx: &mut Context<Self>) {
        let op = ownership::Op::Switch { from, to };
        self.land_policy_op(op, |view| &mut view.policy_write, cx);
    }

    // What new items in `scope` use: one of the reader's policies, or (none) the wider scope's.
    pub(crate) fn set_default(
        &mut self,
        scope: Scope,
        policy: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        let unchanged = self
            .holdings
            .ready()
            .is_some_and(|holdings| holdings.default_of(&scope) == policy);
        if unchanged {
            self.close_policy_sheet(cx);
            return;
        }
        let op = ownership::Op::SetDefault { scope, policy };
        self.land_policy_op(op, |view| &mut view.policy_write, cx);
    }

    // Sends `op` from `form`; landed, the sheet it was sent from closes while it is still the one
    // open, and the pane reads again.
    fn land_policy_op(
        &mut self,
        op: ownership::Op,
        form: fn(&mut Settings) -> &mut Form,
        cx: &mut Context<Self>,
    ) {
        let pending = form(self);
        if pending.busy {
            return;
        }
        pending.busy = true;
        pending.problem = None;
        cx.notify();
        let from = self.policy_sheet.clone();
        let ask = cx.host().ask::<Submit<Ownership>>(op);
        cx.land(ask, move |view, result, cx| {
            let problem = design::failure(&result);
            let landed = problem.is_none();
            let sent = form(view);
            sent.busy = false;
            sent.problem = problem.map(Problem::Refused);
            if landed && view.policy_sheet == from {
                view.close_policy_sheet(cx);
            }
            view.refresh_policies(cx);
            cx.notify();
        })
        .detach();
    }
}

impl PolicyForm {
    // `party` joins the list, once; the field it was typed in empties.
    fn listed(&mut self, party: Party) {
        if !self.listed.contains(&party) {
            self.listed.push(party);
        }
        self.person.text.reset("");
    }
}

// What a refused or empty policy write says, under the control that sent it.
pub(crate) fn problem_text(form: &Form, empty: &str) -> Option<String> {
    form.problem
        .as_ref()
        .map(|problem| words::problem_text(problem, empty))
}
