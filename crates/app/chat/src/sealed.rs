//! Sealed rooms (J-377, J-389): the device keys chat keeps, a direct room between two people sealed
//! under an epoch key wrapped to each live device, and the ops a sealed room takes. chat checks the
//! bookkeeping and keeps opaque bytes; each device does the crypto (the `seal` crate).
use std::collections::{BTreeMap, BTreeSet};

use ::program::{
    Error, ExecCtx, QueryCtx, Scheme, already_exists, code, invalid, not_found, unauthorized,
    unexpected_reply, wrong_state,
};
use abi::role::identity as role;

use crate::state::{
    ACCOUNT_DEVICES, Body, DEVICE_COUNTS, DEVICES, Device, MESSAGE_IDS, ROOM_EPOCHS, SEALED_BODIES,
    WRAPS, WRAPS_BY_EPOCH, Wrapping, channel, message,
};
use crate::{
    AccountNumber, Block, DeviceKey, DeviceWrap, EpochWrap, Kind, Notice, Op, Principal, SealedRow,
    WrapRow, dm_peers, rules,
};

pub type DeviceId = [u8; 32];

pub const WRAPPED: usize = 80;

pub const DEVICE_WRAP_BYTES: usize = 32 + 4 + WRAPPED;

pub const KEYS_PER_ACCOUNT: u64 = (abi::BLOCK_BYTES / (2 * DEVICE_WRAP_BYTES) - 1) as u64;

enum Takes<'a> {
    Any,
    Emoji(&'a str),
    Never,
}

fn in_room(op: &Op) -> Option<(&str, Takes<'_>)> {
    let (channel_id, takes) = match op {
        Op::Seal { channel_id, .. }
        | Op::PostSealed { channel_id, .. }
        | Op::Rekey { channel_id, .. }
        | Op::Wrap { channel_id, .. }
        | Op::Unopenable { channel_id, .. }
        | Op::RemoveReaction { channel_id, .. }
        | Op::DeleteMessage { channel_id, .. }
        | Op::SetMembership { channel_id, .. }
        | Op::SetChannelArchived { channel_id, .. }
        | Op::Take { channel_id, .. }
        | Op::Release { channel_id, .. }
        | Op::Break { channel_id, .. }
        | Op::Undraft { channel_id }
        | Op::Listen { channel_id, .. }
        | Op::SetNotices { channel_id, .. }
        | Op::Watch { channel_id, .. }
        | Op::ChooseForMany { channel_id, .. } => (channel_id, Takes::Any),
        Op::AddReaction {
            channel_id, emoji, ..
        } => (channel_id, Takes::Emoji(emoji)),
        Op::PostMessage { channel_id, .. }
        | Op::Post { channel_id, .. }
        | Op::EditMessage { channel_id, .. }
        | Op::Notify { channel_id, .. }
        | Op::NotifyWith { channel_id, .. }
        | Op::Drafting { channel_id, .. }
        | Op::SetChannelMeta { channel_id, .. }
        | Op::RemoveChannelMeta { channel_id, .. }
        | Op::RenameChannel { channel_id, .. }
        | Op::SetPostPolicy { channel_id, .. }
        | Op::Link { channel_id, .. }
        | Op::FromPeer { channel_id, .. } => (channel_id, Takes::Never),
        Op::CreateChannel { .. }
        | Op::CreateChannelWithMeta { .. }
        | Op::CreateDmChannel { .. }
        | Op::AdoptOwners { .. }
        | Op::WatchPlaces { .. }
        | Op::PublishDeviceKey { .. }
        | Op::RetireDeviceKey { .. } => return None,
    };
    Some((channel_id.as_str(), takes))
}

pub(crate) fn admitted(ctx: &QueryCtx, op: &Op) -> Result<(), Error> {
    let Some((channel_id, takes)) = in_room(op) else {
        return Ok(());
    };
    if !is_sealed(ctx, channel_id) {
        return Ok(());
    }
    match takes {
        Takes::Any => Ok(()),
        Takes::Emoji(emoji) => match crate::emoji::fully_qualified(emoji) {
            true => Ok(()),
            false => Err(invalid(
                "a reaction in a sealed room is one fully-qualified emoji",
            )),
        },
        Takes::Never => Err(wrong_state(format!(
            "{channel_id} is sealed: it takes sealed posts and ops that carry no text"
        ))),
    }
}

