//! The codec over Rust types' own schemas: the JSON form of each borsh shape, read to JSON and
//! written back to the same bytes.
mod forms;
mod interface;

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
