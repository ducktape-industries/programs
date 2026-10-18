// The rules: who acts as an account, then the ops and queries.

use ::program::{
    Error, ExecCtx, Need, Origin, Ownership, Party, ProgramId, QueryCtx, Resource, Scheme,
    already_exists, invalid, not_found, unauthorized, wrong_state,
};
use store::{Item, Map, Set};

use crate::{
    Account, AccountNumber, Admission, CONSENT_NAMESPACE, Card, Category, Consent, Control, Key,
    Life, Op, Reference, Reply, Script,
};

pub(crate) const ACCOUNTS: Map<AccountNumber, Account> = Map::new("a/");
pub(crate) const OF_KEY: Map<Vec<u8>, AccountNumber> = Map::new("k/");
pub(crate) const OF_PROGRAM: Map<ProgramId, AccountNumber> = Map::new("m/");
const GENERATION: Map<Vec<u8>, u64> = Map::new("g/");
/// `(manager, managed)`: the program accounts an account manages, revoked
/// ones too, so a manager's list keeps what they answered for.
pub(crate) const MANAGED: Set<(AccountNumber, AccountNumber)> = Set::new("d/");
const NEXT: Item<AccountNumber> = Item::new("next");

pub(crate) fn account(ctx: &QueryCtx, number: AccountNumber) -> Result<Account, Error> {
    ACCOUNTS
        .get(ctx, &number)?
        .ok_or_else(|| not_found(format!("account {number}")))
}

/// The account a frame signed by `key` acts as: refused while it does not
/// act, so the host rejects the frame. A user and a system account always
/// act; a program account while its manager keeps it active (a revoked one
/// holds no key to be asked about).
pub(crate) fn of_key(ctx: &QueryCtx, key: &Vec<u8>) -> Result<Option<AccountNumber>, Error> {
    let Some(number) = OF_KEY.get(ctx, key)? else {
        return Ok(None);
    };
    match account(ctx, number)?.control {
        Control::User { .. }
        | Control::System { .. }
        | Control::Program {
            life: Life::Active { .. },
            ..
        } => Ok(Some(number)),
        Control::Program { .. } => Err(unauthorized(format!("account {number} is suspended"))),
    }
}

pub(crate) fn resolve(
    ctx: &QueryCtx,
    reference: &Reference,
) -> Result<Option<AccountNumber>, Error> {
    match reference {
        Reference::Account(number) => Ok(ACCOUNTS.has(ctx, number).then_some(*number)),
        Reference::Key(key) => OF_KEY.get(ctx, key),
    }
}

pub(crate) fn profiles(
    ctx: &QueryCtx,
    after: Option<AccountNumber>,
    limit: u32,
) -> Result<Reply, Error> {
    let limit = u64::from(limit.max(1));
    let mut range = ACCOUNTS.prefix_of(&());
    if let Some(after) = after {
        range = range.after(ACCOUNTS.key(&after));
    }
    let mut accounts = ACCOUNTS.scan(ctx, range.limit(limit + 1))?;
    let more = accounts.len() as u64 > limit;
    accounts.truncate(limit as usize);
    let next = accounts.last().filter(|_| more).map(|(number, _)| *number);
    let profiles = accounts
        .iter()
        .map(|(_, account)| account.profile())
        .collect();
    Ok(Reply::Profiles { profiles, next })
}

pub(crate) fn generation(ctx: &QueryCtx, key: &Vec<u8>) -> Result<u64, Error> {
    Ok(GENERATION.get(ctx, key)?.unwrap_or(0))
}

fn next_number(ctx: &ExecCtx) -> Result<AccountNumber, Error> {
    let number = NEXT.get(ctx)?.unwrap_or(1);
    NEXT.put(ctx, &(number + 1));
    Ok(number)
}

fn admit_key(ctx: &ExecCtx, key: &Vec<u8>, number: AccountNumber) -> Result<(), Error> {
    if OF_KEY.has(ctx, key) {
        return Err(already_exists("this key already belongs to an account"));
    }
    OF_KEY.put(ctx, key, &number);
    GENERATION.put(ctx, key, &(generation(ctx, key)? + 1));
    Ok(())
}

/// Forgets every key in `keys`: the account no longer holds them. Their
/// generations stay, so no old consent admits one again.
fn drop_keys(ctx: &ExecCtx, keys: &[Key]) {
    for key in keys {
        OF_KEY.remove(ctx, &key.key);
    }
}