pub(crate) fn is_sealed(ctx: &QueryCtx, channel_id: &str) -> bool {
    ROOM_EPOCHS.has(ctx, &channel_id.to_owned())
}

fn current(ctx: &QueryCtx, channel_id: &str) -> Result<u64, Error> {
    (ROOM_EPOCHS.get(ctx, &channel_id.to_owned())?)
        .ok_or_else(|| wrong_state(format!("{channel_id} is not sealed")))
}

fn hex(device: &DeviceId) -> String {
    abi::hex(device)
}

fn acts_as(ctx: &QueryCtx, signer: &[u8]) -> Result<Option<AccountNumber>, Error> {
    let identity = ctx.env().role(abi::role::IDENTITY)?.clone();
    let asked = role::Query::Account(signer.to_vec());
    match ctx.query::<role::Query, role::Reply>(&identity, &asked) {
        Ok(role::Reply::Account(account)) => Ok(account),
        Ok(other) => Err(unexpected_reply(&identity, "Account", &other)),
        Err(refused) if refused.code == code::UNAUTHORIZED => Ok(None),
        Err(fault) => Err(fault),
    }
}

fn person(ctx: &QueryCtx, account: AccountNumber) -> Result<(), Error> {
    let identity = ctx.env().role(abi::role::IDENTITY)?.clone();
    let asked = role::Query::Profile(account);
    let kind = match ctx.query::<role::Query, role::Reply>(&identity, &asked)? {
        role::Reply::Profile(profile) => profile.map(|profile| profile.kind),
        other => return Err(unexpected_reply(&identity, "Profile", &other)),
    };
    match kind {
        Some(Kind::User) => Ok(()),
        _ => Err(unauthorized(format!(
            "account {account} is no person: a conversation with an agent or a program runs on the network and is never sealed"
        ))),
    }
}

fn live_row(ctx: &QueryCtx, device: &DeviceId) -> Result<Option<Device>, Error> {
    let Some(row) = DEVICES.get(ctx, device)? else {
        return Ok(None);
    };
    if row.retired {
        return Ok(None);
    }
    Ok((acts_as(ctx, &row.signer)? == Some(row.account)).then_some(row))
}

fn live(ctx: &QueryCtx, device: &DeviceId) -> Result<Option<AccountNumber>, Error> {
    Ok(live_row(ctx, device)?.map(|row| row.account))
}

fn live_devices(ctx: &QueryCtx, account: AccountNumber) -> Result<Vec<(DeviceId, Device)>, Error> {
    let held = ACCOUNT_DEVICES.scan(ctx, ACCOUNT_DEVICES.prefix_of(&account))?;
    let mut live = Vec::new();
    for (_, device) in held {
        if let Some(row) = live_row(ctx, &device)? {
            live.push((device, row));
        }
    }
    Ok(live)
}

fn scheme_of(ctx: &QueryCtx, account: AccountNumber, signer: &[u8]) -> Result<Scheme, Error> {
    let registry = ctx.env().role(abi::role::IDENTITY)?.clone();
    let asked = ::identity::Query::Get { number: account };
    let held = match ctx.query::<::identity::Query, ::identity::Reply>(&registry, &asked)? {
        ::identity::Reply::Account(held) => held,
        other => return Err(unexpected_reply(&registry, "Account", &other)),
    };
    let key = held
        .iter()
        .flat_map(|held| held.keys())
        .find(|held| held.key == signer);
    key.map(|key| key.scheme).ok_or_else(|| {
        unauthorized(format!(
            "identity holds this frame's key on account {account} no more"
        ))
    })
}

