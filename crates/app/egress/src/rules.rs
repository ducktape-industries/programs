//! What each op checks and writes: a service its owner registers and assigns (the access list on
//! `services/<name>`), a member node serving, a program's call and the elected node's answer (whole, or as events in `seq` order, then the stream's end) or expiry.
use ::program::kernel::error_from;
use ::program::{
    BlobId, Cause, Ctx, Error, ExecCtx, Need, Origin, Ownership, Party, QueryCtx, Reply, Resource,
    code, invalid, not_found, unauthorized, unexpected_reply, wrong_state,
};
use abi::role::validators;
use store::{PageRequest, PageResponse};

use crate::state::{
    ASSIGNED, COUNT, EARLY, EXPIRIES, Ending, NODES, REQUESTS, RETIRED, SENDERS, SERVICES, SERVING,
    STREAMS, Stored, Stream, TURNS,
};
use crate::{
    AccountNumber, Answer, DEFAULT_WAIT, Delivery, Event, HTTP, NodeRow, Op, PROGRAM, RELAY,
    RESERVED, Request, RequestRow, ServiceRow, StreamEnd, http,
};

fn members(ctx: &QueryCtx) -> Result<Vec<Vec<u8>>, Error> {
    let validators = &ctx.env().role(abi::role::VALIDATORS)?.clone();
    match ctx
        .query::<validators::Query, validators::Reply>(validators, &validators::Query::Members)?
    {
        validators::Reply::Members(members) => {
            Ok(members.into_iter().map(|member| member.key).collect())
        }
        other => Err(unexpected_reply(validators, "Members", &other)),
    }
}

fn service_name(service: &str) -> Result<(), Error> {
    let named = !service.is_empty()
        && service.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_.".contains(&byte)
        });
    if !named {
        return Err(invalid(
            "a service is lowercase letters, digits, -, _ and .",
        ));
    }
    Ok(())
}

fn registered(ctx: &QueryCtx, name: &str) -> Result<ServiceRow, Error> {
    SERVICES
        .get(ctx, &name.to_owned())?
        .ok_or_else(|| not_found(format!("no service named {name} is registered")))
}

pub(crate) fn register(ctx: &ExecCtx, name: String) -> Result<(), Error> {
    let retired = RETIRED.get(ctx, &name)?;
    let voted = ctx.env().authority().is_ok();
    let owner = match (retired, voted) {
        (Some(owner), true) => owner,
        _ => ctx.sender_account()?,
    };
    let taken = retired.is_some_and(|previous| previous != owner);
    if taken {
        return Err(unauthorized(format!(
            "service {name} stays reserved to its last owner: only that owner or the validators' vote may register it again"
        )));
    }
    service_name(&name)?;
    if RESERVED.contains(&name.as_str()) {
        return Err(invalid(format!(
            "{name} is egress's own service, never registered: use the built-in service without registering it"
        )));
    }
    if SERVICES.has(ctx, &name) {
        return Err(wrong_state(format!("service {name} is registered")));
    }
    let row = ServiceRow {
        name: name.clone(),
        owner,
        nodes: Vec::new(),
        registered: ctx.env().height,
    };
    SERVICES.put(ctx, &name, &row);
    RETIRED.remove(ctx, &name);
    Ok(())
}

pub(crate) fn unregister(ctx: &ExecCtx, name: String) -> Result<(), Error> {
    let row = registered(ctx, &name)?;
    if !row.nodes.is_empty() {
        return Err(wrong_state(format!(
            "service {name} has nodes assigned: assign it no nodes first"
        )));
    }
    RETIRED.put(ctx, &name, &row.owner);
    SERVICES.remove(ctx, &name);
    Ok(())
}

pub(crate) fn migrate(
    ctx: &ExecCtx,
    height: u64,
    first: Option<u64>,
    last: Option<u64>,
) -> Result<(), Error> {
    let renamed = ctx.renames(height)?;
    let mut range = REQUESTS.prefix_of(&());
    if let Some(first) = first {
        range.start = REQUESTS.key(&first);
    }
    if let Some(last) = last {
        range.end = Some([REQUESTS.key(&last), vec![0]].concat());
    }
    for (id, mut stored) in REQUESTS.scan(ctx, range)? {
        let Some(to) = renamed.moved(&stored.row.requester) else {
            continue;
        };
        stored.row.requester = to.clone();
        REQUESTS.put(ctx, &id, &stored);
    }
    Ok(())
}

