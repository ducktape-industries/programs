//! Who reads a direct room: a query's signer as its account, an asking program as its own account
//! or, from a program chat's params list, the reader its `As` names; the kernel reads all.
use std::cell::OnceCell;

use ::program::{
    Error, Origin, Principal, ProgramId, QueryCtx, code, invalid, unauthorized, unexpected_reply,
};
use abi::role::identity as role;

use crate::state::{CHANNELS, MEMBERS};
use crate::{ChannelRow, dm_peers};

pub(crate) struct Reader {
    asking: Asking,
    account: OnceCell<Option<Principal>>,
}

enum Asking {
    Kernel,
    Key(Vec<u8>),
    Program(ProgramId),
    Proxied {
        program: ProgramId,
        reader: Principal,
    },
}

impl Reader {
    pub(crate) fn of(ctx: &QueryCtx) -> Reader {
        let asking = match &ctx.env().origin {
            Origin::Root => Asking::Kernel,
            Origin::Signed(key) => Asking::Key(key.clone()),
            Origin::Program(program) => Asking::Program(program.clone()),
        };
        Reader {
            asking,
            account: OnceCell::new(),
        }
    }

    pub(crate) fn naming(ctx: &QueryCtx, reader: Principal) -> Reader {
        let asking = match &ctx.env().origin {
            Origin::Program(program) => Asking::Proxied {
                program: program.clone(),
                reader,
            },
            Origin::Root | Origin::Signed(_) => return Reader::of(ctx),
        };
        Reader {
            asking,
            account: OnceCell::new(),
        }
    }

    pub(crate) fn room(&self, ctx: &QueryCtx, channel_id: &str) -> Result<(), Error> {
        let Some(room) = CHANNELS.get(ctx, &channel_id.to_owned())? else {
            return Ok(());
        };
        match self.reads(ctx, &room)? {
            true => Ok(()),
            false => Err(unauthorized(format!(
                "{channel_id} is a direct room: only its two peers read it"
            ))),
        }
    }

    pub(crate) fn may_read(&self, ctx: &QueryCtx, channel_id: &str) -> Result<bool, Error> {
        match CHANNELS.get(ctx, &channel_id.to_owned())? {
            Some(room) => self.reads(ctx, &room),
            None => Ok(false),
        }
    }

    fn reads(&self, ctx: &QueryCtx, room: &ChannelRow) -> Result<bool, Error> {
        let is_kernel = matches!(self.asking, Asking::Kernel);
        if is_kernel {
            return Ok(true);
        }
        let is_direct = dm_peers(&room.id).is_some();
        if !is_direct {
            return Ok(true);
        }
        let Some(account) = self.account(ctx)? else {
            return Ok(false);
        };
        let seated = MEMBERS.has(ctx, &(room.id.clone(), account.clone()));
        Ok(room.admits(&account, seated))
    }

    fn account(&self, ctx: &QueryCtx) -> Result<Option<Principal>, Error> {
        if let Some(account) = self.account.get() {
            return Ok(account.clone());
        }
        let account = match &self.asking {
            Asking::Kernel => None,
            Asking::Key(key) => acting(ctx, role::Query::Account(key.clone()))?,
            Asking::Program(program) => acting(ctx, role::Query::OfProgram(program.clone()))?,
            Asking::Proxied { program, reader } => match proxies(ctx)?.contains(program) {
                true => Some(reader.clone()),
                false => acting(ctx, role::Query::OfProgram(program.clone()))?,
            },
        };
        Ok(self.account.get_or_init(|| account).clone())
    }
}

fn acting(ctx: &QueryCtx, asked: role::Query) -> Result<Option<Principal>, Error> {
    let identity = ctx.env().role(abi::role::IDENTITY)?.clone();
    match ctx.query::<role::Query, role::Reply>(&identity, &asked) {
        Ok(role::Reply::Account(number)) => Ok(number.map(Principal::Account)),
        Ok(other) => Err(unexpected_reply(&identity, "Account", &other)),
        Err(refused) if refused.code == code::UNAUTHORIZED => Ok(None),
        Err(fault) => Err(fault),
    }
}

fn proxies(ctx: &QueryCtx) -> Result<Vec<ProgramId>, Error> {
    let role = ctx.env().role(abi::role::PROGRAMS)?.clone();
    let asked = programs::Query::Program(ctx.env().program.clone());
    match ctx.query::<programs::Query, programs::Reply>(&role, &asked)? {
        programs::Reply::Program { entry, .. } => match entry {
            Some(entry) => proxied(&entry.params),
            None => Ok(Vec::new()),
        },
        other => Err(unexpected_reply(&role, "Program", &other)),
    }
}

pub(crate) fn proxied(params: &[u8]) -> Result<Vec<ProgramId>, Error> {
    if params.is_empty() {
        return Ok(Vec::new());
    }
    abi::decode(params).map_err(|_| {
        invalid("chat's params do not read as its proxies: a borsh Vec of program ids")
    })
}
