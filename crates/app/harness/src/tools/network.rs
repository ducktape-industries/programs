//! Tools over the network's people and what they did: accounts as the identity role
//! profiles them, and an account's recent frames as the node's activity index keeps them.
use ::program::{Error, ExecCtx, HashKind, code, invalid};
use abi::role::egress::{self, http};
use abi::role::identity::{self as role, Category, Kind, Profile, Standing};
use serde::Deserialize;
use serde_json::{Value, json};

use super::service::whole;
use super::{Run, Seat, Tool, cursor_schema, next, object, page, unexpected};
use crate::PART;

const NODE: &str = "node";
const ACTIVITY: &str = "/activity";
const REFUSAL: u8 = 5;
const REFUSED: u8 = 3;

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
    height: Option<u64>,
    position: Option<u32>,
}

fn node_answer(output: &[u8]) -> Result<Vec<u8>, Error> {
    let response: http::Response = abi::decode(output).map_err(|fault| {
        let why = format!("the answer is no HTTP response: {}", fault.sentence);
        Error::new(code::UNEXPECTED_REPLY, why)
    })?;
    match response.status {
        200 => Ok(response.body),
        400 => {
            let refusal: abi::Refusal = abi::decode(&response.body).map_err(|fault| {
                let why = format!("the node's refusal does not decode: {}", fault.sentence);
                Error::new(code::UNEXPECTED_REPLY, why)
            })?;
            Err(Error::new(refusal.reason, refusal.sentence))
        }
        status => Err(Error::new(
            code::WRONG_STATE,
            format!(
                "the node answered {status}: {}",
                String::from_utf8_lossy(&response.body)
            ),
        )),
    }
}

fn frames_of(output: &[u8]) -> Result<Value, Error> {
    let body = node_answer(output)?;
    let reply: activity::Reply = abi::decode(&body).map_err(|fault| {
        let why = format!("the node's activity does not decode: {}", fault.sentence);
        Error::new(code::UNEXPECTED_REPLY, why)
    })?;
    let activity::Reply::Frames(frames) = reply else {
        return Err(unexpected("node", "Acted", &reply));
    };
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
    Ok(json!({ "frames": acted, "next": next(&frames.next) }))
}

fn refusal_of(output: &[u8]) -> Result<Value, Error> {
    let body = node_answer(output)?;
    let unread = || Error::new(code::UNEXPECTED_REPLY, "the node answered no refusal");
    let (tag, rest) = body.split_first().ok_or_else(unread)?;
    if *tag != REFUSED {
        return Err(unread());
    }
    let refusal: Option<Error> = abi::decode(rest).map_err(|_| unread())?;
    let why = refusal.map(|refusal| json!({ "code": refusal.code, "message": refusal.message }));
    Ok(json!({ "refusal": why }))
}

pub(crate) struct Activity;

impl Tool for Activity {
    const NAME: &'static str = "network_activity";
    const DESCRIPTION: &'static str = "An account's recent frames on the network, newest first, as the connected node's activity index keeps them: the block, the program it targeted, and whether it applied. With height and position (a frame it listed), why that frame did not apply instead: its code and message, or null where it applied or the node kept no reason for it. A node that keeps no activity index refuses both; what your own calls did is then in your conversation's frames (harness Frames).";
    const WRITES: bool = true;
    type Args = ActivityArgs;

    fn schema() -> Value {
        object(
            json!({
                "account": { "type": "integer" },
                "cursor": cursor_schema(),
                "height": { "type": "integer", "description": "with position: the block of a frame to say why it did not apply" },
                "position": { "type": "integer", "description": "with height: the frame's place in its block" },
            }),
            &["account"],
        )
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: ActivityArgs) -> Result<Run, Error> {
        let (body, read): (Vec<u8>, super::Reading) = match (args.height, args.position) {
            (None, None) => {
                let asked = activity::Query::Acted {
                    account: args.account,
                    page: page(&args.cursor)?,
                };
                (abi::encode(&asked), Box::new(frames_of))
            }
            (Some(height), Some(position)) => {
                let asked = [vec![REFUSAL], abi::encode(&(height, position))].concat();
                (asked, Box::new(refusal_of))
            }
            _ => {
                return Err(invalid(
                    "a frame is named by its height and its position together",
                ));
            }
        };
        let held = super::held_for(seat.thread, seat.call);
        let part = ctx.blob_put_on(HashKind::Sha256, PART, body, held)?;
        let request = egress::Request::Http {
            service: NODE.into(),
            head: http::Head {
                method: http::Method::Post,
                target: ACTIVITY.into(),
                headers: Vec::new(),
            },
            body: vec![part],
        };
        Ok(Run::Read {
            writes: whole(ctx, request)?,
            read,
        })
    }
}