pub fn service_key(name: &str) -> String {
    format!("services/{name}")
}

pub(crate) fn needs(ctx: &QueryCtx, op: &Op) -> Result<Vec<Need>, Error> {
    if let Op::Unregister { service } = op {
        registered(ctx, service)?;
    }
    Ok(match op {
        Op::Assign { service, .. } | Op::Unregister { service } => {
            vec![Need::write(service_key(service))]
        }
        Op::Register { .. }
        | Op::Serve { .. }
        | Op::Request { .. }
        | Op::Answer { .. }
        | Op::Events(_)
        | Op::EndStream { .. }
        | Op::Expire { .. }
        | Op::Migrate { .. } => Vec::new(),
    })
}

pub(crate) fn resource(ctx: &QueryCtx, key: &str) -> Result<Resource, Error> {
    let Some(name) = key.strip_prefix("services/") else {
        return Ok(Resource::default());
    };
    let owner = SERVICES.get(ctx, &name.to_owned())?.map(|row| row.owner);
    Ok(Resource {
        owner: owner.map(Party::Account),
        default: Some(Ownership::Owner),
    })
}

pub(crate) fn assign(ctx: &ExecCtx, name: String, mut nodes: Vec<Vec<u8>>) -> Result<(), Error> {
    let mut row = registered(ctx, &name)?;
    nodes.sort();
    nodes.dedup();
    row.nodes = nodes;
    SERVICES.put(ctx, &name, &row);
    Ok(())
}

pub(crate) fn serve(ctx: &ExecCtx, mut services: Vec<String>) -> Result<(), Error> {
    let key = ctx.env().signer()?;
    services.sort();
    services.dedup();
    for service in &services {
        service_name(service)?;
    }
    let stopping = services.is_empty();
    if !stopping && !members(ctx)?.contains(&key) {
        return Err(unauthorized(
            "only a member node of this network serves: enroll the serving node in this network",
        ));
    }
    let since = match NODES.get(ctx, &key)? {
        Some(previous) => {
            for service in previous.services {
                SERVING.remove(ctx, &(service, key.clone()));
            }
            previous.since
        }
        None => ctx.env().height,
    };
    if stopping {
        NODES.remove(ctx, &key);
        return Ok(());
    }
    for service in &services {
        SERVING.insert(ctx, &(service.clone(), key.clone()));
    }
    NODES.put(
        ctx,
        &key,
        &NodeRow {
            key: key.clone(),
            services,
            since,
        },
    );
    Ok(())
}

fn check_parts(ctx: &QueryCtx, parts: &[BlobId]) -> Result<(), Error> {
    let missing = parts.iter().find(|part| ctx.blob_stat(**part).is_none());
    match missing {
        Some(part) => Err(not_found(format!(
            "part {} is not on the network: put it first",
            abi::hex(part.digest())
        ))),
        None => Ok(()),
    }
}

fn check(ctx: &QueryCtx, request: &Request) -> Result<(), Error> {
    let Request::Http {
        service,
        head,
        body,
    } = request
    else {
        return Ok(());
    };
    match service.as_str() {
        HTTP => head.check_url().map_err(error_from)?,
        RELAY => return Err(invalid("relay is asked with Request::Relay")),
        named => {
            registered(ctx, named)?;
            head.check_path().map_err(error_from)?;
        }
    }
    check_parts(ctx, body)
}

pub(crate) fn serving(ctx: &QueryCtx, service: &str) -> Result<Vec<Vec<u8>>, Error> {
    let members = members(ctx)?;
    let reserved = RESERVED.contains(&service);
    let assigned = match reserved {
        true => None,
        false => Some(
            SERVICES
                .get(ctx, &service.to_owned())?
                .map(|row| row.nodes)
                .unwrap_or_default(),
        ),
    };
    let may_serve = |key: &Vec<u8>| {
        let member = members.contains(key);
        let allowed = assigned.as_ref().is_none_or(|nodes| nodes.contains(key));
        member && allowed
    };
    Ok(SERVING
        .scan(ctx, SERVING.prefix_of(&service.to_owned()))?
        .into_iter()
        .map(|(_, key)| key)
        .filter(may_serve)
        .collect())
}

