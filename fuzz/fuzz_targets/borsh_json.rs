//! borsh-json's `decode` against every program's committed interface: the first byte picks the
//! interface, in file-name order, the next two (little-endian) the declaration, of every one its
//! schema defines, and the rest are the bytes read as it.
#![no_main]

use std::fs;
use std::sync::LazyLock;

use borsh_json::Interface;
use libfuzzer_sys::fuzz_target;

static READ: LazyLock<Vec<(Interface, Vec<String>)>> = LazyLock::new(|| {
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../crates/sdk/founding/interfaces"
    );
    let mut paths: Vec<_> = fs::read_dir(dir)
        .expect("the committed interfaces")
        .map(|entry| entry.expect("an interface").path())
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|path| {
            let section = fs::read(path).expect("an interface reads");
            let interface = Interface::read(&section).expect("a committed interface is one");
            let declarations = interface.schema.definitions();
            let declarations = declarations.map(|(name, _)| name.clone()).collect();
            (interface, declarations)
        })
        .collect()
});

fuzz_target!(|data: &[u8]| {
    let [which, low, high, bytes @ ..] = data else {
        return;
    };
    let (interface, declarations) = &READ[usize::from(*which) % READ.len()];
    let index = usize::from(u16::from_le_bytes([*low, *high])) % declarations.len();
    let _ = borsh_json::decode(&interface.schema, &declarations[index], "value", bytes);
});
