// receive-pack protocol v1: ref advertisement, push request parsing, report-status.

use super::{pktline, sideband, smart_http_service_header};
use crate::error::{Error, Result};
use crate::oid::{Hash, Oid};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefCommand {
    pub old: Oid,
    pub new: Oid,
    pub name: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PushRequest<'a> {
    pub commands: Vec<RefCommand>,
    pub capabilities: Vec<Vec<u8>>,
    pub pack: &'a [u8],
    pub shallow: Vec<Oid>,
}

impl PushRequest<'_> {
    pub fn has_capability(&self, name: &[u8]) -> bool {
        self.capabilities.iter().any(|c| c == name)
    }
}

pub fn parse_request(bytes: &[u8], hash: Hash) -> Result<PushRequest<'_>> {
    let mut reader = pktline::Reader::new(bytes);
    let mut commands = Vec::new();
    let mut capabilities = Vec::new();
    let mut shallow = Vec::new();
    while let Some(line) = pktline::read_data(&mut reader)? {
        let first = commands.is_empty();
        let boundary = line.strip_prefix(b"shallow ").filter(|_| first);
        if let Some(boundary) = boundary {
            shallow.push(Oid::from_hex(hash, boundary)?);
            continue;
        }
        let (command, caps) = match line.iter().position(|byte| *byte == 0) {
            Some(nul) if first => (&line[..nul], Some(&line[nul + 1..])),
            Some(_) => return Err(Error::BadRequest),
            None => (line, None),
        };
        if let Some(caps) = caps {
            capabilities = caps
                .split(|byte| *byte == b' ')
                .filter(|c| !c.is_empty())
                .map(<[u8]>::to_vec)
                .collect();
        }
        commands.push(parse_command(command, hash)?);
    }
    for capability in &capabilities {
        let Some(format) = capability.strip_prefix(b"object-format=") else {
            continue;
        };
        let Some(actual) = Hash::from_name(format) else {
            return Err(Error::Unsupported);
        };
        let matches = actual == hash;
        if !matches {
            return Err(Error::WrongHash {
                expected: hash,
                actual,
            });
        }
    }
    Ok(PushRequest {
        commands,
        capabilities,
        pack: reader.rest(),
        shallow,
    })
}

fn parse_command(line: &[u8], hash: Hash) -> Result<RefCommand> {
    let hex = hash.size() * 2;
    let shape_ok = line.len() > hex * 2 + 2 && line[hex] == b' ' && line[hex * 2 + 1] == b' ';
    if !shape_ok {
        return Err(Error::BadRequest);
    }
    let old = Oid::from_hex(hash, &line[..hex])?;
    let new = Oid::from_hex(hash, &line[hex + 1..hex * 2 + 1])?;
    let name = line[hex * 2 + 2..].to_vec();
    Ok(RefCommand { old, new, name })
}

pub fn advertise_refs(
    refs: &BTreeMap<Vec<u8>, Oid>,
    hash: Hash,
    capabilities: &[&[u8]],
) -> Vec<u8> {
    let mut out = smart_http_service_header(b"git-receive-pack");
    let mut caps = Vec::new();
    for capability in capabilities {
        caps.extend_from_slice(capability);
        caps.push(b' ');
    }
    caps.extend_from_slice(b"object-format=");
    caps.extend_from_slice(hash.name().as_bytes());
    let mut first = true;
    for (name, id) in refs {
        let mut line = id.to_hex().into_bytes();
        line.push(b' ');
        line.extend_from_slice(name);
        if first {
            line.push(0);
            line.extend_from_slice(&caps);
        }
        first = false;
        pktline::push_line(&mut out, &line);
    }
    if first {
        let mut line = hash.zero().to_hex().into_bytes();
        line.extend_from_slice(b" capabilities^{}\0");
        line.extend_from_slice(&caps);
        pktline::push_line(&mut out, &line);
    }
    out.extend_from_slice(pktline::flush());
    out
}

pub type RefStatus = (Vec<u8>, std::result::Result<(), Vec<u8>>);

pub fn report_status(
    unpack_error: Option<&[u8]>,
    results: &[RefStatus],
    sideband: bool,
) -> Vec<u8> {
    let mut report = Vec::new();
    let mut unpack = b"unpack ".to_vec();
    unpack.extend_from_slice(unpack_error.unwrap_or(b"ok"));
    pktline::push_line(&mut report, &unpack);
    for (name, result) in results {
        let mut line = Vec::new();
        match result {
            Ok(()) => {
                line.extend_from_slice(b"ok ");
                line.extend_from_slice(name);
            }
            Err(reason) => {
                line.extend_from_slice(b"ng ");
                line.extend_from_slice(name);
                line.push(b' ');
                line.extend_from_slice(reason);
            }
        }
        pktline::push_line(&mut report, &line);
    }
    report.extend_from_slice(pktline::flush());
    if !sideband {
        return report;
    }
    let mut out = sideband::encode(sideband::Band::Pack, &report);
    out.extend_from_slice(pktline::flush());
    out
}
