//! The light client one network runs of another: a header's certificate, block, roots and
//! validator set checked against the set a link trusts, a packet's, an ack's and a low-water
//! mark's proofs against a crosspond root a header gave, and this network's id from its genesis.
use ::program::{Error, QueryCtx, Scheme, code, invalid, stale};
use abi::Root;
use commonware_codec::{Encode, ReadExt, varint::UInt};

use crate::state::{Accepted, LinkRow, Trusted};
use crate::{
    CertificateKind, Header, NetworkId, PROGRAM, Packet, Proven, ProvenAck, ProvenLow,
    Settled as SettledRow, commitment_key, low_key, settled_key,
};

const SETTLED: &[u8] = b"\xffducktape:settled";
const FINALIZE: &[u8] = b"_FINALIZE";
const NETWORK: &str = "$network";
const SIGNATURE: usize = 64;

pub(crate) fn genesis(ctx: &QueryCtx, name: &[u8], time: u64) -> NetworkId {
    let parent = ctx.sha256(name.to_vec());
    let block = [
        UInt(0u64).encode().as_ref(),
        parent.as_slice(),
        UInt(time).encode().as_ref(),
        UInt(0u32).encode().as_ref(),
    ]
    .concat();
    ctx.sha256(block)
}

pub(crate) struct Checked {
    pub(crate) trusted: Trusted,
    pub(crate) header: Accepted,
}

pub(crate) fn check(ctx: &QueryCtx, link: &LinkRow, header: &Header) -> Result<Checked, Error> {
    match (link.kind, header.kind) {
        (CertificateKind::Ed25519SimplexFinalize, CertificateKind::Ed25519SimplexFinalize) => {
            ed25519_simplex_finalize(ctx, link, header)
        }
    }
}

fn ed25519_simplex_finalize(
    ctx: &QueryCtx,
    link: &LinkRow,
    header: &Header,
) -> Result<Checked, Error> {
    let validators = seated(&header.validators)?;
    let finalized = Finalized::read(&header.certificate, validators.len())?;
    let epoch = finalized.epoch;
    if epoch <= link.trusted.epoch {
        return Err(stale(format!(
            "the link to {} trusts epoch {}, and this header is for epoch {epoch}: send a later one",
            link.name, link.trusted.epoch
        )));
    }
    if finalized.payload != ctx.sha256(header.block.clone()) {
        return Err(invalid(
            "the certificate finalizes another block than the header's",
        ));
    }
    let block = Block::read(&header.block)?;
    let first = epoch
        .checked_mul(link.epoch_length)
        .ok_or_else(|| invalid(format!("epoch {epoch} opens past the last height")))?;
    if block.height != first {
        return Err(invalid(format!(
            "the block is at height {}, and epoch {epoch} opens at height {first}",
            block.height
        )));
    }
    let settled = block.settled()?;
    if settled.height.checked_add(1) != Some(first) {
        return Err(invalid(format!(
            "the block settles height {}, not the close of the epoch before {epoch}",
            settled.height
        )));
    }
    if ctx.sha256(abi::encode(&header.roots)) != settled.root {
        return Err(invalid("the roots are not the ones the block settles"));
    }
    let network = root_of(&header.roots, NETWORK)?;
    let key = format!("validators/{epoch}");
    let seated_here = state_proof::holds(
        &network,
        header.validators_proof.location,
        &header.validators_proof.proof,
        key.as_bytes(),
        &abi::encode(&header.validators),
    );
    if !seated_here {
        return Err(invalid(format!(
            "the proof does not hold the validators of epoch {epoch} under the {NETWORK} root"
        )));
    }
    let quorum = validators.len() - (validators.len() - 1) / 3;
    if finalized.signers.len() < quorum {
        return Err(invalid(format!(
            "{} of the {} validators of epoch {epoch} signed, short of a quorum of {quorum}",
            finalized.signers.len(),
            validators.len()
        )));
    }
    let namespace = [link.name.as_bytes(), FINALIZE].concat();
    for (index, signature) in finalized.signers.iter().zip(&finalized.signatures) {
        let signed = ctx.verify(
            Scheme::Ed25519,
            validators[*index].clone(),
            namespace.clone(),
            finalized.proposal.clone(),
            signature.to_vec(),
        )?;
        if !signed {
            return Err(invalid(format!(
                "validator {index} of epoch {epoch} did not sign the certificate"
            )));
        }
    }
    let trusted = &link.trusted.validators;
    let overlap = finalized
        .signers
        .iter()
        .filter(|index| trusted.contains(&validators[**index]))
        .count();
    if overlap * 3 <= trusted.len() {
        return Err(Error::new(
            code::NOT_YET,
            format!(
                "{overlap} of the {} validators the link trusts at epoch {} signed, not more than \
                 a third: send the headers of the epochs between first",
                trusted.len(),
                link.trusted.epoch
            ),
        ));
    }
    let root = root_of(&header.roots, PROGRAM)?;
    Ok(Checked {
        trusted: Trusted { epoch, validators },
        header: Accepted { epoch, root },
    })
}