fn named(name: String) -> Result<String, Error> {
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err(invalid("a name is not empty"));
    }
    Ok(name)
}

/// A fresh card: a name, nothing else yet.
fn card(ctx: &ExecCtx, name: String) -> Result<Card, Error> {
    Ok(Card {
        name: named(name)?,
        avatar: None,
        bio: None,
        updated_at: ctx.env().time,
    })
}

fn save(ctx: &ExecCtx, mut account: Account) {
    account.card.updated_at = ctx.env().time;
    ACCOUNTS.put(ctx, &account.number, &account);
}

/// The kernel, admitting `program`, gives it its account, named after it.
/// A program that has one keeps it.
pub(crate) fn register_program(ctx: &ExecCtx, program: ProgramId) -> Result<(), Error> {
    if ctx.env().origin != Origin::Root {
        return Err(unauthorized("only the system registers a program"));
    }
    if OF_PROGRAM.has(ctx, &program) {
        return Ok(());
    }
    let card = card(ctx, program.clone())?;
    let number = next_number(ctx)?;
    let account = Account {
        number,
        card,
        control: Control::System {
            program: program.clone(),
        },
    };
    ACCOUNTS.put(ctx, &number, &account);
    OF_PROGRAM.put(ctx, &program, &number);
    Ok(())
}

pub(crate) fn create(ctx: &ExecCtx, name: String, scheme: Scheme) -> Result<(), Error> {
    let env = ctx.env();
    let signer = env.signer()?;
    let card = card(ctx, name)?;
    if OF_KEY.has(ctx, &signer) {
        return Err(already_exists("this key already belongs to an account"));
    }
    let number = next_number(ctx)?;
    admit_key(ctx, &signer, number)?;
    let key = Key {
        scheme,
        key: signer,
        label: None,
        added_at: env.time,
    };
    let account = Account {
        number,
        card,
        control: Control::User { keys: vec![key] },
    };
    ACCOUNTS.put(ctx, &number, &account);
    ctx.set_return_data(abi::encode(&number));
    Ok(())
}

/// The account number the frame acts as.
fn acting(ctx: &ExecCtx) -> Result<AccountNumber, Error> {
    ctx.sender()?
        .account()
        .ok_or_else(|| unauthorized("the system acts as no account"))
}

/// `number`'s account, which must be a user's: the one kind that manages.
fn user(ctx: &ExecCtx, number: AccountNumber) -> Result<Account, Error> {
    let account = account(ctx, number)?;
    match account.control {
        Control::User { .. } => Ok(account),
        Control::Program { .. } | Control::System { .. } => Err(unauthorized(format!(
            "account {number} is not a user's: a program or system account manages no one"
        ))),
    }
}

pub(crate) fn create_agent(ctx: &ExecCtx, name: String) -> Result<(), Error> {
    let manager = user(ctx, acting(ctx)?)?.number;
    let card = card(ctx, name)?;
    let number = next_number(ctx)?;
    let account = Account {
        number,
        card,
        control: Control::Program {
            manager,
            category: Category::Agent,
            life: Life::Active { keys: Vec::new() },
            script: None,
        },
    };
    ACCOUNTS.put(ctx, &number, &account);
    MANAGED.insert(ctx, &(manager, number));
    ctx.set_return_data(abi::encode(&number));
    Ok(())
}

/// Refused unless the frame acts as `manager`, the manager of `number`.
fn managing(ctx: &ExecCtx, manager: AccountNumber, number: AccountNumber) -> Result<(), Error> {
    let acting = acting(ctx)?;
    if acting != manager {
        return Err(unauthorized(format!(
            "account {acting} does not manage account {number}"
        )));
    }
    Ok(())
}

/// The refusal every op on a revoked program account meets.
fn revoked(number: AccountNumber) -> Error {
    wrong_state(format!("account {number} is revoked for good"))
}

fn not_managed(number: AccountNumber) -> Error {
    wrong_state(format!("account {number} is not a program account"))
}

