//! Tools over the network's people and what they did: accounts as the identity role
//! profiles them, and an account's recent frames as `activity` indexes them.
use ::program::{Error, ExecCtx, invalid};
use abi::role::identity::{self as role, Category, Kind, Profile, Standing};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Run, Seat, Tool, cursor_schema, next, object, page, unexpected};

fn kind(kind: &Kind) -> Value {
    match kind {
        Kind::User => json!("user"),
        Kind::Program {
            manager,
            category: Category::Agent,
            standing,
            script,
        } => {
            let standing = match standing {
                Standing::Active => "active",
                Standing::Suspended => "suspended",
                Standing::Revoked => "revoked",
            };
            let run = match script {
                Some(script) => format!(", its script calls program {}", script.calls),
                None => String::new(),
            };
            json!(format!(
                "program account, an agent managed by account {manager}, {standing}{run}"
            ))
        }
        Kind::System(program) => json!(format!("system account of program {program}")),
    }
}

fn profile(profile: &Profile) -> Value {
    json!({ "number": profile.number, "name": profile.name, "kind": kind(&profile.kind) })
}

fn ask(ctx: &ExecCtx, query: role::Query) -> Result<role::Reply, Error> {
    ctx.query(ctx.env().role(abi::role::IDENTITY)?.clone(), &query)
}

#[derive(Deserialize)]
pub(crate) struct AccountsArgs {
    cursor: Option<String>,
}

pub(crate) struct Accounts;

impl Tool for Accounts {
    const NAME: &'static str = "network_accounts";
    const DESCRIPTION: &'static str = "List the network's accounts in number order: each one's name and whether it is a user, a program account or a system account.";
    type Args = AccountsArgs;

    fn schema() -> Value {
        object(json!({ "cursor": cursor_schema() }), &[])
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: AccountsArgs) -> Result<Run, Error> {
        let after = args
            .cursor
            .map(|cursor| cursor.parse::<u64>())
            .transpose()
            .map_err(|_| invalid("an accounts cursor is the account number to list after: use the next account number returned by the listing"))?;
        let asked = role::Query::Profiles {
            after,
            limit: u32::MAX,
        };
        match ask(ctx, asked)? {
            role::Reply::Profiles { profiles, next } => Ok(Run::Done(json!({
                "accounts": profiles.iter().map(profile).collect::<Vec<_>>(),
                "next": next.map(|number| number.to_string()),
            }))),
            other => Err(unexpected("identity", "Profiles", &other)),
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct AccountArgs {
    number: u64,
}

pub(crate) struct Account;

impl Tool for Account {
    const NAME: &'static str = "network_account";
    const DESCRIPTION: &'static str = "One account of the network by number: its name and kind.";
    type Args = AccountArgs;

    fn schema() -> Value {
        object(json!({ "number": { "type": "integer" } }), &["number"])
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: AccountArgs) -> Result<Run, Error> {
        match ask(ctx, role::Query::Profile(args.number))? {
            role::Reply::Profile(found) => {
                Ok(Run::Done(json!({ "account": found.as_ref().map(profile) })))
            }
            other => Err(unexpected("identity", "Profile", &other)),
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct ActivityArgs {
    account: u64,
    cursor: Option<String>,
}

pub(crate) struct Activity;

impl Tool for Activity {
    const NAME: &'static str = "network_activity";
    const DESCRIPTION: &'static str = "An account's recent frames on the network, newest first: the block, the program it targeted, and whether it applied.";
    type Args = ActivityArgs;

    fn schema() -> Value {
        object(
            json!({ "account": { "type": "integer" }, "cursor": cursor_schema() }),
            &["account"],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: ActivityArgs) -> Result<Run, Error> {
        let asked = activity::Query::Acted {
            account: args.account,
            page: page(&args.cursor)?,
        };
        match ctx.query(activity::PROGRAM, &asked)? {
            activity::Reply::Frames(frames) => {
                let acted: Vec<Value> = frames
                    .items
                    .iter()
                    .map(|frame| {
                        json!({
                            "height": frame.height,
                            "position": frame.position,
                            "time": frame.time,
                            "program": frame.target,
                            "applied": frame.applied,
                        })
                    })
                    .collect();
                Ok(Run::Done(
                    json!({ "frames": acted, "next": next(&frames.next) }),
                ))
            }
            other => Err(unexpected(activity::PROGRAM, "Acted", &other)),
        }
    }
}
