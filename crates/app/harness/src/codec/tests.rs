//! The codec over Rust types' own schemas: the JSON form of each borsh shape, and every program's
//! ops, queries and replies read to JSON and written back to the same bytes.
mod forms;
mod programs;

use borsh::schema::{BorshSchemaContainer, Definition};
use borsh::{BorshSchema, BorshSerialize};
use serde_json::Value;

use super::*;

fn schema_of<T: BorshSchema>() -> BorshSchemaContainer {
    BorshSchemaContainer::for_type::<T>()
}

fn json_of<T: BorshSchema + BorshSerialize>(value: &T) -> Value {
    let schema = schema_of::<T>();
    let bytes = borsh::to_vec(value).unwrap();
    let json = decode(&schema, schema.declaration(), "value", &bytes).unwrap();
    let again = encode(&schema, schema.declaration(), "value", &json).unwrap();
    assert_eq!(again, bytes, "{json}");
    json
}

fn bytes_of<T: BorshSchema>(json: Value) -> Result<Vec<u8>, Fault> {
    let schema = schema_of::<T>();
    encode(&schema, schema.declaration(), "value", &json)
}

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