pub(crate) fn proven(
    home: &NetworkId,
    from: &NetworkId,
    root: &[u8; 32],
    proven: &Proven,
) -> Result<(), Error> {
    let Proven { packet, proof } = proven;
    let Packet {
        source,
        destination,
        seq,
        ..
    } = packet;
    if source != from {
        return Err(invalid(format!(
            "packet {seq} comes from another network than the link's"
        )));
    }
    if destination != home {
        return Err(invalid(format!(
            "packet {seq} is for another network than this one"
        )));
    }
    let held = state_proof::holds(
        root,
        proof.location,
        &proof.proof,
        &commitment_key(destination, *seq),
        &abi::encode(packet),
    );
    if !held {
        return Err(invalid(format!(
            "the proof does not hold packet {seq} under the header's crosspond root"
        )));
    }
    Ok(())
}

pub(crate) fn acked(
    ctx: &QueryCtx,
    home: &NetworkId,
    to: &NetworkId,
    root: &[u8; 32],
    owed: &Packet,
    proven: &ProvenAck,
) -> Result<(), Error> {
    let ProvenAck { settled, proof } = proven;
    let SettledRow {
        source,
        destination,
        seq,
        packet,
        ..
    } = settled;
    if source != home {
        return Err(invalid(format!(
            "the ack of packet {seq} settles a packet another network sent"
        )));
    }
    if destination != to {
        return Err(invalid(format!(
            "the ack of packet {seq} settles a packet sent to another network"
        )));
    }
    if *seq != owed.seq {
        return Err(invalid(format!(
            "the ack of packet {seq} answers for packet {}",
            owed.seq
        )));
    }
    if *packet != ctx.sha256(abi::encode(owed)) {
        return Err(invalid(format!(
            "the ack of packet {seq} settles another packet than the one owed"
        )));
    }
    let held = state_proof::holds(
        root,
        proof.location,
        &proof.proof,
        &settled_key(source, *seq),
        &abi::encode(settled),
    );
    if !held {
        return Err(invalid(format!(
            "the proof does not hold the ack of packet {seq} under the other network's crosspond root"
        )));
    }
    Ok(())
}

pub(crate) fn low(home: &NetworkId, root: &[u8; 32], low: &ProvenLow) -> Result<(), Error> {
    let ProvenLow { n, proof } = low;
    let held = state_proof::holds(root, proof.location, &proof.proof, &low_key(home, *n), &[]);
    if !held {
        return Err(invalid(format!(
            "the proof does not hold the low-water mark {n} for this network under the header's \
             crosspond root"
        )));
    }
    Ok(())
}

pub(crate) fn seated(validators: &[Vec<u8>]) -> Result<Vec<Vec<u8>>, Error> {
    let mut sorted = validators.to_vec();
    sorted.sort();
    sorted.dedup();
    if sorted.is_empty() {
        return Err(invalid("a validator set holds at least one key"));
    }
    if sorted.len() != validators.len() {
        return Err(invalid("the validator set names a key twice"));
    }
    Ok(sorted)
}

fn root_of(roots: &[(String, Root)], program: &str) -> Result<[u8; 32], Error> {
    roots
        .iter()
        .find(|(id, _)| id == program)
        .map(|(_, root)| root.0)
        .ok_or_else(|| invalid(format!("the roots hold none for {program}")))
}

