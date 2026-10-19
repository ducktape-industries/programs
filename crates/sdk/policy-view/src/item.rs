//! "Who can edit" on one item a view keeps, alike in every view that links it: whose it is and its
//! policy in words, and for its owner the sheets that relink it to another of their policies and
//! transfer it. Generic over the view through a lens to the slot (`Editing`) the view keeps.
use std::collections::BTreeMap;

use ownership::{Ownership, Party, Policy, PolicyId, Record, Writers};
use view::design::{Kind as ButtonKind, Ladder, SHEET_INSET};
use view::host::Error;
use view::methods::Change;
use view::prelude::*;
use view::{Div, Stateful};

use crate::{Holdings, holdings, party, writers};

// The row's title, and the menu item a view opens its sheet from.
pub const WHO: &str = "Who can edit";
pub(crate) const READING: &str = "Reading who can edit it…";
pub(crate) const BEFORE: &str = "Open (from before policies)";
const POLICIES: &str = "Reading your policies…";
const RELINK_HELP: &str = "Pick the policy that says who may change it beside you.";
const TRANSFER_HELP: &str = "Who should own it: their name or account number.";
const PERSON: &str = "New owner";
const EMPTY: &str = "Type a name or an account number.";
const NOBODY: &str = "No account goes by that.";
const SEVERAL: &str = "Several accounts go by that name: type the number.";
const WORKING: &str = "Working…";
const FINDING: &str = "Finding…";
const CANCEL: &str = "Cancel";

// An item a view keeps: the program and the key ownership records it under, and what a person
// calls it ("#general", "this message").
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub program: String,
    pub key: Vec<u8>,
    pub label: String,
}

impl Item {
    fn is(&self, other: &Item) -> bool {
        self.program == other.program && self.key == other.key
    }
}

// What ownership says of an item, read whole: its record and the policy that governs it (none for
// an item from before policies), the names of the accounts they name, whether the reader owns it,
// and whether its policy lets them change it (`None` with no record: the view keeps the rule it
// had before policies).
#[derive(Clone, Debug, PartialEq)]
pub struct Standing {
    pub record: Option<Record>,
    pub policy: Option<Policy>,
    pub names: BTreeMap<u64, String>,
    pub owns: bool,
    pub admits: Option<bool>,
}

// The sheet open over the view: what the item's row says, the owner's policies to relink it to,
// or the transfer (an account found, then a confirm that names it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sheet {
    About,
    Relink,
    Transfer,
}

// The slot a view keeps per item it shows a row for.
#[derive(Default)]
pub struct Editing {
    pub item: Option<Item>,
    // whom `standing` was read for
    pub reader: Option<Party>,
    pub standing: Loadable<Standing>,
    pub sheet: Option<Sheet>,
    // the owner's policies, read when Change policy… opens
    pub holdings: Loadable<Holdings>,
    pub person: TextField,
    // whom a transfer goes to, found by what was typed, with their name
    pub found: Option<(Party, String)>,
    pub busy: bool,
    pub problem: Option<String>,
}

pub type Lens<V> = fn(&mut V) -> &mut Editing;

impl Editing {
    // Whether its policy lets the reader change `item`, once read; `None` before then, and for an
    // item with no record.
    pub fn admits(&self, item: &Item) -> Option<bool> {
        let held = self.item.as_ref().is_some_and(|held| held.is(item));
        let standing = self.standing.ready().filter(|_| held)?;
        standing.admits
    }

    pub fn owns(&self) -> bool {
        self.standing.ready().is_some_and(|standing| standing.owns)
    }
}

// `item` is the one the slot shows, for `reader`: read when it is another item or reader than the
// one held, else only its words are kept.
pub fn hold<V: 'static>(
    view: &mut V,
    lens: Lens<V>,
    item: Item,
    reader: Option<Party>,
    cx: &mut Context<V>,
) {
    let editing = lens(view);
    let same = editing.item.as_ref().is_some_and(|held| held.is(&item));
    if same && editing.reader == reader {
        editing.item = Some(item);
        return;
    }
    *editing = Editing {
        item: Some(item),
        reader,
        ..Editing::default()
    };
    reread(view, lens, cx);
}