fn elect(ctx: &ExecCtx, service: &str) -> Result<Vec<u8>, Error> {
    let eligible = serving(ctx, service)?;
    if eligible.is_empty() {
        return Err(not_found(format!("no member node serves {service}")));
    }
    let turn = TURNS.get(ctx, &service.to_owned())?.unwrap_or(0);
    TURNS.put(ctx, &service.to_owned(), &(turn + 1));
    Ok(eligible[(turn % eligible.len() as u64) as usize].clone())
}

fn expiry_height(made: u64, wait: u64) -> Result<u64, Error> {
    if wait == 0 {
        return Err(invalid("a request waits at least one block"));
    }
    made.checked_add(wait).ok_or_else(|| {
        invalid(format!(
            "a wait of {wait} blocks from height {made} passes the last height: reduce the wait to fit the remaining heights"
        ))
    })
}

pub(crate) async fn request(
    ctx: &Ctx,
    request: Request,
    delivery: Delivery,
    wait: Option<u64>,
) -> Result<(), Error> {
    let requester = ctx.env().sending_program()?;
    let called = matches!(ctx.env().cause, Cause::Call { .. });
    if !called {
        return Err(wrong_state(
            "a request is a call: the program awaits egress with ctx.call",
        ));
    }
    let unscheduled = ctx.env().roles.get(abi::role::SCHEDULER).is_none();
    if unscheduled {
        return Err(Error::new(
            code::UNSUPPORTED,
            "egress expires a request through the scheduler role, and this network binds no \
             scheduler: bind a scheduler before sending the request",
        ));
    }
    let made = ctx.env().height;
    let wait = wait.unwrap_or(DEFAULT_WAIT);
    let expires = expiry_height(made, wait)?;
    check(ctx, &request)?;
    let streams_a_relay = matches!(
        (&request, &delivery),
        (Request::Relay { .. }, Delivery::Events { .. })
    );
    if streams_a_relay {
        return Err(invalid(
            "a relay is answered whole: ask it with Delivery::Whole",
        ));
    }
    let node = elect(ctx, request.service())?;
    let id = COUNT.get(ctx)?.unwrap_or(0) + 1;
    COUNT.put(ctx, &id);
    let expiry = ctx.schedule(expires, PROGRAM, &Op::Expire { id }).await?;
    let ticket = ctx.defer()?;
    let row = RequestRow {
        id,
        requester,
        request,
        delivery,
        node,
        made,
        expiry,
    };
    ASSIGNED.insert(ctx, &(row.node.clone(), id));
    REQUESTS.put(ctx, &id, &Stored { row, ticket, wait });
    if let Some(account) = ctx.env().sender.clone().and_then(|sender| sender.account()) {
        SENDERS.put(ctx, &id, &account);
    }
    Ok(())
}

fn expiry_of(ctx: &QueryCtx, row: &RequestRow) -> Result<u64, Error> {
    Ok(EXPIRIES.get(ctx, &row.id)?.unwrap_or(row.expiry))
}

fn current(ctx: &QueryCtx, mut row: RequestRow) -> Result<RequestRow, Error> {
    row.expiry = expiry_of(ctx, &row)?;
    Ok(row)
}

fn open(ctx: &QueryCtx, id: u64) -> Result<Stored, Error> {
    REQUESTS
        .get(ctx, &id)?
        .ok_or_else(|| not_found(format!("no open request {id}")))
}

fn close(ctx: &ExecCtx, stored: Stored, answer: Result<Vec<u8>, Error>) -> Result<(), Error> {
    let row = stored.row;
    REQUESTS.remove(ctx, &row.id);
    SENDERS.remove(ctx, &row.id);
    EXPIRIES.remove(ctx, &row.id);
    ASSIGNED.remove(ctx, &(row.node.clone(), row.id));
    STREAMS.remove(ctx, &row.id);
    drop_early(ctx, row.id)?;
    ctx.resolve(stored.ticket, answer)?;
    Ok(())
}

fn drop_early(ctx: &ExecCtx, id: u64) -> Result<(), Error> {
    for (key, _) in EARLY.scan(ctx, EARLY.prefix_of(&id))? {
        EARLY.remove(ctx, &key);
    }
    Ok(())
}

