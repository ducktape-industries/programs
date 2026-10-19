//! Every program's ops, queries and replies read to JSON by the codec and written back to the
//! same bytes.
mod examples;
mod programs;

use borsh::BorshSerialize;
use borsh::schema::BorshSchemaContainer;
use serde_json::Value;

use super::*;

struct Roots {
    schema: BorshSchemaContainer,
    op: String,
    query: String,
    reply: String,
}

fn roots(schema: BorshSchemaContainer) -> Roots {
    let named = ::program::schema::roots(&schema).unwrap();
    let (op, query, reply) = (
        named.op.to_owned(),
        named.query.to_owned(),
        named.reply.to_owned(),
    );
    Roots {
        schema,
        op,
        query,
        reply,
    }
}

impl Roots {
    fn round_trip(&self, declaration: &str, bytes: Vec<u8>) -> Value {
        let json = decode(&self.schema, declaration, "value", &bytes).unwrap();
        let text = json.to_string();
        let parsed: Value = serde_json::from_str(&text).unwrap();
        let again = encode(&self.schema, declaration, "value", &parsed).unwrap();
        assert_eq!(again, bytes, "{text}");
        json
    }

    fn op(&self, op: &impl BorshSerialize) -> Value {
        self.round_trip(&self.op, borsh::to_vec(op).unwrap())
    }

    fn query(&self, query: &impl BorshSerialize) -> Value {
        self.round_trip(&self.query, borsh::to_vec(query).unwrap())
    }

    fn reply(&self, reply: &impl BorshSerialize) -> Value {
        self.round_trip(&self.reply, borsh::to_vec(reply).unwrap())
    }

    fn shapes(&self) -> Value {
        let roots = [
            ("op", self.op.as_str()),
            ("query", self.query.as_str()),
            ("reply", self.reply.as_str()),
        ];
        shape(&self.schema, &roots, None).unwrap()
    }
}