// Nothing is shown of an item any more: the slot empties.
pub fn let_go<V>(view: &mut V, lens: Lens<V>) {
    let editing = lens(view);
    if editing.item.is_some() {
        *editing = Editing::default();
    }
}

// The held item's standing, read again with what is shown kept.
pub fn reread<V: 'static>(view: &mut V, lens: Lens<V>, cx: &mut Context<V>) {
    let editing = lens(view);
    let Some(item) = editing.item.clone() else {
        return;
    };
    let work = standing(cx.host(), item, editing.reader.clone());
    cx.load(view, work, move |view| &mut lens(view).standing);
}

// Ownership wrote: the held item is read again where the block touched its record or policy, and
// the owner's policies where the Relink sheet shows them.
pub fn moved<V: 'static>(
    view: &mut V,
    lens: Lens<V>,
    change: Option<&Change>,
    cx: &mut Context<V>,
) {
    let editing = lens(view);
    let Some(item) = editing.item.clone() else {
        return;
    };
    let relinking = editing.sheet == Some(Sheet::Relink);
    let owner = editing
        .standing
        .ready()
        .and_then(|standing| standing.record.as_ref())
        .map(|record| record.owner.clone());
    let touched = |query: ownership::Query| {
        change.is_none_or(|change| change.touches::<Ownership, _>(&query))
    };
    let record = ownership::Query::Record {
        program: item.program.clone(),
        key: item.key.clone(),
    };
    let effective = ownership::Query::Effective {
        program: item.program,
        key: item.key,
    };
    if touched(record) || touched(effective) {
        reread(view, lens, cx);
    }
    if let Some(owner) = owner.filter(|_| relinking) {
        let policies = ownership::Query::Policies {
            owner,
            page: ownership::PageRequest::default(),
        };
        if touched(policies) {
            read_holdings(view, lens, cx);
        }
    }
}

// What ownership says of `item`, for `reader`.
pub async fn standing(host: Host, item: Item, reader: Option<Party>) -> Result<Standing, Error> {
    let record = host
        .query(ownership::ask::Record {
            program: item.program.clone(),
            key: item.key.clone(),
        })
        .await?;
    let policy = host
        .query(ownership::ask::Effective {
            program: item.program,
            key: item.key,
        })
        .await?;
    let mut numbers = Vec::new();
    if let Some(Record {
        owner: Party::Account(number),
        ..
    }) = &record
    {
        numbers.push(*number);
    }
    if let Some(Policy {
        write: Writers::Listed(parties),
        ..
    }) = &policy
    {
        numbers.extend(parties.iter().filter_map(|party| match party {
            Party::Account(number) => Some(*number),
            Party::Program(_) => None,
        }));
    }
    let mut names = BTreeMap::new();
    for number in numbers {
        if names.contains_key(&number) {
            continue;
        }
        if let Some(profile) = host.query(identity::ask::Profile { number }).await? {
            names.insert(number, profile.name);
        }
    }
    let (owns, admits) = match (&record, &reader) {
        (Some(record), Some(reader)) => {
            let owns = is(&host, &record.owner, reader).await?;
            let admits = match &policy {
                Some(policy) if !owns => lets(&host, policy, reader).await?,
                _ => owns,
            };
            (owns, Some(admits))
        }
        // nobody seated changes anything
        (Some(_), None) => (false, Some(false)),
        (None, _) => (false, None),
    };
    Ok(Standing {
        record,
        policy,
        names,
        owns,
        admits,
    })
}