pub(crate) fn publish(ctx: &ExecCtx, key: DeviceId, sig: Vec<u8>) -> Result<(), Error> {
    let signer = ctx.env().signer()?;
    let account = ctx.sender_account()?;
    if acts_as(ctx, &signer)? != Some(account) {
        return Err(unauthorized(
            "this frame's key does not act as your account in identity, so a device key it published would never be live",
        ));
    }
    person(ctx, account)?;
    let scheme = scheme_of(ctx, account, &signer)?;
    let program = &ctx.env().program;
    let statement = crate::device_statement(program, &ctx.env().chain_id, account, &key);
    if !ctx.verify(
        scheme,
        signer.clone(),
        crate::VIEW_SIGN,
        statement,
        sig.clone(),
    )? {
        return Err(invalid(format!(
            "sig does not sign device key {} for account {account} on this network with this frame's key, under {} as {}'s {}",
            hex(&key),
            crate::VIEW_SIGN,
            program,
            crate::DEVICE_PURPOSE
        )));
    }
    if let Some(held) = DEVICES.get(ctx, &key)? {
        let again = held.account == account && held.signer == signer && !held.retired;
        return match again {
            true => Ok(()),
            false => Err(already_exists(
                "this device key is published already: make a fresh one",
            )),
        };
    }
    let count = DEVICE_COUNTS.get(ctx, &account)?.unwrap_or(0);
    if count >= KEYS_PER_ACCOUNT {
        return Err(wrong_state(format!(
            "account {account} holds {count} device keys, the most whose wraps fit one frame beside the other member's: retire one first"
        )));
    }
    let device = Device {
        account,
        signer,
        scheme,
        sig,
        height: ctx.env().height,
        retired: false,
        framed_under: program.clone(),
    };
    DEVICES.put(ctx, &key, &device);
    ACCOUNT_DEVICES.insert(ctx, &(account, key));
    DEVICE_COUNTS.put(ctx, &account, &(count + 1));
    Ok(())
}

pub(crate) fn retire(ctx: &ExecCtx, key: DeviceId) -> Result<(), Error> {
    let account = ctx.sender_account()?;
    let Some(mut held) = DEVICES.get(ctx, &key)? else {
        return Err(not_found(format!("no device key {}", hex(&key))));
    };
    if held.account != account {
        return Err(unauthorized("the device key is another account's"));
    }
    if held.retired {
        return Ok(());
    }
    held.retired = true;
    DEVICES.put(ctx, &key, &held);
    ACCOUNT_DEVICES.remove(ctx, &(account, key));
    let left = DEVICE_COUNTS
        .get(ctx, &account)?
        .unwrap_or(0)
        .saturating_sub(1);
    match left {
        0 => DEVICE_COUNTS.remove(ctx, &account),
        left => DEVICE_COUNTS.put(ctx, &account, &left),
    }
    Ok(())
}

fn whole(device: &DeviceId, wrapped: &[u8]) -> Result<(), Error> {
    if wrapped.len() == WRAPPED {
        return Ok(());
    }
    Err(invalid(format!(
        "the wrap for device {} is {} bytes: a wrap is {WRAPPED}",
        hex(device),
        wrapped.len()
    )))
}

fn named_once(devices: impl IntoIterator<Item = DeviceId>) -> Result<BTreeSet<DeviceId>, Error> {
    let mut named = BTreeSet::new();
    for device in devices {
        if !named.insert(device) {
            return Err(invalid(format!("device {} is wrapped twice", hex(&device))));
        }
    }
    Ok(named)
}

fn covers(given: &BTreeSet<DeviceId>, want: &BTreeSet<DeviceId>) -> Result<(), Error> {
    if given == want {
        return Ok(());
    }
    let listed = |devices: Vec<&DeviceId>| {
        let named: Vec<String> = devices.into_iter().map(hex).collect();
        match named.is_empty() {
            true => "none".to_owned(),
            false => named.join(", "),
        }
    };
    Err(wrong_state(format!(
        "the wraps must cover exactly the live devices: missing {}; not to wrap {}",
        listed(want.difference(given).collect()),
        listed(given.difference(want).collect()),
    )))
}

