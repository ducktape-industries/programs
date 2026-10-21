//! A program's interface as its code's `ducktape.schema` section carries it: the schema, the op,
//! query and reply it names and what each op returns, each value written from JSON and read back.
use std::fmt;

use borsh::schema::{BorshSchemaContainer, Declaration};
use program::schema::{returned, roots};
use serde_json::{Value, json};

use crate::{Fault, declared_at, decode, encode, fits, shape, spelled, variant};

#[derive(Clone, Debug, PartialEq)]
pub struct Interface {
    pub schema: BorshSchemaContainer,
    pub op: Declaration,
    pub query: Declaration,
    pub reply: Declaration,
    pub returns: Option<Vec<(String, Declaration)>>,
}

// Why a section is no interface, worded to follow "the section".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unreadable {
    NotBorsh,
    Invalid,
    NoRoots,
    NoReturns,
}

impl fmt::Display for Unreadable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Unreadable::NotBorsh => "is no borsh schema",
            Unreadable::Invalid => "is no valid schema",
            Unreadable::NoRoots => "names no op, query and reply",
            Unreadable::NoReturns => "names what its ops return in no struct of them: declare a returns struct with one field per returning op",
        })
    }
}

impl Interface {
    pub fn read(section: &[u8]) -> Result<Interface, Unreadable> {
        let schema: BorshSchemaContainer =
            abi::decode(section).map_err(|_| Unreadable::NotBorsh)?;
        schema.validate().map_err(|_| Unreadable::Invalid)?;
        let named = roots(&schema).ok_or(Unreadable::NoRoots)?;
        let returns = match named.returns {
            Some(returns) => {
                let listed = returned(&schema, returns).ok_or(Unreadable::NoReturns)?;
                let owned = listed
                    .into_iter()
                    .map(|(op, of)| (op.to_owned(), of.to_owned()));
                Some(owned.collect())
            }
            None => None,
        };
        let (op, query, reply) = (
            named.op.to_owned(),
            named.query.to_owned(),
            named.reply.to_owned(),
        );
        Ok(Interface {
            schema,
            op,
            query,
            reply,
            returns,
        })
    }

    pub fn encode_op(&self, op: &Value) -> Result<Vec<u8>, Fault> {
        encode(&self.schema, &self.op, "op", op)
    }

    pub fn encode_query(&self, query: &Value) -> Result<Vec<u8>, Fault> {
        encode(&self.schema, &self.query, "query", query)
    }

    // What `encode_op` would refuse in `op`, each value `hole` takes standing for any value.
    pub fn op_fits(&self, op: &Value, hole: fn(&Value) -> bool) -> Result<(), Fault> {
        fits(&self.schema, &self.op, "op", op, hole)
    }

    pub fn query_fits(&self, query: &Value, hole: fn(&Value) -> bool) -> Result<(), Fault> {
        fits(&self.schema, &self.query, "query", query, hole)
    }

    pub fn decode_op(&self, op: &[u8]) -> Result<Value, Fault> {
        decode(&self.schema, &self.op, "op", op)
    }

    pub fn decode_reply(&self, reply: &[u8]) -> Result<Value, Fault> {
        decode(&self.schema, &self.reply, "reply", reply)
    }

    pub fn reply_at(&self, path: &str) -> Result<Declaration, Fault> {
        declared_at(&self.schema, &self.reply, "reply", path)
    }

    // The shapes of the op, the query, the reply and (where the schema names them) what each op
    // returns, with every type they reach.
    pub fn shape(&self) -> Result<Value, Fault> {
        let roots = [
            ("op", self.op.as_str()),
            ("query", self.query.as_str()),
            ("reply", self.reply.as_str()),
        ];
        let returns: Option<Vec<(&str, &str)>> = self.returns.as_ref().map(|returns| {
            let named = returns.iter().map(|(op, of)| (op.as_str(), of.as_str()));
            named.collect()
        });
        shape(&self.schema, &roots, returns.as_deref())
    }

    // What the op `payload` of `program` output: nothing, JSON in the type the schema names for
    // the op, else the output's bytes as hex and why they read as nothing more.
    pub fn returned(&self, program: &str, payload: &[u8], output: &[u8]) -> Value {
        if output.is_empty() {
            return json!({ "ok": true });
        }
        let op = variant(&self.schema, &self.op, payload);
        let named = match &op {
            Some(op) => format!("its op {}", spelled(op)),
            None => "this op".to_owned(),
        };
        let declared = match (&self.returns, &op) {
            (Some(returns), Some(op)) => returns
                .iter()
                .find(|(returning, _)| returning == op)
                .map(|(_, of)| of.as_str()),
            _ => None,
        };
        let Some(of) = declared else {
            let why = match &self.returns {
                Some(_) => format!(
                    "the schema of program {program} names no type for what {named} returns, so this is the output's bytes"
                ),
                None => format!(
                    "the schema of program {program} names no type for what its ops return, so this is the output's bytes"
                ),
            };
            return unread(output, &why);
        };
        match decode(&self.schema, of, "returned", output) {
            Ok(value) => json!({ "ok": true, "returned": value }),
            Err(fault) => {
                let why = format!(
                    "the output does not read as {of}, the type the schema of program {program} names for {named}: {fault}"
                );
                unread(output, &why)
            }
        }
    }
}

// An output that reads as nothing more than its bytes, and why.
pub fn unread(output: &[u8], why: &str) -> Value {
    json!({ "ok": true, "returned": abi::hex(output), "unread": why })
}