// Whether `policy` lets `reader` write beside its items' owner.
async fn lets(host: &Host, policy: &Policy, reader: &Party) -> Result<bool, Error> {
    match &policy.write {
        Writers::Anyone => Ok(true),
        Writers::Owner => Ok(false),
        Writers::Listed(parties) => {
            for party in parties {
                if is(host, party, reader).await? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
    }
}

// Whether `party` is `reader` as ownership counts them: the same party, or a program and the
// account it acts as.
async fn is(host: &Host, party: &Party, reader: &Party) -> Result<bool, Error> {
    match (party, reader) {
        (party, reader) if party == reader => Ok(true),
        (Party::Program(program), Party::Account(number))
        | (Party::Account(number), Party::Program(program)) => {
            let asked = identity::ask::OfProgram {
                program: program.clone(),
            };
            Ok(host.query(asked).await? == Some(*number))
        }
        _ => Ok(false),
    }
}

// A party a person typed, by name or number, with the name it goes by.
pub enum Found {
    Party(Party, String),
    Nobody,
    Several,
}

// The account (or program) `typed` names: `#12` or `12` by number, else every account whose name
// is it, letter case aside.
pub async fn find(host: Host, typed: String) -> Result<Found, Error> {
    let numbered = typed.strip_prefix('#').unwrap_or(&typed).parse::<u64>();
    if let Ok(number) = numbered {
        return Ok(match host.query(identity::ask::Profile { number }).await? {
            Some(profile) => Found::Party(Party::Account(number), profile.name),
            None => Found::Nobody,
        });
    }
    let wanted = typed.to_lowercase();
    let accounts = host
        .query_all(|after| identity::ask::List {
            page: identity::PageRequest { after, limit: None },
        })
        .await?;
    let mut matched = accounts
        .into_iter()
        .filter(|account| account.card.name.to_lowercase() == wanted)
        .map(|account| {
            let party = match account.kind() {
                identity::Kind::System(program) => Party::Program(program),
                identity::Kind::User | identity::Kind::Program { .. } => {
                    Party::Account(account.number)
                }
            };
            (party, account.card.name)
        });
    Ok(match (matched.next(), matched.next()) {
        (Some((party, name)), None) => Found::Party(party, name),
        (None, _) => Found::Nobody,
        (Some(_), Some(_)) => Found::Several,
    })
}

pub fn open_sheet<V: 'static>(view: &mut V, lens: Lens<V>, sheet: Sheet, cx: &mut Context<V>) {
    let editing = lens(view);
    if !editing.busy {
        editing.problem = None;
    }
    editing.found = None;
    editing.person.reset("");
    editing.sheet = Some(sheet);
    if sheet == Sheet::Relink {
        read_holdings(view, lens, cx);
    }
    cx.notify();
}

// The sheet closes (its Cancel, a dismiss, Back); a write in flight keeps going.
pub fn close_sheet<V: 'static>(view: &mut V, lens: Lens<V>, cx: &mut Context<V>) {
    let editing = lens(view);
    editing.sheet = None;
    editing.found = None;
    if !editing.busy {
        editing.problem = None;
    }
    cx.notify();
}

fn read_holdings<V: 'static>(view: &mut V, lens: Lens<V>, cx: &mut Context<V>) {
    let owner = lens(view)
        .standing
        .ready()
        .and_then(|standing| standing.record.as_ref())
        .map(|record| record.owner.clone());
    let Some(owner) = owner else {
        return;
    };
    let work = holdings(cx.host(), owner);
    cx.load(view, work, move |view| &mut lens(view).holdings);
}

// The held item is linked to the owner's policy `policy`.
pub fn relink<V: 'static>(view: &mut V, lens: Lens<V>, policy: PolicyId, cx: &mut Context<V>) {
    let Some(item) = lens(view).item.clone() else {
        return;
    };
    let op = ownership::Op::Relink {
        program: item.program,
        key: item.key,
        policy,
    };
    send(view, lens, op, cx);
}

// The account typed in the Transfer sheet, found: the sheet asks to confirm it next.
pub fn find_owner<V: 'static>(view: &mut V, lens: Lens<V>, cx: &mut Context<V>) {
    let editing = lens(view);
    if editing.busy {
        return;
    }
    let typed = editing.person.text().trim().to_owned();
    if typed.is_empty() {
        editing.problem = Some(EMPTY.to_owned());
        cx.notify();
        return;
    }
    editing.busy = true;
    editing.problem = None;
    cx.notify();
    let found = find(cx.host(), typed);
    cx.land(found, move |view, found, cx| {
        let editing = lens(view);
        editing.busy = false;
        match found {
            Ok(Found::Party(party, name)) => editing.found = Some((party, name)),
            Ok(Found::Nobody) => editing.problem = Some(NOBODY.to_owned()),
            Ok(Found::Several) => editing.problem = Some(SEVERAL.to_owned()),
            Err(refusal) => editing.problem = Some(design::sentence(&refusal)),
        }
        cx.notify();
    })
    .detach();
}