struct Finalized {
    epoch: u64,
    proposal: Vec<u8>,
    payload: [u8; 32],
    signers: Vec<usize>,
    signatures: Vec<[u8; SIGNATURE]>,
}

impl Finalized {
    fn read(bytes: &[u8], seats: usize) -> Result<Finalized, Error> {
        let mut rest = bytes;
        let epoch = uint(&mut rest)?;
        uint(&mut rest)?;
        uint(&mut rest)?;
        let payload = array(&mut rest)?;
        let proposal = bytes[..bytes.len() - rest.len()].to_vec();
        let width = u64::from_be_bytes(array(&mut rest)?);
        if usize::try_from(width).ok() != Some(seats) {
            return Err(invalid(format!(
                "the certificate counts {width} signer seats, and the epoch seats {seats}"
            )));
        }
        let bitmap = take(&mut rest, seats.div_ceil(8))?;
        let signers: Vec<usize> = (0..seats)
            .filter(|seat| bitmap[seat / 8] & (1 << (seat % 8)) != 0)
            .collect();
        let trailing = (seats..bitmap.len() * 8).any(|bit| bitmap[bit / 8] & (1 << (bit % 8)) != 0);
        if trailing {
            return Err(invalid(
                "the certificate's signer bitmap sets a bit past its seats",
            ));
        }
        let count = uint(&mut rest)?;
        if usize::try_from(count).ok() != Some(signers.len()) {
            return Err(invalid(format!(
                "the certificate carries {count} signatures for {} signers",
                signers.len()
            )));
        }
        let signatures = signers
            .iter()
            .map(|_| array(&mut rest))
            .collect::<Result<Vec<_>, Error>>()?;
        if !rest.is_empty() {
            return Err(invalid("the certificate runs on past its signatures"));
        }
        Ok(Finalized {
            epoch,
            proposal,
            payload,
            signers,
            signatures,
        })
    }
}

struct Block<'a> {
    height: u64,
    last: Option<&'a [u8]>,
}

struct Settled {
    height: u64,
    root: [u8; 32],
}

impl<'a> Block<'a> {
    fn read(bytes: &'a [u8]) -> Result<Block<'a>, Error> {
        let mut rest = bytes;
        let height = uint(&mut rest)?;
        array::<32>(&mut rest)?;
        uint(&mut rest)?;
        let frames = uint(&mut rest)?;
        let mut last = None;
        for _ in 0..frames {
            let length = usize::try_from(uint(&mut rest)?)
                .map_err(|_| invalid("a frame of the block is longer than this host holds"))?;
            last = Some(take(&mut rest, length)?);
        }
        if !rest.is_empty() {
            return Err(invalid("the block runs on past its frames"));
        }
        Ok(Block { height, last })
    }

    fn settled(&self) -> Result<Settled, Error> {
        let unsettled = || invalid("the block's last frame is no Settled record");
        let record = self
            .last
            .and_then(|frame| frame.strip_prefix(SETTLED))
            .ok_or_else(unsettled)?;
        let (height, root) = record.split_first_chunk::<8>().ok_or_else(unsettled)?;
        let root = <[u8; 32]>::try_from(root).map_err(|_| unsettled())?;
        Ok(Settled {
            height: u64::from_le_bytes(*height),
            root,
        })
    }
}

fn uint(rest: &mut &[u8]) -> Result<u64, Error> {
    UInt::<u64>::read(rest)
        .map(u64::from)
        .map_err(|error| invalid(format!("the header does not decode: {error}")))
}

fn take<'a>(rest: &mut &'a [u8], length: usize) -> Result<&'a [u8], Error> {
    let (head, tail) = rest
        .split_at_checked(length)
        .ok_or_else(|| invalid("the header ends early"))?;
    *rest = tail;
    Ok(head)
}

fn array<const N: usize>(rest: &mut &[u8]) -> Result<[u8; N], Error> {
    let head = take(rest, N)?;
    <[u8; N]>::try_from(head).map_err(|_| invalid("the header ends early"))
}
