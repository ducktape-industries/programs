//! The rules: who may call each op, and what each op checks and writes.
use ::program::{
    Cause, Ctx, Env, Error, ExecCtx, MessageId, Origin, Outcome, QueryCtx, Reply as Wanted, code,
    invalid, not_found, stale, unauthorized, unexpected_reply, wrong_state,
};
use abi::Refusal;
use abi::role::egress::{self, DEFAULT_WAIT, Delivery, Request};
use abi::role::identity;

use crate::light::{self, Checked};
use crate::queries::home;
use crate::state::{
    HOME, LINKED, LINKS, LOWS, LinkRow, NAMED, NAMES, OUTBOX, PORTS, PRUNED, Parked, RECEIPTS,
    REPLIES, SETTLED, STATUSES, Trusted,
};
use crate::{
    AccountNumber, Acked, CertificateKind, Changed, Foreign, Header, NetworkId, Op, Packet, Proven,
    ProvenLow, Settled, Status, max_packet,
};

fn hex(id: &NetworkId) -> String {
    id.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn init(ctx: &ExecCtx, params: &[u8]) -> Result<(), Error> {
    let env = ctx.env();
    let founding = env.height == 0;
    let home = match (params.is_empty(), founding) {
        (true, true) => light::genesis(ctx, &env.chain_id, env.time),
        (true, false) => {
            return Err(invalid(
                "crosspond published after its network's founding takes the network's id as its \
                 params: the 32-byte genesis digest `ducktape status` shows",
            ));
        }
        (false, _) => abi::decode::<NetworkId>(params).map_err(|refusal| {
            invalid(format!(
                "crosspond's params are its network's 32-byte genesis digest: {}",
                refusal.sentence
            ))
        })?,
    };
    if founding {
        let genesis = light::genesis(ctx, &env.chain_id, env.time);
        if home != genesis {
            return Err(invalid(format!(
                "the params name network {}, and this network's genesis digest is {}",
                hex(&home),
                hex(&genesis)
            )));
        }
    }
    HOME.put(ctx, &home);
    Ok(())
}

fn link(ctx: &QueryCtx, network: &NetworkId) -> Result<LinkRow, Error> {
    LINKS
        .get(ctx, network)?
        .ok_or_else(|| not_found(format!("no link to network {}", hex(network))))
}

fn open_link(ctx: &QueryCtx, network: &NetworkId) -> Result<LinkRow, Error> {
    let row = link(ctx, network)?;
    if row.closed {
        return Err(wrong_state(format!("the link to {} is closed", row.name)));
    }
    Ok(row)
}

pub(crate) fn open(
    ctx: &ExecCtx,
    network: NetworkId,
    name: String,
    epoch_length: u64,
    epoch: u64,
    kind: CertificateKind,
    validators: Vec<Vec<u8>>,
) -> Result<(), Error> {
    ctx.env().authority()?;
    let before = LINKS.get(ctx, &network)?;
    if let Some(LinkRow { closed: false, .. }) = &before {
        return Err(::program::already_exists(format!(
            "network {} is linked already",
            hex(&network)
        )));
    }
    if epoch_length == 0 {
        return Err(invalid("an epoch is at least one block long"));
    }
    let validators = light::seated(&validators)?;
    let trusted = Trusted { epoch, validators };
    let row = match before {
        None => LinkRow {
            name,
            epoch_length,
            kind,
            trusted,
            header: None,
            next_in: 1,
            next_out: 1,
            refused: None,
            closed: false,
            asked: None,
        },
        Some(closed) => {
            LINKED.remove(ctx, &(closed.name.clone(), network));
            LinkRow {
                name,
                epoch_length,
                kind,
                trusted,
                header: None,
                refused: None,
                closed: false,
                ..closed
            }
        }
    };
    LINKED.insert(ctx, &(row.name.clone(), network));
    LINKS.put(ctx, &network, &row);
    Ok(())
}

pub(crate) fn repin(
    ctx: &ExecCtx,
    network: NetworkId,
    epoch: u64,
    kind: CertificateKind,
    validators: Vec<Vec<u8>>,
) -> Result<(), Error> {
    ctx.env().authority()?;
    let mut row = link(ctx, &network)?;
    let validators = light::seated(&validators)?;
    row.kind = kind;
    row.trusted = Trusted { epoch, validators };
    LINKS.put(ctx, &network, &row);
    Ok(())
}

pub(crate) fn close(ctx: &ExecCtx, network: NetworkId) -> Result<(), Error> {
    ctx.env().authority()?;
    let mut row = link(ctx, &network)?;
    row.closed = true;
    LINKS.put(ctx, &network, &row);
    Ok(())
}

pub(crate) fn port(ctx: &ExecCtx, role: String, mut from: Vec<NetworkId>) -> Result<(), Error> {
    ctx.env().authority()?;
    from.sort();
    from.dedup();
    match from.is_empty() {
        true => PORTS.remove(ctx, &role),
        false => PORTS.put(ctx, &role, &from),
    }
    Ok(())
}

fn name_of(ctx: &QueryCtx, account: AccountNumber) -> Result<String, Error> {
    use identity::{Query, Reply};
    let identity = ctx.env().role(abi::role::IDENTITY)?.clone();
    match ctx.query::<Query, Reply>(&identity, &Query::Profile(account))? {
        Reply::Profile(Some(profile)) => Ok(profile.name),
        Reply::Profile(None) => Err(not_found(format!("account {account} has no profile"))),
        other => Err(unexpected_reply(&identity, "Profile", &other)),
    }
}

fn changed(ctx: &ExecCtx, to: &NetworkId, seq: u64, status: Status) {
    STATUSES.put(ctx, &(*to, seq), &status);
    let changed = Changed {
        to: *to,
        seq,
        status,
    };
    ctx.event(abi::encode(&changed));
}

pub(crate) struct Sending {
    pub(crate) to: NetworkId,
    pub(crate) port: String,
    pub(crate) op: Vec<u8>,
    pub(crate) deadline: Option<u64>,
}

pub(crate) async fn send(ctx: &Ctx, sending: Sending) -> Result<(), Error> {
    let account = ctx.sender_account()?;
    commit(ctx, sending, account, None).await
}

pub(crate) async fn send_for(
    ctx: &Ctx,
    sending: Sending,
    account: AccountNumber,
) -> Result<(), Error> {
    let env = ctx.env();
    let bound = env.roles.get(&sending.port);
    let by_port = matches!(&env.origin, Origin::Program(sender) if Some(sender) == bound);
    if !by_port {
        return Err(unauthorized(format!(
            "only the program bound to the {} role sends for an account through it, not {:?}",
            sending.port, env.origin
        )));
    }
    let via = Some(sending.port.clone());
    commit(ctx, sending, account, via).await
}

async fn commit(
    ctx: &Ctx,
    sending: Sending,
    from: AccountNumber,
    via: Option<String>,
) -> Result<(), Error> {
    let Sending {
        to,
        port,
        op,
        deadline,
    } = sending;
    let mut row = open_link(ctx, &to)?;
    let env = ctx.env();
    let seq = row.next_out;
    let packet = Packet {
        source: home(ctx)?,
        destination: to,
        seq,
        from,
        name: name_of(ctx, from)?,
        via,
        port,
        op,
        sent: env.time,
        deadline,
    };
    let encoded = abi::encode(&packet).len();
    let room = max_packet(&row.name);
    if encoded > room {
        return Err(::program::capacity(format!(
            "the packet is {encoded} bytes, past the {room} one relay frame to {} carries",
            row.name
        )));
    }
    OUTBOX.put(ctx, &(to, seq), &packet);
    changed(ctx, &to, seq, Status::Pending);
    row.next_out = seq.checked_add(1).ok_or_else(|| {
        Error::new(
            code::EXHAUSTED,
            "the link has sent its last sequence number",
        )
    })?;
    let pumping = row
        .asked
        .is_some_and(|asked| asked.saturating_add(DEFAULT_WAIT) >= env.height);
    if !pumping {
        let next = env
            .height
            .checked_add(1)
            .ok_or_else(|| Error::new(code::EXHAUSTED, "no height follows this one"))?;
        ctx.schedule(next, env.program.clone(), &Op::Pump { to })
            .await?;
        row.asked = Some(env.height);
    }
    LINKS.put(ctx, &to, &row);
    ctx.set_return_data(abi::encode(&seq));
    Ok(())
}

fn its_own_scheduled_run(env: &Env) -> Result<(), Error> {
    let scheduled = matches!(env.cause, Cause::Scheduled { .. });
    let from_itself = env.origin == Origin::Program(env.program.clone());
    let its_own = scheduled && from_itself;
    if its_own {
        return Ok(());
    }
    Err(unauthorized(
        "a relay is asked only by crosspond's own scheduled run",
    ))
}

fn owed(ctx: &QueryCtx, to: &NetworkId) -> Result<Vec<Packet>, Error> {
    Ok(OUTBOX
        .scan(ctx, OUTBOX.prefix_of(to))?
        .into_iter()
        .map(|(_, packet)| packet)
        .collect())
}

fn idle(ctx: &ExecCtx, to: &NetworkId) -> Result<(), Error> {
    let mut row = link(ctx, to)?;
    row.asked = None;
    LINKS.put(ctx, to, &row);
    Ok(())
}

fn refused(ctx: &ExecCtx, to: &NetworkId, reason: String) -> Result<(), Error> {
    let mut row = link(ctx, to)?;
    for packet in owed(ctx, to)? {
        let status = Status::Refused {
            reason: reason.clone(),
        };
        changed(ctx, to, packet.seq, status);
    }
    row.refused = Some(reason);
    LINKS.put(ctx, to, &row);
    Ok(())
}

fn unacked(ctx: &ExecCtx, to: &NetworkId, reason: String) -> Result<(), Error> {
    let mut row = link(ctx, to)?;
    row.refused = Some(reason);
    LINKS.put(ctx, to, &row);
    Ok(())
}

fn acked(ctx: &ExecCtx, to: &NetworkId, answer: Acked) -> Result<(), Error> {
    let home = home(ctx)?;
    let mut row = link(ctx, to)?;
    let root = match ack_root(ctx, &mut row, answer.epoch, answer.header.as_ref()) {
        Ok(root) => root,
        Err(unchecked) => return unacked(ctx, to, unchecked.message),
    };
    let mut refused = None;
    for proven in answer.acks {
        let seq = proven.settled.seq;
        let key = (*to, seq);
        let Some(packet) = OUTBOX.get(ctx, &key)? else {
            continue;
        };
        if let Err(unproven) = light::acked(ctx, &home, to, &root, &packet, &proven) {
            refused.get_or_insert(unproven.message);
            continue;
        }
        OUTBOX.remove(ctx, &key);
        changed(ctx, to, seq, Status::Done(proven.settled.result));
    }
    row.refused = refused;
    LINKS.put(ctx, to, &row);
    lowered(ctx, to, row.next_out)
}

fn ack_root(
    ctx: &QueryCtx,
    row: &mut LinkRow,
    epoch: u64,
    header: Option<&Header>,
) -> Result<[u8; 32], Error> {
    let held = row.header.as_ref().filter(|held| held.epoch == epoch);
    if let Some(held) = held {
        return Ok(held.root);
    }
    let Some(header) = header else {
        return Err(stale(format!(
            "the acks are proven at epoch {epoch} of {}, and the link holds no header of it",
            row.name
        )));
    };
    if row.closed {
        return Err(wrong_state(format!(
            "the link to {} is closed and takes no header",
            row.name
        )));
    }
    let Checked { trusted, header } = light::check(ctx, row, header)?;
    if header.epoch != epoch {
        return Err(invalid(format!(
            "the acks are proven at epoch {epoch}, and their header is of epoch {}",
            header.epoch
        )));
    }
    let root = header.root;
    row.trusted = trusted;
    row.header = Some(header);
    Ok(root)
}

fn lowered(ctx: &ExecCtx, to: &NetworkId, next_out: u64) -> Result<(), Error> {
    let lowest = OUTBOX
        .scan(ctx, OUTBOX.prefix_of(to))?
        .first()
        .map_or(next_out, |((_, seq), _)| *seq);
    let kept = LOWS.scan(ctx, LOWS.prefix_of(to))?;
    let risen = kept.iter().all(|(_, n)| *n < lowest);
    if !risen {
        return Ok(());
    }
    for key in kept {
        LOWS.remove(ctx, &key);
    }
    if lowest > 1 {
        LOWS.insert(ctx, &(*to, lowest));
    }
    Ok(())
}

pub(crate) async fn pump(ctx: &Ctx, to: NetworkId) -> Result<(), Error> {
    its_own_scheduled_run(&ctx.env())?;
    loop {
        if owed(ctx, &to)?.is_empty() {
            return idle(ctx, &to);
        }
        let asked = ctx.env().height;
        let mut row = link(ctx, &to)?;
        row.asked = Some(asked);
        LINKS.put(ctx, &to, &row);
        let request = egress::Op::Request {
            request: Request::Crosspond { link: to },
            delivery: Delivery::Whole,
            wait: None,
        };
        let answered = match ctx.env().role(abi::role::EGRESS) {
            Ok(egress) => ctx.call_raw(egress.clone(), abi::encode(&request)).await,
            Err(unbound) => Err(unbound),
        };
        let now = ctx.env().height;
        let expired = now >= asked.saturating_add(DEFAULT_WAIT);
        match answered {
            Ok(output) => match abi::decode::<Acked>(&output) {
                Ok(answer) => acked(ctx, &to, answer)?,
                Err(refusal) => unacked(
                    ctx,
                    &to,
                    format!("the relay answered with no acks: {}", refusal.sentence),
                )?,
            },
            Err(refusal) if now == asked => {
                refused(ctx, &to, refusal.message)?;
                return idle(ctx, &to);
            }
            Err(_) if expired => {}
            Err(refusal) => refused(ctx, &to, refusal.message)?,
        }
    }
}

pub(crate) fn take_header(ctx: &ExecCtx, from: NetworkId, header: Header) -> Result<(), Error> {
    let mut row = open_link(ctx, &from)?;
    let Checked { trusted, header } = light::check(ctx, &row, &header)?;
    row.trusted = trusted;
    row.header = Some(header);
    LINKS.put(ctx, &from, &row);
    Ok(())
}

fn not_yet(message: impl Into<String>) -> Error {
    Error::new(code::NOT_YET, message)
}

pub(crate) fn deliver(
    ctx: &ExecCtx,
    from: NetworkId,
    epoch: u64,
    low: Option<ProvenLow>,
    packets: Vec<Proven>,
) -> Result<(), Error> {
    let home = home(ctx)?;
    let mut row = open_link(ctx, &from)?;
    let Some(header) = &row.header else {
        return Err(not_yet(format!(
            "this network holds no header from {} yet: deliver after a Header lands",
            row.name
        )));
    };
    if epoch != header.epoch {
        return Err(stale(format!(
            "the link to {} holds the header of epoch {}, and the packets are proven at epoch \
             {epoch}: prove them against epoch {}",
            row.name, header.epoch, header.epoch
        )));
    }
    let mut last = None;
    for each in &packets {
        light::proven(&home, &from, &header.root, each)?;
        let seq = each.packet.seq;
        if last.is_some_and(|before| seq <= before) {
            return Err(invalid(format!("packet {seq} is out of sequence order")));
        }
        last = Some(seq);
    }
    if let Some(low) = &low {
        light::low(&home, &header.root, low)?;
        prune(ctx, &from, low.n)?;
    }
    let mut taken = Vec::new();
    for Proven { packet, .. } in packets {
        let seq = packet.seq;
        match seq.cmp(&row.next_in) {
            std::cmp::Ordering::Less => {
                if !RECEIPTS.has(ctx, &(from, seq)) {
                    return Err(Error::new(
                        code::DUPLICATE,
                        format!(
                            "packet {seq} from {} was taken, and {} holds its ack",
                            row.name, row.name
                        ),
                    ));
                }
            }
            std::cmp::Ordering::Greater => {
                return Err(not_yet(format!(
                    "packet {seq} from {} is ahead of packet {}: deliver that one first",
                    row.name, row.next_in
                )));
            }
            std::cmp::Ordering::Equal => {
                row.next_in = seq.checked_add(1).ok_or_else(|| {
                    Error::new(
                        code::EXHAUSTED,
                        "the link has taken its last sequence number",
                    )
                })?;
                taken.push(packet);
            }
        }
    }
    let name = row.name.clone();
    LINKS.put(ctx, &from, &row);
    for packet in taken {
        take(ctx, &from, &home, &name, packet)?;
    }
    Ok(())
}

fn prune(ctx: &ExecCtx, from: &NetworkId, n: u64) -> Result<(), Error> {
    let mark = PRUNED.get(ctx, from)?.unwrap_or(0);
    if n <= mark {
        return Ok(());
    }
    for (key, taken) in RECEIPTS.scan(ctx, RECEIPTS.prefix_of(from))? {
        if key.1 >= n {
            break;
        }
        if taken.is_none() {
            continue;
        }
        RECEIPTS.remove(ctx, &key);
        SETTLED.remove(ctx, &key);
    }
    PRUNED.put(ctx, from, &n);
    Ok(())
}

pub(crate) fn settle(
    ctx: &ExecCtx,
    key: &(NetworkId, u64),
    home: &NetworkId,
    packet: [u8; 32],
    result: Result<Vec<u8>, Refusal>,
) -> Result<(), Error> {
    let (source, seq) = *key;
    if SETTLED.has(ctx, key) {
        return Err(wrong_state(format!(
            "packet {seq} from network {} is settled already",
            hex(&source)
        )));
    }
    RECEIPTS.put(ctx, key, &Some(result.clone()));
    let settled = Settled {
        source,
        destination: *home,
        seq,
        packet,
        result,
    };
    SETTLED.put(ctx, key, &settled);
    Ok(())
}

fn heard(ctx: &ExecCtx, from: &NetworkId, account: AccountNumber, name: &str) -> Result<(), Error> {
    let key = (*from, account);
    if let Some(before) = NAMES.get(ctx, &key)? {
        NAMED.remove(ctx, &(*from, before, account));
    }
    NAMES.put(ctx, &key, &name.to_owned());
    NAMED.insert(ctx, &(*from, name.to_owned(), account));
    Ok(())
}

fn take(
    ctx: &ExecCtx,
    from: &NetworkId,
    home: &NetworkId,
    name: &str,
    packet: Packet,
) -> Result<(), Error> {
    let env = ctx.env();
    let seq = packet.seq;
    let key = (*from, seq);
    let digest = ctx.sha256(abi::encode(&packet));
    heard(ctx, from, packet.from, &packet.name)?;
    let settled = |result| settle(ctx, &key, home, digest, result);
    RECEIPTS.put(ctx, &key, &None);
    if packet.deadline.is_some_and(|deadline| env.time > deadline) {
        return settled(Err(Refusal::new(
            code::STALE,
            "the packet's deadline passed before it arrived",
        )));
    }
    let open = PORTS
        .get(ctx, &packet.port)?
        .is_some_and(|from_networks| from_networks.contains(from));
    if !open {
        return settled(Err(Refusal::new(
            code::UNAUTHORIZED,
            format!(
                "this network takes no packets for {} from {name}",
                packet.port
            ),
        )));
    }
    let Some(target) = env.roles.get(&packet.port).cloned() else {
        return settled(Err(Refusal::new(
            code::UNSUPPORTED,
            format!("no program fills the {} role on this network", packet.port),
        )));
    };
    let foreign = Foreign {
        network: *from,
        account: packet.from,
    };
    let id = ctx.emit_raw_foreign(foreign.into(), target, packet.op, Wanted::Wanted);
    let parked = Parked {
        key,
        packet: digest,
    };
    REPLIES.put(ctx, &(id.program, id.seq), &parked);
    Ok(())
}

pub(crate) fn replied(ctx: &ExecCtx, id: &MessageId, outcome: &Outcome) -> Result<(), Error> {
    let message = (id.program.clone(), id.seq);
    let Some(Parked { key, packet }) = REPLIES.get(ctx, &message)? else {
        return Ok(());
    };
    REPLIES.remove(ctx, &message);
    if !RECEIPTS.has(ctx, &key) {
        return Ok(());
    }
    let result = match outcome {
        Outcome::Applied { output } => Ok(output.clone()),
        Outcome::Rejected(error) => Err(::program::kernel::refusal_from(error.clone())),
    };
    settle(ctx, &key, &home(ctx)?, packet, result)
}