// The held item goes to the account its Transfer sheet found.
pub fn transfer<V: 'static>(view: &mut V, lens: Lens<V>, cx: &mut Context<V>) {
    let editing = lens(view);
    let (Some(item), Some((to, _))) = (editing.item.clone(), editing.found.clone()) else {
        return;
    };
    let op = ownership::Op::Transfer {
        program: item.program,
        key: item.key,
        to,
    };
    send(view, lens, op, cx);
}

// Sends `op`; landed, the sheet it was sent from closes while it is still the one open, and the
// item is read again. A refusal stays on the sheet.
fn send<V: 'static>(view: &mut V, lens: Lens<V>, op: ownership::Op, cx: &mut Context<V>) {
    let editing = lens(view);
    if editing.busy {
        return;
    }
    editing.busy = true;
    editing.problem = None;
    let from = editing.sheet;
    cx.notify();
    let ask = cx.host().ask::<Submit<Ownership>>(op);
    cx.land(ask, move |view, result, cx| {
        let problem = design::failure(&result);
        let editing = lens(view);
        editing.busy = false;
        if problem.is_none() && editing.sheet == from {
            editing.sheet = None;
            editing.found = None;
        }
        editing.problem = problem;
        reread(view, lens, cx);
        cx.notify();
    })
    .detach();
}

// The row, while an item is held: whose it is and its policy in words. Its owner's press opens the
// sheet that changes either; anyone else reads it.
pub fn row<V: 'static>(
    editing: &Editing,
    id: &str,
    lens: Lens<V>,
    cx: &mut Context<V>,
    ladder: &Ladder,
    theme: &Theme,
) -> Option<Stateful<Div>> {
    editing.item.as_ref()?;
    let row =
        design::row(id.to_owned(), ladder, theme)
            .min_h(ladder.row_tall)
            .child(design::lead(
                design::icon("shield", ladder.icon_row).text_color(theme.muted),
            ))
            .child(
                design::row_words(design::row_title(WHO, ladder, theme))
                    .child(design::row_caption(said(editing), ladder, theme)),
            );
    if !editing.owns() {
        return Some(row);
    }
    let press = cx.listener(move |view: &mut V, _: &ClickEvent, _, cx| {
        open_sheet(view, lens, Sheet::About, cx)
    });
    Some(
        row.role(Role::Button)
            .focusable()
            .on_click(press)
            .child(design::chevron(ladder, theme)),
    )
}

// What the row says: whose the item is and its policy, or why that is not known yet.
pub(crate) fn said(editing: &Editing) -> String {
    match &editing.standing {
        Loadable::Ready(standing) | Loadable::Reloading(standing, _) => {
            about(standing, editing.reader.as_ref())
        }
        Loadable::Failed(refusal) => design::sentence(refusal),
        Loadable::Idle | Loadable::Loading(_) => READING.to_owned(),
    }
}

// "You · Team · You, Ada and board", "Maya · Open · Anyone", or for an item no record names, what
// it is before policies.
pub(crate) fn about(standing: &Standing, reader: Option<&Party>) -> String {
    let (Some(record), Some(policy)) = (&standing.record, &standing.policy) else {
        return BEFORE.to_owned();
    };
    let owner = match standing.owns {
        true => "You".to_owned(),
        false => party(&record.owner, &standing.names),
    };
    let who = writers(&policy.write, &record.owner, reader, &standing.names);
    format!("{owner} · {} · {who}", policy.name)
}

fn title(sheet: Sheet, item: &Item) -> String {
    match sheet {
        Sheet::About => format!("{WHO} {}", item.label),
        Sheet::Relink => format!("Change the policy of {}", item.label),
        Sheet::Transfer => format!("Transfer {}", item.label),
    }
}