/// A user's new key signs the frame and a key already on their account
/// consents; a program account's manager signs the frame and the new key
/// consents to itself. Either way the consent proves a key agreed to the
/// admission. A system account holds no keys; a revoked program account
/// takes none.
pub(crate) fn add_key(
    ctx: &ExecCtx,
    scheme: Scheme,
    label: Option<String>,
    consent: Consent,
) -> Result<(), Error> {
    let env = ctx.env();
    let mut account = account(ctx, consent.account)?;
    let (keys, key, consenting_scheme) = match &mut account.control {
        Control::User { keys } => {
            let authorizer = keys
                .iter()
                .find(|key| key.key == consent.key)
                .ok_or_else(|| unauthorized("the consenting key is not on this account"))?;
            let consenting_scheme = authorizer.scheme;
            (keys, env.signer()?, consenting_scheme)
        }
        Control::Program {
            manager,
            life: Life::Active { keys } | Life::Suspended { keys },
            ..
        } => {
            managing(ctx, *manager, consent.account)?;
            (keys, consent.key.clone(), scheme)
        }
        Control::Program {
            manager,
            life: Life::Revoked,
            ..
        } => {
            managing(ctx, *manager, consent.account)?;
            return Err(revoked(consent.account));
        }
        Control::System { .. } => {
            return Err(wrong_state("a system account holds no keys"));
        }
    };
    if env.time > consent.expires_at {
        return Err(unauthorized("the consent has expired"));
    }
    let admission = Admission {
        network: env.chain_id.clone(),
        scheme,
        key: key.clone(),
        generation: generation(ctx, &key)?,
        account: consent.account,
        expires_at: consent.expires_at,
    };
    let consented = ctx.verify(
        consenting_scheme,
        consent.key,
        CONSENT_NAMESPACE,
        admission.preimage(),
        consent.proof,
    )?;
    if !consented {
        return Err(unauthorized("the consent does not verify"));
    }
    admit_key(ctx, &key, consent.account)?;
    keys.push(Key {
        scheme,
        key,
        label,
        added_at: env.time,
    });
    keys.sort_by(|a, b| a.key.cmp(&b.key));
    save(ctx, account);
    Ok(())
}

/// A user removes their own keys, never a senior one nor their last; a
/// program account's manager removes any of its keys.
pub(crate) fn remove_key(ctx: &ExecCtx, number: AccountNumber, key: &Vec<u8>) -> Result<(), Error> {
    let env = ctx.env();
    let mut account = account(ctx, number)?;
    let keys = match &mut account.control {
        Control::User { keys } => {
            let signer = env.signer()?;
            let held = |wanted: &[u8]| keys.iter().find(|held| held.key == wanted);
            let removed = held(key).ok_or_else(|| not_found("that key is not on this account"))?;
            let remover =
                held(&signer).ok_or_else(|| unauthorized("the signer is not on this account"))?;
            if keys.len() == 1 {
                return Err(wrong_state("an account keeps its last key"));
            }
            if removed.added_at < remover.added_at {
                return Err(unauthorized("a key removes only itself or a junior key"));
            }
            keys
        }
        Control::Program {
            manager,
            life: Life::Active { keys } | Life::Suspended { keys },
            ..
        } => {
            managing(ctx, *manager, number)?;
            if !keys.iter().any(|held| &held.key == key) {
                return Err(not_found("that key is not on this account"));
            }
            keys
        }
        Control::Program {
            manager,
            life: Life::Revoked,
            ..
        } => {
            managing(ctx, *manager, number)?;
            return Err(revoked(number));
        }
        Control::System { .. } => {
            return Err(wrong_state("a system account holds no keys"));
        }
    };
    keys.retain(|held| &held.key != key);
    OF_KEY.remove(ctx, key);
    save(ctx, account);
    Ok(())
}

fn editable(ctx: &ExecCtx, number: AccountNumber) -> Result<Account, Error> {
    let account = account(ctx, number)?;
    let revoked_for_good = matches!(
        account.control,
        Control::Program {
            life: Life::Revoked,
            ..
        }
    );
    if revoked_for_good {
        return Err(revoked(number));
    }
    Ok(account)
}

pub fn account_key(number: AccountNumber) -> String {
    number.to_string()
}

pub(crate) fn needs(op: &Op) -> Vec<Need> {
    match op {
        Op::SetName { account, .. }
        | Op::SetProfile { account, .. }
        | Op::Suspend { account }
        | Op::Resume { account }
        | Op::Revoke { account }
        | Op::SetScript { account, .. } => vec![Need::write(account_key(*account))],
        Op::RegisterProgram { .. }
        | Op::Create { .. }
        | Op::CreateAgent { .. }
        | Op::AddKey { .. }
        | Op::RemoveKey { .. }
        | Op::SetOwnership { .. } => Vec::new(),
    }
}

