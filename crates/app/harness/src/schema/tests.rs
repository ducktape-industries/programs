use ::program::MockChain;
use ::program::schema::interface;
use ::program::section::{CUSTOM, MODULE};
use ::program::tools::Tools;
use borsh::schema::BorshSchemaContainer;
use programs::{Entry, Genesis, Programs};

use super::*;

fn leb(mut value: usize) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        match value {
            0 => {
                out.push(byte);
                return out;
            }
            _ => out.push(byte | 0x80),
        }
    }
}

pub(crate) fn module(customs: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = MODULE.to_vec();
    out.extend([1, 1, 0]);
    for (name, payload) in customs {
        let mut body = leb(name.len());
        body.extend(name.as_bytes());
        body.extend(*payload);
        out.push(CUSTOM);
        out.extend(leb(body.len()));
        out.extend(body);
    }
    out
}

pub(crate) fn code_with(schema: &BorshSchemaContainer) -> Vec<u8> {
    let view = vec![7u8; 300];
    let encoded = borsh::to_vec(schema).unwrap();
    module(&[
        ("ducktape.view", &view),
        (SECTION, &encoded),
        ("ducktape.describe", b"after"),
    ])
}

// Code as founding packs it: the schema, and the tools section `tools` tells.
pub(crate) fn described_code(schema: &BorshSchemaContainer, tools: &Tools) -> Vec<u8> {
    let encoded = borsh::to_vec(schema).unwrap();
    let interface = Interface::read(&encoded).unwrap();
    let told = borsh_json::tools::section(&interface, tools).unwrap();
    module(&[(SECTION, &encoded), (::program::tools::SECTION, &told)])
}

pub(crate) fn listed(chain: &MockChain, codes: &[(&str, Vec<u8>)]) {
    let programs = codes
        .iter()
        .map(|(program, code)| Entry {
            program: (*program).into(),
            code: chain.put_blob(programs::CODE_KIND, code.clone()).unwrap(),
            params: match *program {
                files::PROGRAM => crate::tests::files_params(),
                _ => Vec::new(),
            },
        })
        .collect();
    let genesis = Genesis {
        programs,
        views: Vec::new(),
        hooks: Vec::new(),
        roles: ::program::Roles::default(),
    };
    chain.init(programs::PROGRAM, &genesis).unwrap();
}

fn chain() -> MockChain {
    let mut chain = MockChain::default();
    chain.seat::<Programs>(programs::PROGRAM);
    chain.seat::<crate::Harness>(crate::PROGRAM);
    let codes = [
        ("chat", code_with(&interface::<chat::Chat>())),
        ("plain", module(&[("ducktape.view", b"view")])),
        ("text", b"not a module".to_vec()),
        ("broken", module(&[(SECTION, b"\x05")])),
    ];
    listed(&chain, &codes);
    chain
}

fn asked(chain: &MockChain, program: &str) -> Result<Interface, Error> {
    let ctx = chain.host(crate::PROGRAM).query(chain.env(crate::PROGRAM));
    schema_of(&ctx, program)
}

#[test]
fn the_schema_section_is_found_among_the_codes_sections_and_names_op_query_and_reply() {
    let chain = chain();
    let found = asked(&chain, "chat").unwrap();
    let expected = interface::<chat::Chat>();
    assert_eq!(found.schema, expected);
    assert_eq!(
        (
            found.op.as_str(),
            found.query.as_str(),
            found.reply.as_str()
        ),
        ("Op", "Query", "Reply")
    );
}

#[test]
fn a_program_without_a_readable_schema_says_why() {
    let chain = chain();
    let code_of = |program: &str| asked(&chain, program).err().map(|error| error.code);
    assert_eq!(code_of("plain"), Some(code::UNSUPPORTED.to_owned()));
    assert_eq!(code_of("text"), Some(code::UNSUPPORTED.to_owned()));
    assert_eq!(code_of("broken"), Some(code::CORRUPT.to_owned()));
    assert_eq!(code_of("absent"), Some(code::UNKNOWN_PROGRAM.to_owned()));
    assert_eq!(
        code_of("Not A Name"),
        Some(code::UNKNOWN_PROGRAM.to_owned())
    );
}

#[test]
fn an_interface_read_in_a_frame_is_kept_so_the_next_read_takes_nothing_from_the_code() {
    let chain = chain();
    let host = chain.host(crate::PROGRAM);
    let ctx = host.exec(chain.env(crate::PROGRAM));
    schema_of(&ctx, "chat").unwrap();
    let read = host.borrow().work.blob_reads;
    let again = schema_of(&ctx, "chat").unwrap();
    assert_eq!(
        host.borrow().work.blob_reads,
        read,
        "the second read went to the code blob"
    );
    assert_eq!(again.schema, interface::<chat::Chat>());
}

#[test]
fn a_kept_interface_is_read_again_from_new_code() {
    let chain = chain();
    let host = chain.host(crate::PROGRAM);
    let ctx = host.exec(chain.env(crate::PROGRAM));
    schema_of(&ctx, "chat").unwrap();
    let newer = code_with(&interface::<crate::Harness>());
    let newer = chain.put_blob(programs::CODE_KIND, newer).unwrap();
    let read = schema_in(&ctx, "chat", newer).unwrap();
    assert_eq!(read.schema, interface::<crate::Harness>());
}

#[test]
fn a_query_reads_the_code_and_keeps_nothing() {
    let chain = chain();
    let host = chain.host(crate::PROGRAM);
    let ctx = host.query(chain.env(crate::PROGRAM));
    schema_of(&ctx, "chat").unwrap();
    let read = host.borrow().work.blob_reads;
    schema_of(&ctx, "chat").unwrap();
    assert!(
        host.borrow().work.blob_reads > read,
        "a query kept what it read"
    );
}