// The sheet open over the view, if one is: `id` leads every id in it.
pub fn sheet<V: 'static>(
    editing: &Editing,
    id: &str,
    lens: Lens<V>,
    cx: &mut Context<V>,
    ladder: &Ladder,
    theme: &Theme,
) -> Option<AnyElement> {
    let open = editing.sheet?;
    let item = editing.item.as_ref()?;
    // Change policy… and Transfer… need the item's record: until it is read, the sheet says so
    let (shown, body) = match (open, editing.standing.ready()) {
        (Sheet::Relink, Some(_)) => (open, relinking(editing, item, id, lens, cx, ladder, theme)),
        (Sheet::Transfer, Some(_)) => (
            open,
            transferring(editing, item, id, lens, cx, ladder, theme),
        ),
        _ => (
            Sheet::About,
            about_sheet(editing, item, id, lens, cx, ladder, theme),
        ),
    };
    let close = cx.listener(move |view: &mut V, _: &(), _, cx| close_sheet(view, lens, cx));
    let sheet = design::sheet(format!("{id}/sheet"), title(shown, item), ladder, theme)
        .on_dismiss(close)
        .child(body);
    Some(sheet.into_any_element())
}

// Whose it is and its policy; for its owner, Change policy… and Transfer….
fn about_sheet<V: 'static>(
    editing: &Editing,
    item: &Item,
    id: &str,
    lens: Lens<V>,
    cx: &mut Context<V>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let to = |sheet: Sheet| {
        cx.listener(move |view: &mut V, _: &ClickEvent, _, cx| open_sheet(view, lens, sheet, cx))
    };
    let relink = design::sheet_row(
        format!("{id}/relink"),
        "Change policy…",
        ladder,
        theme,
        to(Sheet::Relink),
    )
    .icon("shield");
    let transfer = design::sheet_row(
        format!("{id}/transfer"),
        "Transfer…",
        ladder,
        theme,
        to(Sheet::Transfer),
    )
    .icon("send");
    div()
        .flex()
        .flex_col()
        .child(design::sheet_title(
            format!("{id}/head"),
            title(Sheet::About, item),
            ladder,
            theme,
        ))
        .child(
            div()
                .id(format!("{id}/about"))
                .px(SHEET_INSET)
                .py(design::space::LG)
                .child(note(said(editing), theme.foreground, ladder)),
        )
        .when(editing.owns(), |column| {
            column.child(relink).child(transfer)
        })
        .into_any_element()
}

// The owner's live policies, the one in use marked; a press links the item to it.
fn relinking<V: 'static>(
    editing: &Editing,
    item: &Item,
    id: &str,
    lens: Lens<V>,
    cx: &mut Context<V>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let linked = editing
        .standing
        .ready()
        .and_then(|standing| standing.record.as_ref())
        .map(|record| record.policy);
    let rows: Vec<AnyElement> = match &editing.holdings {
        Loadable::Ready(holdings) | Loadable::Reloading(holdings, _) => {
            relink_rows(holdings, linked, id, lens, cx, ladder, theme)
        }
        Loadable::Failed(refusal) => {
            vec![line(design::sentence(refusal), theme.danger, ladder).into_any_element()]
        }
        Loadable::Idle | Loadable::Loading(_) => {
            vec![line(POLICIES, theme.muted, ladder).into_any_element()]
        }
    };
    let busy = editing.busy.then(|| line(WORKING, theme.muted, ladder));
    div()
        .flex()
        .flex_col()
        .child(design::sheet_title(
            format!("{id}/relink/head"),
            title(Sheet::Relink, item),
            ladder,
            theme,
        ))
        .child(line(RELINK_HELP, theme.muted, ladder))
        .children(rows)
        .children(busy)
        .children(refused(editing, ladder, theme))
        .into_any_element()
}

fn relink_rows<V: 'static>(
    holdings: &Holdings,
    linked: Option<PolicyId>,
    id: &str,
    lens: Lens<V>,
    cx: &mut Context<V>,
    ladder: &Ladder,
    theme: &Theme,
) -> Vec<AnyElement> {
    holdings
        .live()
        .map(|policy| {
            let to = policy.id;
            let press =
                cx.listener(move |view: &mut V, _: &ClickEvent, _, cx| relink(view, lens, to, cx));
            let who = holdings.writers(policy);
            let hint = match linked == Some(to) {
                true => format!("In use · {who}"),
                false => who,
            };
            design::sheet_row(
                format!("{id}/relink/{to}"),
                policy.name.clone(),
                ladder,
                theme,
                press,
            )
            .hint(hint)
            .into_any_element()
        })
        .collect()
}