fn from_its_node(ctx: &Ctx, id: u64) -> Result<Stored, Error> {
    let key = ctx.env().signer()?;
    let stored = open(ctx, id)?;
    if stored.row.node != key {
        return Err(unauthorized(format!(
            "request {id} was elected to another node"
        )));
    }
    let same_block = ctx.env().height <= stored.row.made;
    if same_block {
        return Err(wrong_state(format!(
            "request {id} is answered in a later block than the one that made it: wait for the next block before answering"
        )));
    }
    Ok(stored)
}

fn stream_event_prefix(stored: &Stored) -> Result<&[u8], Error> {
    match &stored.row.delivery {
        Delivery::Events { deliver } => Ok(deliver),
        Delivery::Whole => Err(wrong_state(format!(
            "request {} asked for its answer whole: send a whole answer instead of events",
            stored.row.id
        ))),
    }
}

pub(crate) async fn answer(ctx: &Ctx, id: u64, answer: Answer) -> Result<(), Error> {
    let stored = from_its_node(ctx, id)?;
    ctx.cancel(expiry_of(ctx, &stored.row)?).await?;
    let answered = match answer {
        Answer::Output(output) => Ok(output),
        Answer::Failed(reason) => Err(failed(stored.row.request.service(), &reason)),
    };
    close(ctx, stored, answered)
}

pub(crate) async fn events(ctx: &Ctx, events: Vec<Event>) -> Result<(), Error> {
    if events.is_empty() {
        return Err(invalid("an Events op carries at least one event"));
    }
    let mut events = events.into_iter().peekable();
    while let Some(first) = events.next() {
        let id = first.id;
        let stored = from_its_node(ctx, id)?;
        let deliver = stream_event_prefix(&stored)?.to_vec();
        let mut stream = STREAMS.get(ctx, &id)?.unwrap_or_default();
        let height = ctx.env().height;
        let unmoved = stream.moved != height;
        if unmoved {
            let expires = expiry_height(height, stored.wait)?;
            ctx.cancel(expiry_of(ctx, &stored.row)?).await?;
            let expiry = ctx.schedule(expires, PROGRAM, &Op::Expire { id }).await?;
            EXPIRIES.put(ctx, &id, &expiry);
            stream.moved = height;
        }
        let same_request = std::iter::once(first).chain(std::iter::from_fn(|| {
            events.next_if(|event| event.id == id)
        }));
        for event in same_request {
            hear(ctx, &stored.row, &deliver, &mut stream, event)?;
        }
        let heard_out = stream
            .ending
            .clone()
            .filter(|ending| stream.next >= ending.events);
        match heard_out {
            Some(ending) => finish(ctx, stored, ending).await?,
            None => STREAMS.put(ctx, &id, &stream),
        }
    }
    Ok(())
}

enum Arrival {
    Restart,
    Repeat,
    Due,
    Early,
}

fn arrival(next: u64, seq: u64) -> Arrival {
    let restarted = seq == 0 && next > 0;
    if restarted {
        return Arrival::Restart;
    }
    match seq.cmp(&next) {
        std::cmp::Ordering::Less => Arrival::Repeat,
        std::cmp::Ordering::Equal => Arrival::Due,
        std::cmp::Ordering::Greater => Arrival::Early,
    }
}

fn hear(
    ctx: &ExecCtx,
    row: &RequestRow,
    deliver: &[u8],
    stream: &mut Stream,
    event: Event,
) -> Result<(), Error> {
    match arrival(stream.next, event.seq) {
        Arrival::Restart => restart(ctx, row, deliver, stream, event),
        Arrival::Repeat => Ok(()),
        Arrival::Due => due(ctx, row, deliver, stream, event),
        Arrival::Early => {
            EARLY.put(ctx, &(row.id, event.seq), &event.data);
            Ok(())
        }
    }
}

fn restart(
    ctx: &ExecCtx,
    row: &RequestRow,
    deliver: &[u8],
    stream: &mut Stream,
    event: Event,
) -> Result<(), Error> {
    drop_early(ctx, row.id)?;
    stream.next = 0;
    stream.ending = None;
    due(ctx, row, deliver, stream, event)
}

