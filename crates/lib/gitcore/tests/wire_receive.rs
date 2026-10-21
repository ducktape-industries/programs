// pkt-line and sideband goldens, receive-pack request parsing, ref advertisement and report-status.

#[path = "common/mod.rs"]
mod common;

use common::sha1;
use gitcore::wire::pktline::{self, Pkt, Reader};
use gitcore::wire::receive::{advertise_refs, parse_request, report_status};
use gitcore::wire::sideband::{self, Band};
use gitcore::{Error, Hash, Oid};
use std::collections::BTreeMap;

#[test]
fn pktline_goldens() {
    assert_eq!(pktline::encode(b"hello\n"), b"000ahello\n");
    assert_eq!(pktline::encode(b""), b"0004");
    assert_eq!(pktline::flush(), b"0000");
    assert_eq!(pktline::delim(), b"0001");
    assert_eq!(pktline::response_end(), b"0002");
    let big = vec![b'x'; pktline::MAX_DATA];
    let encoded = pktline::encode(&big);
    assert_eq!(&encoded[..4], b"fff0");
    assert_eq!(encoded.len(), pktline::MAX_PKT);
}

#[test]
fn reader_yields_packets_and_reports_consumed_bytes() {
    let input = b"000ahello\n0001000000020006ab0000PACKrest";
    let mut reader = Reader::new(input);
    assert_eq!(reader.next().unwrap().unwrap(), Pkt::Data(b"hello\n"));
    assert_eq!(reader.next().unwrap().unwrap(), Pkt::Delim);
    assert_eq!(reader.next().unwrap().unwrap(), Pkt::Flush);
    assert_eq!(reader.next().unwrap().unwrap(), Pkt::ResponseEnd);
    assert_eq!(reader.next().unwrap().unwrap(), Pkt::Data(b"ab"));
    assert_eq!(reader.next().unwrap().unwrap(), Pkt::Flush);
    assert_eq!(reader.consumed(), input.len() - 8);
    assert_eq!(reader.rest(), b"PACKrest");
    assert_eq!(Reader::new(b"0003").next().unwrap(), Err(Error::BadPktLine));
    assert_eq!(Reader::new(b"00zz").next().unwrap(), Err(Error::BadPktLine));
    assert_eq!(
        Reader::new(b"0010abc").next().unwrap(),
        Err(Error::Truncated)
    );
    assert_eq!(Reader::new(b"00").next().unwrap(), Err(Error::Truncated));
    assert!(Reader::new(b"").next().is_none());
    assert_eq!(pktline::strip_newline(b"abc\n"), b"abc");
    assert_eq!(pktline::strip_newline(b"abc"), b"abc");
}

#[test]
fn sideband_splits_at_the_64k_limit() {
    assert_eq!(sideband::encode(Band::Progress, b"hi"), b"0007\x02hi");
    assert!(sideband::encode(Band::Pack, b"").is_empty());
    let data = vec![7u8; sideband::MAX_CHUNK + 1];
    let encoded = sideband::encode(Band::Pack, &data);
    let mut reader = Reader::new(&encoded);
    let Pkt::Data(first) = reader.next().unwrap().unwrap() else {
        panic!();
    };
    assert_eq!(first.len(), sideband::MAX_CHUNK + 1);
    assert_eq!(first[0], 1);
    assert_eq!(reader.next().unwrap().unwrap(), Pkt::Data(&[1, 7]));
    assert!(reader.next().is_none());

    let mut packets = Vec::new();
    let mut sink = |bytes: &[u8]| packets.push(bytes.to_vec());
    let mut writer = sideband::Writer::new(Band::Pack, &mut sink);
    writer.write(&vec![1u8; 40000]);
    writer.write(&vec![2u8; 40000]);
    writer.finish();
    assert_eq!(packets.len(), 2);
    assert_eq!(packets[0].len(), 5 + sideband::MAX_CHUNK);
    assert_eq!(packets[1].len(), 5 + 80000 - sideband::MAX_CHUNK);
}

fn zero() -> Oid {
    Hash::Sha1.zero()
}