// Whom it goes to, typed and found; then a confirm that names them (Q3: a plain confirm).
fn transferring<V: 'static>(
    editing: &Editing,
    item: &Item,
    id: &str,
    lens: Lens<V>,
    cx: &mut Context<V>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let busy = editing.busy;
    let cancel =
        cx.listener(move |view: &mut V, _: &ClickEvent, _, cx| close_sheet(view, lens, cx));
    let body = match &editing.found {
        Some((_, name)) => {
            let confirm =
                cx.listener(move |view: &mut V, _: &ClickEvent, _, cx| transfer(view, lens, cx));
            let said = format!(
                "{name} becomes the owner of {}. You can't take it back: only {name} can give it \
                 back to you.",
                item.label
            );
            div()
                .id(format!("{id}/transfer/confirm"))
                .flex()
                .flex_col()
                .gap(design::space::LG)
                .p(SHEET_INSET)
                .child(note(said, theme.foreground, ladder))
                .children(
                    editing
                        .problem
                        .clone()
                        .map(|problem| note(problem, theme.danger, ladder)),
                )
                .child(verbs(
                    &format!("{id}/transfer"),
                    "Transfer",
                    busy.then_some(WORKING),
                    confirm,
                    cancel,
                    ladder,
                    theme,
                ))
        }
        None => {
            let input = Input::new(
                format!("{id}/transfer/person"),
                &editing.person,
                PERSON.to_owned(),
            )
            .placeholder("Ada or #12")
            .disabled(busy)
            .autocorrect(false)
            .autocapitalize(Autocapitalize::None);
            let input = match busy {
                true => input,
                false => input.on_submit(
                    cx.listener(move |view: &mut V, _: &(), _, cx| find_owner(view, lens, cx)),
                ),
            };
            let find =
                cx.listener(move |view: &mut V, _: &ClickEvent, _, cx| find_owner(view, lens, cx));
            div()
                .id(format!("{id}/transfer/find"))
                .flex()
                .flex_col()
                .gap(design::space::LG)
                .p(SHEET_INSET)
                .child(note(TRANSFER_HELP, theme.muted, ladder))
                .child(
                    design::field(input, ladder, theme)
                        .label(PERSON)
                        .when_some(editing.problem.clone(), design::Field::problem),
                )
                .child(verbs(
                    &format!("{id}/find"),
                    "Find",
                    busy.then_some(FINDING),
                    find,
                    cancel,
                    ladder,
                    theme,
                ))
        }
    };
    div()
        .flex()
        .flex_col()
        .child(design::sheet_title(
            format!("{id}/transfer/head"),
            title(Sheet::Transfer, item),
            ladder,
            theme,
        ))
        .child(body)
        .into_any_element()
}

// A sentence in the secondary size, in `tone`.
fn note(text: impl Into<SharedString>, tone: Hsla, ladder: &Ladder) -> Div {
    div()
        .text_type(ladder.secondary)
        .text_color(tone)
        .child(text.into())
}

// A sentence across the sheet, inset as its rows are.
fn line(text: impl Into<SharedString>, tone: Hsla, ladder: &Ladder) -> Div {
    div()
        .px(SHEET_INSET)
        .py(design::space::SM)
        .child(note(text, tone, ladder))
}

// What stopped the sheet's write, under it.
fn refused(editing: &Editing, ladder: &Ladder, theme: &Theme) -> Option<Div> {
    let said = editing.problem.clone()?;
    Some(line(said, theme.danger, ladder))
}

// A sheet form's buttons, `{base}/cancel` then `{base}/submit`; the verb says `busy` and takes no
// press while its write is in flight.
fn verbs(
    base: &str,
    verb: &str,
    busy: Option<&str>,
    pressed: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ladder: &Ladder,
    theme: &Theme,
) -> Div {
    design::form_verbs(
        design::button(format!("{base}/cancel"), CANCEL, ladder, theme, cancel)
            .kind(ButtonKind::Quiet),
        design::button(
            format!("{base}/submit"),
            busy.unwrap_or(verb),
            ladder,
            theme,
            pressed,
        )
        .kind(ButtonKind::Primary)
        .busy(busy.is_some()),
    )
}
