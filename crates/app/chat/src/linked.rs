//! A channel linked to its peer on another network: what an account of this network writes there
//! is sent to the peer through crosspond, what the peer sends runs here as its author, and a user
//! of another network is named by what crosspond heard.
use ::program::{
    Error, ExecCtx, Need, ProgramId, QueryCtx, Reply as Answer, not_found, unauthorized,
    unexpected_reply, wrong_state,
};
use abi::role::crosspond as role;

use crate::state::{FOREIGNERS, MESSAGE_IDS, PEERS, channel, message};
use crate::{
    AccountNumber, Block, Foreign, ForeignName, Mark, MsgRow, NetworkId, Notice, Op, PageRequest,
    PageResponse, Peer, PeerMessage, PeerWrite, Principal, Span, message_key, rules,
};

pub fn copy_id(network: &NetworkId, message_id: &str) -> String {
    let id = format!("{}:{message_id}", abi::hex(network));
    crate::namespace::id(crate::PROGRAM, &id)
}

pub fn copied(message_id: &str) -> Option<(NetworkId, &str)> {
    let (program, rest) = message_id.split_once(':')?;
    if program != crate::PROGRAM {
        return None;
    }
    let (network, id) = rest.split_once(':')?;
    Some((network_id(network)?, id))
}

fn network_id(hex: &str) -> Option<NetworkId> {
    let mut id = [0u8; 32];
    let digits = hex.as_bytes();
    if digits.len() != 2 * id.len() {
        return None;
    }
    for (byte, pair) in id.iter_mut().zip(digits.chunks(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    (abi::hex(&id) == hex).then_some(id)
}

fn crosspond(ctx: &QueryCtx) -> Option<ProgramId> {
    ctx.env().role(abi::role::CROSSPOND).ok().cloned()
}

fn ask(ctx: &QueryCtx, crosspond: &ProgramId, query: &role::Query) -> Result<role::Reply, Error> {
    ctx.query::<role::Query, role::Reply>(crosspond, query)
}

fn home(ctx: &QueryCtx, crosspond: &ProgramId) -> Result<NetworkId, Error> {
    match ask(ctx, crosspond, &role::Query::Home)? {
        role::Reply::Home(home) => Ok(home),
        other => Err(unexpected_reply(crosspond, "Home", &other)),
    }
}

fn link_of(
    ctx: &QueryCtx,
    crosspond: &ProgramId,
    network: NetworkId,
) -> Result<Option<role::Link>, Error> {
    match ask(ctx, crosspond, &role::Query::Link { network })? {
        role::Reply::Link(link) => Ok(link),
        other => Err(unexpected_reply(crosspond, "Link", &other)),
    }
}

fn only<T: Copy>(held: &[T]) -> Option<T> {
    match held {
        [one] => Some(*one),
        _ => None,
    }
}

pub(crate) fn network_named(ctx: &QueryCtx, name: &str) -> Result<Option<NetworkId>, Error> {
    let Some(crosspond) = crosspond(ctx) else {
        return Ok(None);
    };
    let asked = role::Query::Linked { name: name.into() };
    match ask(ctx, &crosspond, &asked)? {
        role::Reply::Linked(networks) => Ok(only(&networks)),
        other => Err(unexpected_reply(&crosspond, "Linked", &other)),
    }
}

pub(crate) fn account_named(
    ctx: &QueryCtx,
    network: NetworkId,
    name: &str,
) -> Result<Option<AccountNumber>, Error> {
    let Some(crosspond) = crosspond(ctx) else {
        return Ok(None);
    };
    let asked = role::Query::Named {
        network,
        name: name.into(),
    };
    match ask(ctx, &crosspond, &asked)? {
        role::Reply::Named(accounts) => Ok(only(&accounts)),
        other => Err(unexpected_reply(&crosspond, "Named", &other)),
    }
}

pub fn foreign_name(ctx: &QueryCtx, foreign: Foreign) -> Result<ForeignName, Error> {
    let Some(crosspond) = crosspond(ctx) else {
        return Ok(ForeignName {
            foreign,
            name: None,
            network: None,
        });
    };
    let name = match ask(ctx, &crosspond, &role::Query::Name(foreign.into()))? {
        role::Reply::Name(name) => name,
        other => return Err(unexpected_reply(&crosspond, "Name", &other)),
    };
    let network = link_of(ctx, &crosspond, foreign.network)?.map(|link| link.name);
    Ok(ForeignName {
        foreign,
        name,
        network,
    })
}

pub(crate) fn foreigners(
    ctx: &QueryCtx,
    page: &PageRequest,
) -> Result<PageResponse<ForeignName>, Error> {
    let held = FOREIGNERS.range(ctx, page)?;
    let items = (held.items.into_iter())
        .map(|(network, account)| foreign_name(ctx, Foreign { network, account }))
        .collect::<Result<_, _>>()?;
    Ok(PageResponse {
        height: held.height,
        items,
        next: held.next,
    })
}

pub(crate) fn heard(ctx: &ExecCtx, principal: &Principal) {
    let Principal::Foreign(foreign) = principal else {
        return;
    };
    let key = (foreign.network, foreign.account);
    if !FOREIGNERS.has(ctx, &key) {
        FOREIGNERS.insert(ctx, &key);
    }
}

fn mentions(blocks: &[Block]) -> impl Iterator<Item = &Principal> {
    let spans = blocks.iter().flat_map(|block| match block {
        Block::Paragraph(spans) | Block::Quote(spans) => spans.as_slice(),
        Block::Code { .. } | Block::Divider | Block::Attachment(_) | Block::Notice(_) => &[],
    });
    (spans.flat_map(|span| &span.marks)).filter_map(|mark| match mark {
        Mark::Mention(principal) => Some(principal),
        _ => None,
    })
}

pub(crate) fn mentioned(ctx: &ExecCtx, blocks: &[Block]) {
    for principal in mentions(blocks) {
        heard(ctx, principal);
    }
}

pub(crate) fn admitted(ctx: &QueryCtx, sender: &Principal, op: &Op) -> Result<(), Error> {
    match (sender, op) {
        (Principal::Foreign(_), Op::FromPeer { .. }) => Ok(()),
        (Principal::Foreign(_), _) => Err(unauthorized(
            "a user of another network writes here only with the FromPeer a linked channel's peer sends",
        )),
        (
            _,
            Op::SetMembership {
                channel_id,
                principal: Principal::Foreign(_),
                member: true,
            },
        ) => confined(ctx, channel_id),
        _ => Ok(()),
    }
}

pub(crate) fn names_foreign_where_linked(
    ctx: &QueryCtx,
    channel_id: &str,
    blocks: &[Block],
) -> Result<(), Error> {
    let foreign = mentions(blocks).any(|principal| matches!(principal, Principal::Foreign(_)));
    match foreign {
        true => confined(ctx, channel_id),
        false => Ok(()),
    }
}

fn confined(ctx: &QueryCtx, channel_id: &str) -> Result<(), Error> {
    match PEERS.has(ctx, &channel_id.to_owned()) {
        true => Ok(()),
        false => Err(unauthorized(format!(
            "#{channel_id} is linked to no other network: a user of another network is named only in a linked channel"
        ))),
    }
}

pub(crate) fn link(ctx: &ExecCtx, channel_id: &str, peer: Option<Peer>) -> Result<(), Error> {
    let room = channel(ctx, channel_id)?;
    rules::not_dm(&room)?;
    if let Some(peer) = &peer {
        rules::id("the peer's channel_id", &peer.channel_id)?;
        let crosspond = ctx.env().role(abi::role::CROSSPOND)?.clone();
        let Some(link) = link_of(ctx, &crosspond, peer.network)? else {
            return Err(not_found(format!(
                "crosspond links no network {}: link the channel to a network crosspond is open to",
                abi::hex(&peer.network)
            )));
        };
        if link.closed {
            return Err(wrong_state(format!(
                "crosspond's link to {} is closed: nothing more is sent there",
                link.name
            )));
        }
    }
    crate::ops::touch_channel_for_policy(ctx, &room);
    match peer {
        Some(peer) => PEERS.put(ctx, &room.id, &peer),
        None => PEERS.remove(ctx, &room.id),
    }
    Ok(())
}

pub(crate) fn peer(ctx: &QueryCtx, channel_id: &str) -> Result<Option<Peer>, Error> {
    PEERS.get(ctx, &channel_id.to_owned())
}

struct Sending {
    crosspond: ProgramId,
    peer: Peer,
    home: NetworkId,
    here: String,
    account: AccountNumber,
}

fn sending(ctx: &ExecCtx, sender: &Principal, channel_id: &str) -> Result<Option<Sending>, Error> {
    let Principal::Account(account) = sender else {
        return Ok(None);
    };
    let Some(peer) = PEERS.get(ctx, &channel_id.to_owned())? else {
        return Ok(None);
    };
    let crosspond = ctx.env().role(abi::role::CROSSPOND)?.clone();
    let home = home(ctx, &crosspond)?;
    Ok(Some(Sending {
        crosspond,
        peer,
        home,
        here: String::from_utf8_lossy(&ctx.env().chain_id).into_owned(),
        account: *account,
    }))
}

impl Sending {
    fn named(&self, row: &MsgRow) -> Option<PeerMessage> {
        match copied(&row.message_id) {
            Some((network, id)) if network == self.peer.network => {
                Some(PeerMessage::Here(id.to_owned()))
            }
            Some(_) => None,
            None => Some(PeerMessage::There(row.message_id.clone())),
        }
    }

    fn blocks(&self, blocks: &[Block]) -> Vec<Block> {
        let spans = |spans: &[Span]| spans.iter().map(|span| self.span(span)).collect();
        (blocks.iter())
            .map(|block| match block {
                Block::Paragraph(held) => Block::Paragraph(spans(held)),
                Block::Quote(held) => Block::Quote(spans(held)),
                Block::Code { .. } | Block::Divider | Block::Attachment(_) | Block::Notice(_) => {
                    block.clone()
                }
            })
            .collect()
    }

    fn span(&self, span: &Span) -> Span {
        let mentioned = span.marks.iter().find_map(|mark| match mark {
            Mark::Mention(principal) => Some(principal),
            _ => None,
        });
        let (principal, text) = match mentioned {
            Some(Principal::Account(number)) => {
                let foreign = Foreign {
                    network: self.home,
                    account: *number,
                };
                let text = format!("@{number}@{}", self.here);
                (Principal::Foreign(foreign), text)
            }
            Some(Principal::Foreign(foreign)) if foreign.network == self.peer.network => {
                let text = format!("<@{}>", foreign.account);
                (Principal::Account(foreign.account), text)
            }
            Some(Principal::Foreign(_) | Principal::Root) | None => return span.clone(),
        };
        let marks = (span.marks.iter())
            .map(|mark| match mark {
                Mark::Mention(_) => Mark::Mention(principal.clone()),
                other => other.clone(),
            })
            .collect();
        Span { text, marks }
    }

    fn send(self, ctx: &ExecCtx, write: PeerWrite) {
        let op = Op::FromPeer {
            channel_id: self.peer.channel_id,
            write,
        };
        let send = role::Op::SendFor {
            to: self.peer.network,
            port: crate::ROLE.into(),
            op: abi::encode(&op),
            deadline: None,
            account: self.account,
        };
        ctx.emit(self.crosspond, &send, Answer::None);
    }
}

pub(crate) fn posted(ctx: &ExecCtx, sender: &Principal, row: &MsgRow) -> Result<(), Error> {
    let Some(sending) = sending(ctx, sender, &row.channel_id)? else {
        return Ok(());
    };
    let thread = match row.thread {
        Some(root) => match sending.named(&message(ctx, &row.channel_id, root)?) {
            Some(root) => Some(root),
            None => return Ok(()),
        },
        None => None,
    };
    let message_id = row.message_id.clone();
    let write = match row.notice() {
        Some(notice) => PeerWrite::Notify {
            message_id,
            thread,
            notice: Notice {
                concerns: Vec::new(),
                ..notice.clone()
            },
        },
        None => PeerWrite::Post {
            message_id,
            blocks: sending.blocks(&row.blocks),
            thread,
        },
    };
    sending.send(ctx, write);
    Ok(())
}

fn about(
    ctx: &ExecCtx,
    sender: &Principal,
    row: &MsgRow,
    write: impl FnOnce(&Sending, PeerMessage) -> PeerWrite,
) -> Result<(), Error> {
    let Some(sending) = sending(ctx, sender, &row.channel_id)? else {
        return Ok(());
    };
    let Some(message) = sending.named(row) else {
        return Ok(());
    };
    let write = write(&sending, message);
    sending.send(ctx, write);
    Ok(())
}

pub(crate) fn edited(ctx: &ExecCtx, sender: &Principal, row: &MsgRow) -> Result<(), Error> {
    about(ctx, sender, row, |sending, message| PeerWrite::Edit {
        message,
        blocks: sending.blocks(&row.blocks),
        base_rev: row.base_rev,
    })
}

pub(crate) fn deleted(ctx: &ExecCtx, sender: &Principal, row: &MsgRow) -> Result<(), Error> {
    about(ctx, sender, row, |_, message| PeerWrite::Delete { message })
}

pub(crate) fn reacted(
    ctx: &ExecCtx,
    sender: &Principal,
    row: &MsgRow,
    emoji: &str,
    on: bool,
) -> Result<(), Error> {
    about(ctx, sender, row, |_, message| PeerWrite::React {
        message,
        emoji: emoji.to_owned(),
        on,
    })
}

fn sent_from(ctx: &QueryCtx, sender: &Principal, channel_id: &str) -> Result<NetworkId, Error> {
    let network = match sender {
        Principal::Foreign(foreign) => foreign.network,
        Principal::Account(_) | Principal::Root => {
            return Err(unauthorized(
                "FromPeer is a linked channel's write from another network, which crosspond sends: write here with PostMessage, EditMessage, DeleteMessage or AddReaction",
            ));
        }
    };
    let linked = PEERS.get(ctx, &channel_id.to_owned())?;
    match linked {
        Some(peer) if peer.network == network => Ok(network),
        Some(_) | None => Err(unauthorized(format!(
            "#{channel_id} is not linked to network {}: only the network its owner linked it to writes there with FromPeer",
            abi::hex(&network)
        ))),
    }
}

fn seq_of(
    ctx: &QueryCtx,
    from: &NetworkId,
    channel_id: &str,
    named: &PeerMessage,
) -> Result<Option<u64>, Error> {
    let id = match named {
        PeerMessage::Here(id) => id.clone(),
        PeerMessage::There(id) => copy_id(from, id),
    };
    Ok(match MESSAGE_IDS.get(ctx, &id)? {
        Some((channel, seq)) if channel == channel_id => Some(seq),
        Some(_) | None => None,
    })
}

fn found(
    ctx: &QueryCtx,
    from: &NetworkId,
    channel_id: &str,
    named: &PeerMessage,
) -> Result<u64, Error> {
    seq_of(ctx, from, channel_id, named)?
        .ok_or_else(|| not_found(format!("{channel_id} holds no message {named:?}")))
}

pub(crate) fn needs(
    ctx: &QueryCtx,
    channel_id: &str,
    write: &PeerWrite,
) -> Result<Vec<Need>, Error> {
    let Some(sender) = &ctx.env().sender else {
        return Ok(Vec::new());
    };
    let from = sent_from(ctx, sender, channel_id)?;
    let (named, deletes) = match write {
        PeerWrite::Edit { message, .. } => (message, false),
        PeerWrite::Delete { message } => (message, true),
        PeerWrite::Post { .. } | PeerWrite::Notify { .. } | PeerWrite::React { .. } => {
            return Ok(Vec::new());
        }
    };
    let Some(seq) = seq_of(ctx, &from, channel_id, named)? else {
        return Ok(Vec::new());
    };
    let key = message_key(channel_id, seq);
    Ok(vec![match deletes {
        true => Need::delete(key),
        false => Need::write(key),
    }])
}

pub(crate) fn from_peer(
    ctx: &ExecCtx,
    sender: &Principal,
    channel_id: String,
    write: PeerWrite,
) -> Result<(), Error> {
    let from = sent_from(ctx, sender, &channel_id)?;
    let thread_of = |thread: Option<PeerMessage>| {
        (thread.map(|root| found(ctx, &from, &channel_id, &root))).transpose()
    };
    match write {
        PeerWrite::Post {
            message_id,
            blocks,
            thread,
        } => {
            rules::id("message_id", &message_id)?;
            rules::no_notice(&blocks)?;
            let thread = thread_of(thread)?;
            let id = copy_id(&from, &message_id);
            crate::ops::store(ctx, sender, channel_id, id, blocks, thread)
        }
        PeerWrite::Notify {
            message_id,
            thread,
            notice,
        } => {
            rules::id("message_id", &message_id)?;
            rules::notice(ctx, &notice)?;
            let thread = thread_of(thread)?;
            let id = copy_id(&from, &message_id);
            let blocks = vec![Block::Notice(notice)];
            crate::ops::store(ctx, sender, channel_id, id, blocks, thread)
        }
        PeerWrite::Edit {
            message,
            blocks,
            base_rev,
        } => {
            let seq = found(ctx, &from, &channel_id, &message)?;
            crate::ops::edit(ctx, sender, &channel_id, seq, blocks, base_rev)
        }
        PeerWrite::Delete { message } => {
            let seq = found(ctx, &from, &channel_id, &message)?;
            crate::ops::delete(ctx, sender, &channel_id, seq)
        }
        PeerWrite::React { message, emoji, on } => {
            let seq = found(ctx, &from, &channel_id, &message)?;
            crate::ops::react(ctx, sender, &channel_id, seq, &emoji, on)
        }
    }
}
