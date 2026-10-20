use super::*;

#[derive(Debug, BorshSerialize, BorshDeserialize)]
enum Op {
    Say(String),
    Pay { to: u64, amount: u128 },
}

fn describe(op: &Op) -> Description {
    match op {
        Op::Say(text) => Description {
            title: format!("Say · {text}"),
            fields: vec![field("text", Value::text(text))],
        },
        Op::Pay { to, amount } => Description {
            title: "Pay".into(),
            fields: vec![
                field("to", Value::Account(*to)),
                field(
                    "amount",
                    Value::Amount {
                        value: *amount,
                        decimals: 2,
                    },
                ),
                field(
                    "parts",
                    Value::List(vec![Value::bytes(&[7; 100]), Value::Time(1)]),
                ),
            ],
        },
    }
}

#[test]
fn an_op_round_trips_to_its_description() {
    let op = Op::Pay {
        to: 3,
        amount: 1250,
    };
    let bytes = describe_bytes(&borsh::to_vec(&op).unwrap(), describe).unwrap();
    assert_eq!(Description::decode(&bytes), Some(describe(&op)));
    assert_eq!(describe_bytes(&[9], describe), None);
    let mut long = borsh::to_vec(&Op::Say("hi".into())).unwrap();
    long.push(0);
    assert_eq!(describe_bytes(&long, describe), None);
}

#[test]
fn bytes_keep_their_length_and_a_short_preview() {
    assert_eq!(
        Value::bytes(&[1, 2]),
        Value::Bytes {
            len: 2,
            preview: vec![1, 2]
        }
    );
    let mut long = vec![7; 100];
    long[98] = 8;
    assert_eq!(
        Value::bytes(&long),
        Value::Bytes {
            len: 100,
            preview: [vec![7; 8], vec![8, 7]].concat()
        }
    );
}

#[test]
fn a_description_nests_lists_only_so_deep() {
    let nest = |depth: u32| {
        let mut value = Value::Time(1);
        for _ in 0..depth {
            value = Value::List(vec![value]);
        }
        Description {
            title: "t".into(),
            fields: vec![field("f", value)],
        }
    };
    let ok = nest(MAX_DEPTH);
    assert_eq!(Description::decode(&borsh::to_vec(&ok).unwrap()), Some(ok));
    assert_eq!(
        Description::decode(&borsh::to_vec(&nest(MAX_DEPTH + 1)).unwrap()),
        None
    );
    let mut bomb = vec![1, 0, 0, 0, b't', 1, 0, 0, 0, 1, 0, 0, 0, b'f'];
    for _ in 0..1 << 16 {
        bomb.extend([8, 1, 0, 0, 0]);
    }
    assert_eq!(Description::decode(&bomb), None);
    let ok = nest(MAX_DEPTH);
    assert_eq!(Description::decode(&borsh::to_vec(&ok).unwrap()), Some(ok));
}

#[test]
fn variants_read_in_tag_order() {
    assert_eq!(variants::<Op>(), ["Say", "Pay"]);
}

fn engine() -> wasmtime::Engine {
    let mut config = wasmtime::Config::new();
    config.consume_fuel(true);
    wasmtime::Engine::new(&config).unwrap()
}

fn module(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).unwrap()
}

fn answering(data: &str, body: &str) -> Vec<u8> {
    module(&format!(
        r#"(module
            (memory (export "memory") 1)
            (data (i32.const 1024) "{data}")
            (func (export "alloc") (param i32) (result i32) i32.const 0)
            (func (export "describe") (param i32 i32) (result i64) {body}))"#
    ))
}

const HI: &str = "\\02\\00\\00\\00hi\\00\\00\\00\\00";

#[test]
fn a_section_is_read_out_of_a_program_and_run() {
    let engine = engine();
    let section = answering(HI, "i64.const 0x4000000000a");
    let mut program = module("(module)");
    let name = SECTION.as_bytes();
    let mut body = vec![name.len() as u8];
    body.extend_from_slice(name);
    body.extend_from_slice(&section);
    program.push(0);
    let mut size = body.len();
    while size >= 0x80 {
        program.push((size as u8 & 0x7f) | 0x80);
        size >>= 7;
    }
    program.push(size as u8);
    program.extend_from_slice(&body);
    assert_eq!(crate::section(&program), Some(&section[..]));
    assert_eq!(crate::section(&module("(module)")), None);
    assert_eq!(crate::section(b"not wasm"), None);

    let compiled = host::compile(&engine, &section).unwrap();
    let description = host::run(&engine, &compiled, &[1, 2, 3]).unwrap();
    assert_eq!(description.title, "hi");
}

#[test]
fn a_hostile_section_describes_nothing() {
    let engine = engine();
    let runs = |section: Vec<u8>| {
        host::compile(&engine, &section)
            .and_then(|compiled| host::run(&engine, &compiled, &[1, 2, 3]))
    };
    assert_eq!(runs(answering("", "(loop (br 0)) i64.const 0")), None);
    let huge = module(
        r#"(module
            (memory (export "memory") 1)
            (func (export "alloc") (param i32) (result i32)
                (drop (memory.grow (i32.const 4096))) i32.const 0)
            (func (export "describe") (param i32 i32) (result i64) i64.const 0))"#,
    );
    assert_eq!(runs(huge), None);
    let declared = module(
        r#"(module (memory (export "memory") 2000)
            (func (export "alloc") (param i32) (result i32) i32.const 0)
            (func (export "describe") (param i32 i32) (result i64) i64.const 0))"#,
    );
    assert_eq!(runs(declared), None);
    assert_eq!(runs(answering(HI, "i64.const 0xffff00000010")), None);
    assert_eq!(runs(answering(HI, "i64.const 0x40000100000")), None);
    assert_eq!(runs(answering("\\ff\\ff", "i64.const 0x40000000002")), None);
    assert_eq!(runs(answering(HI, "i64.const 0x4000000000b")), None);
    assert_eq!(runs(answering("", "unreachable")), None);
    let importing = module(
        r#"(module (import "env" "f" (func))
            (memory (export "memory") 1)
            (func (export "alloc") (param i32) (result i32) i32.const 0)
            (func (export "describe") (param i32 i32) (result i64) i64.const 0))"#,
    );
    assert!(host::compile(&engine, &importing).is_none());
    assert!(host::compile(&engine, b"(module)").is_none());
    assert_eq!(
        runs(module("(module (memory (export \"memory\") 1))")),
        None
    );
    let free = wasmtime::Engine::default();
    let compiled = host::compile(&free, &answering(HI, "i64.const 0x4000000000a")).unwrap();
    assert_eq!(host::run(&free, &compiled, &[]), None);
}