fn put_wrap(
    ctx: &ExecCtx,
    channel_id: &str,
    epoch: u64,
    device: DeviceId,
    wrapped: Vec<u8>,
    account: AccountNumber,
) {
    let wrapping = Wrapping {
        wrapped,
        unopenable: false,
    };
    WRAPS.put(ctx, &(channel_id.to_owned(), device, epoch), &wrapping);
    WRAPS_BY_EPOCH.put(ctx, &(channel_id.to_owned(), epoch, device), &account);
}

fn members(channel_id: &str) -> Result<[AccountNumber; 2], Error> {
    match dm_peers(channel_id) {
        Some((a, b)) => Ok([a, b]),
        None => Err(invalid(format!(
            "{channel_id} is no direct room: only a direct room between two people is sealed"
        ))),
    }
}

pub(crate) fn seal(
    ctx: &ExecCtx,
    sender: &Principal,
    channel_id: &str,
    wraps: Vec<DeviceWrap>,
) -> Result<(), Error> {
    let room = channel(ctx, channel_id)?;
    let members = members(&room.id)?;
    rules::writable(ctx, &room, sender)?;
    if is_sealed(ctx, &room.id) {
        return Err(wrong_state(format!("{} is sealed already", room.id)));
    }
    let mut want = BTreeMap::new();
    for account in members {
        person(ctx, account)?;
        let devices = live_devices(ctx, account)?;
        if devices.is_empty() {
            return Err(wrong_state(format!(
                "account {account} has no live device key yet: sealing waits until they open chat on a device"
            )));
        }
        want.extend(devices.into_iter().map(|(device, _)| (device, account)));
    }
    for wrap in &wraps {
        whole(&wrap.device, &wrap.wrapped)?;
    }
    let given = named_once(wraps.iter().map(|wrap| wrap.device))?;
    covers(&given, &want.keys().copied().collect())?;
    for wrap in wraps {
        put_wrap(
            ctx,
            &room.id,
            0,
            wrap.device,
            wrap.wrapped,
            want[&wrap.device],
        );
    }
    ROOM_EPOCHS.put(ctx, &room.id, &0);
    Ok(())
}

fn stale(ctx: &QueryCtx, channel_id: &str, epoch: u64) -> Result<Vec<DeviceId>, Error> {
    let at = WRAPS_BY_EPOCH.prefix_of(&(channel_id.to_owned(), epoch));
    let mut stale = Vec::new();
    for ((_, _, device), account) in WRAPS_BY_EPOCH.scan(ctx, at)? {
        if live(ctx, &device)? != Some(account) {
            stale.push(device);
        }
    }
    Ok(stale)
}

fn holders(
    ctx: &QueryCtx,
    channel_id: &str,
    epoch: u64,
) -> Result<BTreeMap<DeviceId, AccountNumber>, Error> {
    let at = WRAPS_BY_EPOCH.prefix_of(&(channel_id.to_owned(), epoch));
    let mut holders = BTreeMap::new();
    for ((_, _, device), account) in WRAPS_BY_EPOCH.scan(ctx, at)? {
        if live(ctx, &device)? == Some(account) {
            holders.insert(device, account);
        }
    }
    Ok(holders)
}

pub(crate) fn post_sealed(
    ctx: &ExecCtx,
    sender: &Principal,
    channel_id: String,
    message_id: String,
    thread: Option<u64>,
    body: Body,
) -> Result<(), Error> {
    ctx.sender_account()?;
    let current = current(ctx, &channel_id)?;
    if body.epoch != current {
        return Err(wrong_state(format!(
            "{channel_id} is at epoch {current}, not {}: seal at the current epoch",
            body.epoch
        )));
    }
    let stale = stale(ctx, &channel_id, current)?;
    if let Some(device) = stale.first() {
        return Err(wrong_state(format!(
            "rekey first: device {} holds epoch {current} and is no longer live",
            hex(device)
        )));
    }
    let blocks = vec![Block::paragraph(crate::SEALED)];
    crate::ops::write(
        ctx,
        sender,
        channel_id.clone(),
        message_id.clone(),
        blocks,
        thread,
    )?;
    let Some((_, seq)) = MESSAGE_IDS.get(ctx, &message_id)? else {
        return Err(wrong_state(format!("{message_id} was not written")));
    };
    SEALED_BODIES.put(ctx, &(channel_id, seq), &body);
    Ok(())
}