pub(crate) fn resource(ctx: &QueryCtx, key: &str) -> Result<Resource, Error> {
    let Ok(number) = key.parse::<AccountNumber>() else {
        return Ok(Resource::default());
    };
    let Some(account) = ACCOUNTS.get(ctx, &number)? else {
        return Ok(Resource::default());
    };
    let owner = match account.control {
        Control::User { .. } | Control::System { .. } => number,
        Control::Program { manager, .. } => manager,
    };
    Ok(Resource {
        owner: Some(Party::Account(owner)),
        default: Some(Ownership::Owner),
    })
}

pub(crate) fn set_name(ctx: &ExecCtx, number: AccountNumber, name: String) -> Result<(), Error> {
    let mut account = editable(ctx, number)?;
    account.card.name = named(name)?;
    save(ctx, account);
    Ok(())
}

pub(crate) fn set_profile(
    ctx: &ExecCtx,
    number: AccountNumber,
    avatar: Option<abi::BlobId>,
    bio: Option<String>,
) -> Result<(), Error> {
    let mut account = editable(ctx, number)?;
    account.card.avatar = avatar;
    account.card.bio = bio
        .map(|bio| bio.trim().to_owned())
        .filter(|bio| !bio.is_empty());
    save(ctx, account);
    Ok(())
}

fn relive(
    ctx: &ExecCtx,
    number: AccountNumber,
    change: impl FnOnce(Life) -> Result<Life, Error>,
) -> Result<(), Error> {
    let mut account = account(ctx, number)?;
    let Control::Program { life, .. } = &mut account.control else {
        return Err(not_managed(number));
    };
    let was = std::mem::replace(life, Life::Revoked);
    *life = change(was)?;
    save(ctx, account);
    Ok(())
}

pub(crate) fn suspend(ctx: &ExecCtx, number: AccountNumber) -> Result<(), Error> {
    relive(ctx, number, |life| match life {
        Life::Active { keys } => Ok(Life::Suspended { keys }),
        Life::Suspended { .. } => Err(wrong_state(format!(
            "account {number} is already suspended"
        ))),
        Life::Revoked => Err(revoked(number)),
    })
}

pub(crate) fn resume(ctx: &ExecCtx, number: AccountNumber) -> Result<(), Error> {
    relive(ctx, number, |life| match life {
        Life::Suspended { keys } => Ok(Life::Active { keys }),
        Life::Active { .. } => Err(wrong_state(format!("account {number} is active"))),
        Life::Revoked => Err(revoked(number)),
    })
}

/// Final: the program account's keys are dropped and act as no one; the
/// manager's list keeps it.
pub(crate) fn revoke(ctx: &ExecCtx, number: AccountNumber) -> Result<(), Error> {
    relive(ctx, number, |life| match life {
        Life::Active { keys } | Life::Suspended { keys } => {
            drop_keys(ctx, &keys);
            Ok(Life::Revoked)
        }
        Life::Revoked => Err(revoked(number)),
    })
}

pub(crate) fn set_script(
    ctx: &ExecCtx,
    number: AccountNumber,
    script: Option<Script>,
) -> Result<(), Error> {
    let mut account = account(ctx, number)?;
    let Control::Program {
        life,
        script: current,
        ..
    } = &mut account.control
    else {
        return Err(not_managed(number));
    };
    if *life == Life::Revoked {
        return Err(revoked(number));
    }
    if let Some(Script { calls, .. }) = &script
        && !OF_PROGRAM.has(ctx, calls)
    {
        return Err(not_found(format!("program {calls} has no account")));
    }
    *current = script;
    save(ctx, account);
    Ok(())
}

pub(crate) fn runs(
    ctx: &QueryCtx,
    program: &ProgramId,
    number: AccountNumber,
) -> Result<Option<AccountNumber>, Error> {
    let not_run = || unauthorized(format!("{program} does not run account {number}"));
    let Control::Program { life, script, .. } = account(ctx, number)?.control else {
        return Err(not_run());
    };
    let calls_program = script.is_some_and(|script| &script.calls == program);
    if !calls_program {
        return Err(not_run());
    }
    match life {
        Life::Active { .. } => Ok(Some(number)),
        Life::Suspended { .. } => Err(unauthorized(format!("account {number} is suspended"))),
        Life::Revoked => Err(revoked(number)),
    }
}
