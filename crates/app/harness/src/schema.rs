//! Where a program's interface schema comes from: the `ducktape.schema` custom section of the code
//! the programs role runs it from, read out of the code's blob a section header at a time.
#[cfg(test)]
pub(crate) mod tests;

use ::program::schema::{SECTION, returned, roots};
use ::program::{BlobId, Error, QueryCtx, code, is_name, not_found};
use abi::role::programs as role;
use borsh::schema::{BorshSchemaContainer, Declaration};

const MODULE: &[u8] = b"\0asm\x01\0\0\0";

pub(crate) struct Interface {
    pub schema: BorshSchemaContainer,
    pub op: Declaration,
    pub query: Declaration,
    pub reply: Declaration,
    pub returns: Option<Vec<(String, Declaration)>>,
}

pub(crate) fn schema_of(ctx: &QueryCtx, program: &str) -> Result<Interface, Error> {
    let code = code_of(ctx, program)?;
    let Some(section) = section(ctx, code, SECTION)? else {
        return Err(Error::new(
            code::UNSUPPORTED,
            format!("program {program}'s code carries no {SECTION} section, so it has no schema"),
        ));
    };
    let unreadable = |what: &str| {
        Error::new(
            code::CORRUPT,
            format!("program {program}'s {SECTION} section {what}"),
        )
    };
    let schema: BorshSchemaContainer =
        abi::decode(&section).map_err(|_| unreadable("is no borsh schema"))?;
    schema
        .validate()
        .map_err(|_| unreadable("is no valid schema"))?;
    let Some(roots) = roots(&schema) else {
        return Err(unreadable("names no op, query and reply"));
    };
    let returns = match roots.returns {
        Some(returns) => {
            let returned = returned(&schema, returns)
                .ok_or_else(|| unreadable("names what its ops return in no struct of them"))?;
            let owned = returned
                .into_iter()
                .map(|(op, of)| (op.to_owned(), of.to_owned()));
            Some(owned.collect())
        }
        None => None,
    };
    let (op, query, reply) = (
        roots.op.to_owned(),
        roots.query.to_owned(),
        roots.reply.to_owned(),
    );
    Ok(Interface {
        schema,
        op,
        query,
        reply,
        returns,
    })
}

fn code_of(ctx: &QueryCtx, program: &str) -> Result<BlobId, Error> {
    let unknown = || {
        Error::new(
            code::UNKNOWN_PROGRAM,
            format!("no program named {program} runs on this network"),
        )
    };
    if !is_name(program) {
        return Err(unknown());
    }
    let programs = ctx.env().role(abi::role::PROGRAMS)?.clone();
    let asked = role::Query::At(ctx.env().height);
    let entries = match ctx.query(programs.clone(), &asked)? {
        role::Reply::Programs(entries) => entries,
        other => return Err(::program::unexpected_reply(&programs, "At", &other)),
    };
    entries
        .into_iter()
        .find(|entry| entry.program == program)
        .map(|entry| entry.code)
        .ok_or_else(unknown)
}

fn section(ctx: &QueryCtx, code: BlobId, name: &str) -> Result<Option<Vec<u8>>, Error> {
    let read = |offset: u64, len: u64| {
        ctx.blob_read(code, offset, len)
            .ok_or_else(|| not_found(format!("the code blob {code:?} is not held here")))
    };
    if read(0, MODULE.len() as u64)? != MODULE {
        return Ok(None);
    }
    let mut offset = MODULE.len() as u64;
    loop {
        let head = read(offset, 1 + LEB_U32)?;
        let Some((&id, rest)) = head.split_first() else {
            return Ok(None);
        };
        let Some((size, used)) = leb(rest) else {
            return Ok(None);
        };
        let body = offset + 1 + used;
        let end = body + size;
        if id == CUSTOM {
            let named = read(body, LEB_U32)?;
            let Some((length, used)) = leb(&named) else {
                return Ok(None);
            };
            let name_at = body + used;
            let payload_at = name_at + length;
            let same_length = length == name.len() as u64;
            let ours = same_length && read(name_at, length)? == name.as_bytes();
            if ours {
                let Some(payload) = end.checked_sub(payload_at) else {
                    return Ok(None);
                };
                return read(payload_at, payload).map(Some);
            }
        }
        offset = end;
    }
}

const CUSTOM: u8 = 0;
const LEB_U32: u64 = 5;

fn leb(bytes: &[u8]) -> Option<(u64, u64)> {
    let mut value = 0u64;
    for (index, byte) in bytes.iter().take(LEB_U32 as usize).enumerate() {
        value |= u64::from(byte & 0x7f) << (7 * index);
        if byte & 0x80 == 0 {
            return Some((value, index as u64 + 1));
        }
    }
    None
}
