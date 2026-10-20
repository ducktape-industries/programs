//! gitcore's upload-pack v2 request reader: the first byte's low bit picks SHA-256 over SHA-1,
//! the rest is the request.
#![no_main]

use gitcore::Hash;
use gitcore::wire::upload::parse_command;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let [flags, request @ ..] = data else {
        return;
    };
    let hash = match flags & 1 {
        0 => Hash::Sha1,
        _ => Hash::Sha256,
    };
    let _ = parse_command(request, hash);
});