pub(crate) fn rekey(
    ctx: &ExecCtx,
    sender: &Principal,
    channel_id: &str,
    epoch: u64,
    wraps: Vec<DeviceWrap>,
) -> Result<(), Error> {
    let room = channel(ctx, channel_id)?;
    rules::writable(ctx, &room, sender)?;
    let current = current(ctx, &room.id)?;
    if epoch != current + 1 {
        return Err(wrong_state(format!(
            "{} is at epoch {current}: a rekey installs epoch {}",
            room.id,
            current + 1
        )));
    }
    let holders = holders(ctx, &room.id, current)?;
    for wrap in &wraps {
        whole(&wrap.device, &wrap.wrapped)?;
    }
    let given = named_once(wraps.iter().map(|wrap| wrap.device))?;
    covers(&given, &holders.keys().copied().collect())?;
    for wrap in wraps {
        let account = holders[&wrap.device];
        put_wrap(ctx, &room.id, epoch, wrap.device, wrap.wrapped, account);
    }
    ROOM_EPOCHS.put(ctx, &room.id, &epoch);
    Ok(())
}

fn holds_good(
    ctx: &QueryCtx,
    channel_id: &str,
    epoch: u64,
    account: AccountNumber,
) -> Result<bool, Error> {
    for (device, holder) in holders(ctx, channel_id, epoch)? {
        if holder != account {
            continue;
        }
        let wrapping = WRAPS.get(ctx, &(channel_id.to_owned(), device, epoch))?;
        if wrapping.is_some_and(|wrapping| !wrapping.unopenable) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn wrapped_before(ctx: &QueryCtx, channel_id: &str, device: &DeviceId) -> Result<bool, Error> {
    let at = WRAPS.prefix_of(&(channel_id.to_owned(), *device)).limit(1);
    Ok(!WRAPS.scan(ctx, at)?.is_empty())
}

pub(crate) fn wrap(
    ctx: &ExecCtx,
    sender: &Principal,
    channel_id: &str,
    wraps: Vec<EpochWrap>,
) -> Result<(), Error> {
    let room = channel(ctx, channel_id)?;
    let members = members(&room.id)?;
    rules::writable(ctx, &room, sender)?;
    let current = current(ctx, &room.id)?;
    let me = ctx.sender_account()?;
    let mut named = BTreeSet::new();
    let mut owners = BTreeMap::new();
    for wrap in &wraps {
        if !named.insert((wrap.epoch, wrap.device)) {
            return Err(invalid(format!(
                "device {} is wrapped twice at epoch {}",
                hex(&wrap.device),
                wrap.epoch
            )));
        }
        whole(&wrap.device, &wrap.wrapped)?;
        if wrap.epoch > current {
            return Err(invalid(format!(
                "{} is at epoch {current}: it has no epoch {}",
                room.id, wrap.epoch
            )));
        }
        let Some(account) = live(ctx, &wrap.device)? else {
            return Err(wrong_state(format!(
                "device {} is not live",
                hex(&wrap.device)
            )));
        };
        if !members.contains(&account) {
            return Err(unauthorized(format!(
                "device {} is not a member's",
                hex(&wrap.device)
            )));
        }
        owners.insert(wrap.device, account);
    }
    for account in owners.values().copied().collect::<BTreeSet<_>>() {
        let blocked = account != me && holds_good(ctx, &room.id, current, account)?;
        if blocked {
            return Err(unauthorized(format!(
                "account {account} still has a live device holding the current key: one of its own devices wraps to its new one"
            )));
        }
    }
    let mut first = Vec::new();
    for (device, account) in &owners {
        if !wrapped_before(ctx, &room.id, device)? {
            first.push((*device, *account));
        }
    }
    for wrap in wraps {
        let account = owners[&wrap.device];
        put_wrap(
            ctx,
            &room.id,
            wrap.epoch,
            wrap.device,
            wrap.wrapped,
            account,
        );
    }
    for (device, account) in first {
        added(ctx, sender, &room.id, account, &device, members)?;
    }
    Ok(())
}

fn added(
    ctx: &ExecCtx,
    sender: &Principal,
    channel_id: &str,
    account: AccountNumber,
    device: &DeviceId,
    members: [AccountNumber; 2],
) -> Result<(), Error> {
    let id = crate::namespace::id(
        crate::PROGRAM,
        &format!("device:{channel_id}:{}", hex(device)),
    );
    let notice = Notice {
        kind: "device".into(),
        group: hex(device),
        subject: "a device was added".into(),
        link: None,
        status: crate::message::Status::Note,
        detail: String::new(),
        concerns: members.to_vec(),
    };
    let said = crate::parse_message(&format!(
        "<@{account}> added a device; it can read this conversation's history"
    ));
    let blocks = std::iter::once(Block::Notice(notice)).chain(said).collect();
    crate::ops::store(ctx, sender, channel_id.to_owned(), id, blocks, None)
}

pub(crate) fn unopenable(
    ctx: &ExecCtx,
    channel_id: &str,
    epoch: u64,
    device: DeviceId,
) -> Result<(), Error> {
    let signer = ctx.env().signer()?;
    let Some(row) = DEVICES.get(ctx, &device)? else {
        return Err(not_found(format!("no device key {}", hex(&device))));
    };
    if row.signer != signer {
        return Err(unauthorized(
            "only the key that published a device marks its wrap",
        ));
    }
    let key = (channel_id.to_owned(), device, epoch);
    let Some(mut wrapping) = WRAPS.get(ctx, &key)? else {
        return Err(not_found(format!(
            "{channel_id} holds no wrap for device {} at epoch {epoch}",
            hex(&device)
        )));
    };
    wrapping.unopenable = true;
    WRAPS.put(ctx, &key, &wrapping);
    Ok(())
}

pub(crate) fn device_keys(
    ctx: &QueryCtx,
    accounts: &[AccountNumber],
) -> Result<Vec<DeviceKey>, Error> {
    let mut keys = Vec::new();
    for &account in accounts {
        for (key, row) in live_devices(ctx, account)? {
            keys.push(DeviceKey {
                account,
                key,
                height: row.height,
                signer: row.signer,
                scheme: row.scheme,
                sig: row.sig,
                framed_under: row.framed_under,
            });
        }
    }
    Ok(keys)
}

pub(crate) fn sealing(ctx: &QueryCtx, channel_ids: &[String]) -> Result<Vec<(String, u64)>, Error> {
    let mut sealed = Vec::new();
    for id in channel_ids {
        if let Some(epoch) = ROOM_EPOCHS.get(ctx, id)? {
            sealed.push((id.clone(), epoch));
        }
    }
    Ok(sealed)
}

pub(crate) fn wraps(
    ctx: &QueryCtx,
    channel_id: &str,
    devices: &[DeviceId],
) -> Result<Vec<WrapRow>, Error> {
    let mut rows = Vec::new();
    for device in devices {
        let at = WRAPS.prefix_of(&(channel_id.to_owned(), *device));
        for ((_, device, epoch), wrapping) in WRAPS.scan(ctx, at)? {
            rows.push(WrapRow {
                device,
                epoch,
                wrapped: wrapping.wrapped,
                unopenable: wrapping.unopenable,
            });
        }
    }
    Ok(rows)
}

pub(crate) fn sealed(
    ctx: &QueryCtx,
    channel_id: &str,
    seqs: &[u64],
) -> Result<Vec<SealedRow>, Error> {
    let mut rows = Vec::new();
    for &seq in seqs {
        let Some(body) = SEALED_BODIES.get(ctx, &(channel_id.to_owned(), seq))? else {
            continue;
        };
        let row = message(ctx, channel_id, seq)?;
        rows.push(SealedRow {
            seq,
            message_id: row.message_id,
            author: row.author,
            epoch: body.epoch,
            nonce: body.nonce,
            ct: body.ct,
        });
    }
    Ok(rows)
}