fn due(
    ctx: &ExecCtx,
    row: &RequestRow,
    deliver: &[u8],
    stream: &mut Stream,
    event: Event,
) -> Result<(), Error> {
    let mut event = event;
    loop {
        ctx.emit_raw(row.requester.clone(), event.payload(deliver), Reply::Wanted);
        stream.next = event.seq + 1;
        let following = (row.id, stream.next);
        let Some(data) = EARLY.get(ctx, &following)? else {
            return Ok(());
        };
        EARLY.remove(ctx, &following);
        event = Event {
            id: row.id,
            seq: stream.next,
            data,
        };
    }
}

pub(crate) async fn end_stream(
    ctx: &Ctx,
    id: u64,
    events: u64,
    end: StreamEnd,
) -> Result<(), Error> {
    let stored = from_its_node(ctx, id)?;
    stream_event_prefix(&stored)?;
    let mut stream = STREAMS.get(ctx, &id)?.unwrap_or_default();
    let ending = Ending { events, end };
    let unheard = stream.next < events;
    if unheard {
        stream.ending = Some(ending);
        STREAMS.put(ctx, &id, &stream);
        return Ok(());
    }
    finish(ctx, stored, ending).await
}

async fn finish(ctx: &Ctx, stored: Stored, ending: Ending) -> Result<(), Error> {
    ctx.cancel(expiry_of(ctx, &stored.row)?).await?;
    let Ending { events, end } = ending;
    let ended = match &end {
        StreamEnd::Complete { status, headers } => Ok(abi::encode(&http::Response {
            status: *status,
            headers: headers.clone(),
            body: Vec::new(),
        })),
        StreamEnd::Cut { reason } => Err(failed(
            stored.row.request.service(),
            &format!("the stream broke off, the node sent {events} events: {reason}"),
        )),
    };
    close(ctx, stored, ended)
}

fn failed(service: &str, reason: &str) -> Error {
    wrong_state(format!("the {service} service failed: {reason}"))
}

pub(crate) fn expire(ctx: &ExecCtx, id: u64) -> Result<(), Error> {
    let env = ctx.env();
    let Cause::Scheduled { entry } = env.cause else {
        return Err(unauthorized(
            "a request expires only by its own scheduled run",
        ));
    };
    let from_itself = env.origin == Origin::Program(env.program.clone());
    if !from_itself {
        return Err(unauthorized(
            "a request expires only by the run egress scheduled",
        ));
    }
    let stored = open(ctx, id)?;
    let its_expiry = expiry_of(ctx, &stored.row)? == entry;
    if !its_expiry {
        return Err(unauthorized(format!(
            "entry {entry} is not request {id}'s expiry"
        )));
    }
    let waited = env.height.saturating_sub(stored.row.made);
    let expired = wrong_state(format!("request {id} expired after {waited} blocks"));
    close(ctx, stored, Err(expired))
}

pub(crate) fn assigned(ctx: &QueryCtx, key: &Vec<u8>) -> Result<Vec<RequestRow>, Error> {
    ASSIGNED
        .scan(ctx, ASSIGNED.prefix_of(key))?
        .into_iter()
        .map(|(_, id)| current(ctx, open(ctx, id)?.row))
        .collect()
}

pub(crate) fn request_row(ctx: &QueryCtx, id: u64) -> Result<Option<RequestRow>, Error> {
    REQUESTS
        .get(ctx, &id)?
        .map(|stored| current(ctx, stored.row))
        .transpose()
}

pub(crate) fn sender(ctx: &QueryCtx, id: u64) -> Result<Option<AccountNumber>, Error> {
    SENDERS.get(ctx, &id)
}

pub(crate) fn node(ctx: &QueryCtx, key: &Vec<u8>) -> Result<Option<NodeRow>, Error> {
    NODES.get(ctx, key)
}

pub(crate) fn nodes(ctx: &QueryCtx, page: &PageRequest) -> Result<PageResponse<NodeRow>, Error> {
    Ok(NODES.range(ctx, page)?.map(|(_, row)| row))
}

pub(crate) fn service(ctx: &QueryCtx, name: &str) -> Result<Option<ServiceRow>, Error> {
    SERVICES.get(ctx, &name.to_owned())
}

pub(crate) fn services(
    ctx: &QueryCtx,
    page: &PageRequest,
) -> Result<PageResponse<ServiceRow>, Error> {
    Ok(SERVICES.range(ctx, page)?.map(|(_, row)| row))
}
