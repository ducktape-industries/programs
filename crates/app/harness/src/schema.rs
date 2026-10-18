//! Where a program's interface schema comes from: the `ducktape.schema` custom section of the code
//! the programs role runs it from, read out of the code's blob a section header at a time.
#[cfg(test)]
pub(crate) mod tests;

use ::program::schema::{SECTION, returned, roots};
use ::program::{BlobId, Error, QueryCtx, code, is_name, section};
use abi::role::programs as role;
use borsh::schema::{BorshSchemaContainer, Declaration};

pub(crate) struct Interface {
    pub schema: BorshSchemaContainer,
    pub op: Declaration,
    pub query: Declaration,
    pub reply: Declaration,
    pub returns: Option<Vec<(String, Declaration)>>,
}

pub(crate) fn schema_of(ctx: &QueryCtx, program: &str) -> Result<Interface, Error> {
    let code = code_of(ctx, program)?;
    let Some(section) = section::read(ctx, code, SECTION)? else {
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