#[test]
fn parse_push_request_with_capabilities_and_pack() {
    let a = sha1("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let b = sha1("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    let mut request = Vec::new();
    let first = format!(
        "{} {} refs/heads/main\0report-status side-band-64k object-format=sha1 agent=git/2.55",
        zero(),
        a
    );
    pktline::push(&mut request, first.as_bytes());
    let second = format!("{} {} refs/heads/old\n", a, zero());
    pktline::push(&mut request, second.as_bytes());
    let third = format!("{} {} refs/tags/v1", zero(), b);
    pktline::push(&mut request, third.as_bytes());
    request.extend_from_slice(pktline::flush());
    request.extend_from_slice(b"PACK...");
    let parsed = parse_request(&request, Hash::Sha1).unwrap();
    assert_eq!(parsed.commands.len(), 3);
    assert_eq!(parsed.commands[0].name, b"refs/heads/main");
    assert_eq!(parsed.commands[0].old, zero());
    assert_eq!(parsed.commands[0].new, a);
    assert_eq!(parsed.commands[1].name, b"refs/heads/old");
    assert_eq!(parsed.commands[1].new, zero());
    assert_eq!(parsed.commands[2].new, b);
    assert_eq!(parsed.capabilities.len(), 4);
    assert!(parsed.has_capability(b"side-band-64k"));
    assert!(!parsed.has_capability(b"quiet"));
    assert_eq!(parsed.pack, b"PACK...");
    assert!(parsed.shallow.is_empty());

    let empty = parse_request(b"0000", Hash::Sha1).unwrap();
    assert!(empty.commands.is_empty());
    assert!(empty.pack.is_empty());

    let mut wrong_hash = Vec::new();
    pktline::push(
        &mut wrong_hash,
        format!("{} {} refs/heads/x\0object-format=sha256", zero(), a).as_bytes(),
    );
    wrong_hash.extend_from_slice(pktline::flush());
    assert_eq!(
        parse_request(&wrong_hash, Hash::Sha1),
        Err(Error::WrongHash {
            expected: Hash::Sha1,
            actual: Hash::Sha256
        })
    );
    let mut bad = Vec::new();
    pktline::push(&mut bad, b"not a command");
    assert_eq!(parse_request(&bad, Hash::Sha1), Err(Error::BadRequest));
}

#[test]
fn parse_push_request_after_a_shallow_clones_boundary() {
    let a = sha1("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let b = sha1("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    let s = sha1("cccccccccccccccccccccccccccccccccccccccc");
    let mut request = Vec::new();
    pktline::push(&mut request, format!("shallow {s}\n").as_bytes());
    let first = format!("{a} {b} refs/heads/main\0report-status object-format=sha1");
    pktline::push(&mut request, first.as_bytes());
    request.extend_from_slice(pktline::flush());
    let parsed = parse_request(&request, Hash::Sha1).unwrap();
    assert_eq!(parsed.commands.len(), 1);
    assert_eq!(parsed.commands[0].name, b"refs/heads/main");
    assert_eq!(parsed.commands[0].old, a);
    assert_eq!(parsed.commands[0].new, b);
    assert!(parsed.has_capability(b"report-status"));
    assert!(parsed.pack.is_empty());
    assert_eq!(parsed.shallow, [s]);

    let mut late = Vec::new();
    pktline::push(&mut late, first.as_bytes());
    pktline::push(&mut late, format!("shallow {s}\n").as_bytes());
    late.extend_from_slice(pktline::flush());
    assert_eq!(parse_request(&late, Hash::Sha1), Err(Error::BadRequest));
}

#[test]
fn advertisement_goldens() {
    let empty = advertise_refs(
        &BTreeMap::new(),
        Hash::Sha1,
        &[b"report-status", b"side-band-64k"],
    );
    let expected = "001f# service=git-receive-pack\n0000\
        006c\
        0000000000000000000000000000000000000000 capabilities^{}\0report-status side-band-64k object-format=sha1\n\
        0000";
    assert_eq!(String::from_utf8_lossy(&empty), expected);

    let mut refs = BTreeMap::new();
    refs.insert(
        b"refs/heads/main".to_vec(),
        sha1("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
    );
    refs.insert(
        b"refs/heads/dev".to_vec(),
        sha1("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
    );
    let body = advertise_refs(&refs, Hash::Sha1, &[b"report-status"]);
    let expected = "001f# service=git-receive-pack\n0000\
        005d\
        bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb refs/heads/dev\0report-status object-format=sha1\n\
        003d\
        aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa refs/heads/main\n\
        0000";
    assert_eq!(String::from_utf8_lossy(&body), expected);
}

#[test]
fn report_status_goldens() {
    let results = vec![
        (b"refs/heads/main".to_vec(), Ok(())),
        (
            b"refs/heads/dev".to_vec(),
            Err(b"non-fast-forward".to_vec()),
        ),
    ];
    let plain = report_status(None, &results, false);
    assert_eq!(
        String::from_utf8_lossy(&plain),
        "000eunpack ok\n0017ok refs/heads/main\n0027ng refs/heads/dev non-fast-forward\n0000"
    );
    let banded = report_status(Some(b"bad delta"), &results, true);
    let mut reader = Reader::new(&banded);
    let Pkt::Data(inner) = reader.next().unwrap().unwrap() else {
        panic!();
    };
    assert_eq!(inner[0], 1);
    assert!(inner[1..].starts_with(b"0015unpack bad delta\n"));
    assert!(inner.ends_with(b"0000"));
    assert_eq!(reader.next().unwrap().unwrap(), Pkt::Flush);
    assert!(reader.next().is_none());
}
