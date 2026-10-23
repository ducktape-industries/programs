//! The fake node's front: the `/v1` routes the app's node client speaks,
//! over plain threads. Every connection thread parses one HTTP/1.1
//! request at a time and hands it to the one chain thread as a job, as
//! [`Network`] is single-threaded; `/v1/changes/<program>` upgrades to a
//! websocket the chain thread writes to as blocks land.
//!
//! ponytail: HTTP/1.1 with `Content-Length` bodies, which is all reqwest
//! sends here; no chunked uploads, no TLS.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, Sender, channel};

use program::abi;

use super::wire::{self, route};
use super::{Network, now_ms};
use view_wire::methods;

type Job = Box<dyn FnOnce(&Network) + Send>;

pub(super) enum Msg {
    Job(Job),
    Stop,
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

struct Response {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

impl Response {
    fn borsh<T: borsh::BorshSerialize>(value: &T) -> Response {
        Response {
            status: 200,
            content_type: "application/octet-stream",
            body: abi::encode(value),
        }
    }

    fn refused(refusal: abi::Refusal) -> Response {
        Response {
            status: 400,
            content_type: "application/octet-stream",
            body: abi::encode(&refusal),
        }
    }

    fn text(status: u16, text: &str) -> Response {
        Response {
            status,
            content_type: "text/plain",
            body: text.as_bytes().to_vec(),
        }
    }
}

pub(super) fn bind(jobs: Sender<Msg>) -> std::io::Result<SocketAddr> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    std::thread::Builder::new()
        .name("live-node".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let jobs = jobs.clone();
                let _ = std::thread::Builder::new()
                    .name("live-conn".into())
                    .spawn(move || connection(stream, jobs));
            }
        })?;
    Ok(addr)
}

pub(super) fn serve(net: &Network, msgs: Receiver<Msg>) {
    while let Ok(Msg::Job(job)) = msgs.recv() {
        job(net);
    }
}

fn on_chain<T: Send + 'static>(
    jobs: &Sender<Msg>,
    work: impl FnOnce(&Network) -> T + Send + 'static,
) -> Option<T> {
    let (tell, told) = channel();
    jobs.send(Msg::Job(Box::new(move |net| {
        let _ = tell.send(work(net));
    })))
    .ok()?;
    told.recv().ok()
}

fn connection(stream: TcpStream, jobs: Sender<Msg>) {
    let mut reader = BufReader::new(stream.try_clone().expect("a socket clones"));
    let mut writer = stream;
    while let Ok(Some(request)) = read_request(&mut reader) {
        if request
            .header("upgrade")
            .is_some_and(|upgrade| upgrade.eq_ignore_ascii_case("websocket"))
        {
            if let Some(program) = request.path.strip_prefix(&format!("{}/", route::CHANGES)) {
                changes_socket(reader, writer, &request, program, &jobs);
            }
            return;
        }
        let Some(response) = on_chain(&jobs, {
            let (method, path, body) = (request.method, request.path, request.body);
            move |net| respond(net, &method, &path, body)
        }) else {
            return;
        };
        if write_response(&mut writer, &response).is_err() {
            return;
        }
        let _ = jobs.send(Msg::Job(Box::new(|net| net.flush())));
    }
}

fn read_request(reader: &mut BufReader<TcpStream>) -> std::io::Result<Option<Request>> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let (method, path) = match (parts.next(), parts.next()) {
        (Some(method), Some(path)) => (method.to_owned(), path.to_owned()),
        _ => return Err(std::io::Error::other("not a request line")),
    };
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_owned(), value.trim().to_owned()));
        }
    }
    let length = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Some(Request {
        method,
        path,
        headers,
        body,
    }))
}

fn write_response(writer: &mut TcpStream, response: &Response) -> std::io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Error",
    };
    write!(
        writer,
        "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n",
        response.status,
        response.content_type,
        response.body.len()
    )?;
    writer.write_all(&response.body)?;
    writer.flush()
}

fn malformed(sentence: impl std::fmt::Display) -> Response {
    Response::refused(abi::Refusal::new(
        methods::refusal::MALFORMED_REQUEST,
        sentence.to_string(),
    ))
}

fn refusal(error: &crate::host::Error) -> abi::Refusal {
    abi::Refusal::new(&error.code, &error.message)
}

fn respond(net: &Network, method: &str, path: &str, body: Vec<u8>) -> Response {
    match (method, path) {
        ("GET", route::STATUS) => match net.answer("chain.status", None, &[]) {
            Ok(bytes) => {
                let status: methods::NodeStatus = methods::decode(&bytes).expect("own bytes");
                Response::borsh(&wire::Status {
                    network: status.chain_id,
                    time: status.time,
                    block_time_ms: status.block_time_ms,
                    epoch_length: status.epoch_length,
                    height: status.height,
                    tip: status.tip,
                    root: abi::Root(status.root),
                    epoch: status.epoch,
                    identity: status.identity,
                    contract: status.contract,
                    genesis: net.genesis(),
                    roles: net.roles(),
                })
            }
            Err(error) => Response::refused(refusal(&error)),
        },
        ("GET", route::NETWORK) => match net.answer("chain.network", None, &[]) {
            Ok(bytes) => {
                let network: methods::NetworkStatus = methods::decode(&bytes).expect("own bytes");
                Response::borsh(&wire::Network {
                    height: network.height,
                    members: network
                        .members
                        .into_iter()
                        .map(|peer| wire::PeerStatus {
                            key: peer.key,
                            signed: peer.signed,
                        })
                        .collect(),
                })
            }
            Err(error) => Response::refused(refusal(&error)),
        },
        ("POST", route::QUERY) => {
            let query: wire::Query = match abi::decode(&body) {
                Ok(query) => query,
                Err(error) => return malformed(error.sentence),
            };
            let frame = match verify_signed_frame(&query.frame) {
                Ok(frame) => frame,
                Err(response) => return response,
            };
            let call = methods::encode(&methods::Call {
                target: frame.body.target.clone(),
                body: frame.body.payload,
            });
            match net.answer_as(
                frame.body.signer,
                "program.query",
                Some(&frame.body.target),
                &call,
            ) {
                Ok(bytes) => Response::borsh(&bytes),
                Err(error) => Response::refused(refusal(&error)),
            }
        }
        ("POST", route::SUBMIT) => {
            let frame = match verify_signed_frame(&body) {
                Ok(frame) => frame,
                Err(response) => return response,
            };
            if let Err(refusal) = net.fresh(&frame.body) {
                return Response::refused(refusal);
            }
            let (height, id) = (net.height(), frame.body.id());
            let call = methods::encode(&methods::Call {
                target: frame.body.target.clone(),
                body: frame.body.payload,
            });
            let outcome = match net.answer_as(
                frame.body.signer,
                "op.submit",
                Some(&frame.body.target),
                &call,
            ) {
                Ok(bytes) => match methods::decode::<methods::Submitted>(&bytes) {
                    Ok(methods::Submitted::Applied { output, .. }) => {
                        abi::Outcome::Applied { output }
                    }
                    Ok(methods::Submitted::Refused { refusal, .. }) => {
                        abi::Outcome::Rejected(abi::Refusal::new(&refusal.code, &refusal.message))
                    }
                    Ok(methods::Submitted::InDoubt { .. }) => {
                        return Response::text(503, "no answer came back: the frame may be pooled");
                    }
                    Err(error) => return Response::text(500, &error),
                },
                Err(error) => abi::Outcome::Rejected(refusal(&error)),
            };
            let carried = net.height() > height;
            if carried {
                net.spend(id);
            }
            Response::borsh(&wire::Receipt {
                program: frame.body.target,
                outcome,
                events: Vec::new(),
                nested: Vec::new(),
            })
        }
        ("POST", route::BLOB_GET) => {
            let id: abi::BlobId = match abi::decode(&body) {
                Ok(id) => id,
                Err(error) => return malformed(error.sentence),
            };
            let name = match id {
                abi::BlobId::Sha256(digest) => format!("sha256:{}", abi::hex(&digest)),
                abi::BlobId::Sha1(digest) => format!("sha1:{}", abi::hex(&digest)),
            };
            match net.answer("blob.get", None, &methods::encode(&name)) {
                Ok(bytes) => {
                    let body: Option<Vec<u8>> = methods::decode(&bytes).expect("own bytes");
                    Response::borsh(&body.map(|body| {
                        net.blob(&id)
                            .filter(|framed| super::unframe(framed) == Some(&body[..]))
                            .unwrap_or_else(|| wire::framed("blob", &body))
                    }))
                }
                Err(error) => Response::refused(refusal(&error)),
            }
        }
        ("POST", route::BLOCKS) => {
            let page: wire::Blocks = match abi::decode(&body) {
                Ok(page) => page,
                Err(error) => return malformed(error.sentence),
            };
            let page = methods::encode(&methods::BlockPage {
                before: page.before,
                limit: page.limit,
                most: page.most,
            });
            match net.answer("chain.blocks", None, &page) {
                Ok(bytes) => {
                    let list: methods::BlockList = methods::decode(&bytes).expect("own bytes");
                    Response::borsh(&wire::Page {
                        blocks: list.blocks.iter().map(wire::finalized).collect(),
                        listed_from: list.listed_from,
                    })
                }
                Err(error) => Response::refused(refusal(&error)),
            }
        }
        ("POST", route::BLOCK_FACTS) => {
            let ask: methods::FactsAsk = match abi::decode(&body) {
                Ok(ask) => ask,
                Err(error) => return malformed(error.sentence),
            };
            match net.answer("chain.facts", None, &methods::encode(&ask)) {
                Ok(bytes) => {
                    let page: methods::FactsPage = methods::decode(&bytes).expect("own bytes");
                    Response::borsh(&page)
                }
                Err(error) => Response::refused(refusal(&error)),
            }
        }
        ("POST", route::TXS) => {
            let range: methods::TxRange = match abi::decode(&body) {
                Ok(range) => range,
                Err(error) => return malformed(error.sentence),
            };
            match net.answer("chain.txs", None, &methods::encode(&range)) {
                Ok(bytes) => {
                    let page: methods::TxPage = methods::decode(&bytes).expect("own bytes");
                    Response::borsh(&page)
                }
                Err(error) => Response::refused(refusal(&error)),
            }
        }
        ("POST", route::BLOCK) => {
            let ask: wire::BlockAsk = match abi::decode(&body) {
                Ok(ask) => ask,
                Err(error) => return malformed(error.sentence),
            };
            let by = match ask.by {
                wire::BlockRef::Height(height) => methods::BlockRef::Height(height),
                wire::BlockRef::Id(id) => methods::BlockRef::Id(id),
            };
            let ask = methods::encode(&methods::BlockAsk { by, most: ask.most });
            match net.answer("chain.block", None, &ask) {
                Ok(bytes) => {
                    let block: Option<methods::Block> = methods::decode(&bytes).expect("own bytes");
                    Response::borsh(&block.as_ref().map(wire::finalized))
                }
                Err(error) => Response::refused(refusal(&error)),
            }
        }
        ("POST", route::TX) => {
            let ask: wire::TxAsk = match abi::decode(&body) {
                Ok(ask) => ask,
                Err(error) => return malformed(error.sentence),
            };
            let tx = match ask.tx {
                wire::TxRef::At { height, position } => methods::TxRef::At { height, position },
                wire::TxRef::Hash(hash) => methods::TxRef::Hash(hash),
            };
            let ask = methods::encode(&methods::TxAsk { tx, most: ask.most });
            match net.answer("chain.tx", None, &ask) {
                Ok(bytes) => {
                    let found: Option<methods::Located> =
                        methods::decode(&bytes).expect("own bytes");
                    Response::borsh(&found.map(|found| wire::Located {
                        height: found.height,
                        position: found.position,
                        time: found.time,
                        tx: wire::tx(&found.tx),
                    }))
                }
                Err(error) => Response::refused(refusal(&error)),
            }
        }
        ("POST", route::INVITE) => {
            let ttl = serde_json::from_slice::<serde_json::Value>(&body)
                .ok()
                .and_then(|json| json.get("ttl_days")?.as_u64())
                .unwrap_or(0);
            let ask = methods::encode(&methods::CreateInvite { ttl_days: ttl });
            match net.answer("invite.create", None, &ask) {
                Ok(bytes) => {
                    let invite: methods::Invite = methods::decode(&bytes).expect("own bytes");
                    let notes: Vec<serde_json::Value> = invite
                        .notes
                        .iter()
                        .map(|note| serde_json::json!({ "reason": note.code, "sentence": note.message }))
                        .collect();
                    Response {
                        status: 200,
                        content_type: "application/json",
                        body: serde_json::json!({ "invite": invite.invite, "notes": notes })
                            .to_string()
                            .into_bytes(),
                    }
                }
                Err(error) => Response {
                    status: 400,
                    content_type: "application/json",
                    body: serde_json::json!({ "error": error.message, "reason": error.code })
                        .to_string()
                        .into_bytes(),
                },
            }
        }
        _ => Response::text(404, "no such route"),
    }
}

fn verify_signed_frame(bytes: &[u8]) -> Result<wire::Frame, Response> {
    let frame: wire::Frame = abi::decode(bytes).map_err(|error| malformed(error.sentence))?;
    let verified = frame.body.scheme.verify(
        &frame.body.signer,
        wire::FRAME_NAMESPACE,
        &frame.body.preimage(),
        &frame.proof,
    );
    if !verified {
        return Err(Response::refused(abi::Refusal::new(
            "bad_signature",
            "the frame's proof does not verify",
        )));
    }
    Ok(frame)
}

const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

fn changes_socket(
    mut reader: BufReader<TcpStream>,
    mut writer: TcpStream,
    request: &Request,
    program: &str,
    jobs: &Sender<Msg>,
) {
    use base64::Engine as _;
    use sha1::Digest as _;
    let Some(key) = request.header("sec-websocket-key") else {
        return;
    };
    let accept = base64::engine::general_purpose::STANDARD
        .encode(sha1::Sha1::digest(format!("{key}{WS_GUID}").as_bytes()));
    let (send, items) = channel::<Vec<u8>>();
    let program = program.to_owned();
    if on_chain(jobs, move |net| net.subscribe(&program, send)).is_none() {
        return;
    }
    if write!(
        writer,
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    )
    .and_then(|()| writer.flush())
    .is_err()
    {
        return;
    }
    let mut out = match writer.try_clone() {
        Ok(out) => out,
        Err(_) => return,
    };
    let writing = std::thread::spawn(move || {
        for item in items {
            if write_unmasked_server_frame(&mut out, 0x2, &item).is_err() {
                break;
            }
        }
    });
    while let Some((opcode, payload)) = read_masked_client_frame(&mut reader) {
        match opcode {
            0x8 => {
                let _ = write_unmasked_server_frame(&mut writer, 0x8, &payload);
                break;
            }
            0x9 if write_unmasked_server_frame(&mut writer, 0xA, &payload).is_err() => break,
            _ => {}
        }
    }
    let _ = writer.shutdown(std::net::Shutdown::Both);
    let _ = writing.join();
}

fn write_unmasked_server_frame(
    out: &mut TcpStream,
    opcode: u8,
    payload: &[u8],
) -> std::io::Result<()> {
    let mut frame = vec![0x80 | opcode];
    match payload.len() {
        len if len < 126 => frame.push(len as u8),
        len if len < 65536 => {
            frame.push(126);
            frame.extend_from_slice(&(len as u16).to_be_bytes());
        }
        len => {
            frame.push(127);
            frame.extend_from_slice(&(len as u64).to_be_bytes());
        }
    }
    frame.extend_from_slice(payload);
    out.write_all(&frame)?;
    out.flush()
}

fn read_masked_client_frame(reader: &mut BufReader<TcpStream>) -> Option<(u8, Vec<u8>)> {
    let mut head = [0u8; 2];
    reader.read_exact(&mut head).ok()?;
    let opcode = head[0] & 0x0f;
    let masked = head[1] & 0x80 != 0;
    let mut len = (head[1] & 0x7f) as u64;
    if len == 126 {
        let mut ext = [0u8; 2];
        reader.read_exact(&mut ext).ok()?;
        len = u16::from_be_bytes(ext) as u64;
    } else if len == 127 {
        let mut ext = [0u8; 8];
        reader.read_exact(&mut ext).ok()?;
        len = u64::from_be_bytes(ext);
    }
    let mut mask = [0u8; 4];
    if masked {
        reader.read_exact(&mut mask).ok()?;
    }
    let mut payload = vec![0; len as usize];
    reader.read_exact(&mut payload).ok()?;
    if masked {
        for (at, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[at % 4];
        }
    }
    Some((opcode, payload))
}

pub(super) fn say(line: impl std::fmt::Display) {
    println!("[live {}] {line}", now_ms() % 100_000);
}

#[cfg(test)]
mod tests {
    use super::super::{CHAIN, DEV_SEED, key_from_seed};
    use super::*;
    use crate::methods::Submit;
    use commonware_cryptography::Signer as _;

    fn signed(net: &Network, anchor: wire::Tip, salt: Option<u64>) -> (wire::Body, Vec<u8>) {
        signed_op(
            net,
            anchor,
            salt,
            &identity::Op::CreateAgent { name: "bot".into() },
        )
    }

    fn signed_op(
        net: &Network,
        anchor: wire::Tip,
        salt: Option<u64>,
        op: &identity::Op,
    ) -> (wire::Body, Vec<u8>) {
        let body = wire::Body {
            scheme: wire::KeyScheme::Ed25519,
            signer: net.key_of(net.me()),
            network: CHAIN.as_bytes().to_vec(),
            anchor,
            salt,
            target: identity::PROGRAM.into(),
            payload: methods::encode(op),
        };
        let proof = key_from_seed(DEV_SEED)
            .sign(wire::FRAME_NAMESPACE, &body.preimage())
            .as_ref()
            .to_vec();
        let frame = abi::encode(&wire::Frame {
            body: body.clone(),
            proof,
        });
        (body, frame)
    }

    fn tip(net: &Network) -> wire::Tip {
        let status = net.status();
        wire::Tip {
            height: status.height,
            id: status.tip,
        }
    }

    fn refused(response: &Response) -> String {
        assert_ne!(response.status, 200);
        let refusal: abi::Refusal = abi::decode(&response.body).unwrap();
        refusal.reason
    }

    #[test]
    fn the_chain_answers_the_windows_submit_as_a_receipt() {
        let net = Network::new();
        let height = net.height();
        let (_, frame) = signed(&net, tip(&net), None);
        let response = respond(&net, "POST", route::SUBMIT, frame);
        assert_eq!(response.status, 200);
        let receipt: wire::Receipt = abi::decode(&response.body).unwrap();
        let abi::Outcome::Applied { output } = receipt.outcome else {
            panic!("the chain applies a CreateAgent: {:?}", receipt.outcome)
        };
        let agent: u64 = methods::decode(&output).expect("the program's own output");
        assert!(agent > net.me());
        assert_eq!(net.height(), height + 1);

        let (_, frame) = signed_op(
            &net,
            tip(&net),
            None,
            &identity::Op::Suspend { account: 999_999 },
        );
        let response = respond(&net, "POST", route::SUBMIT, frame);
        assert_eq!(response.status, 200);
        let receipt: wire::Receipt = abi::decode(&response.body).unwrap();
        assert!(
            matches!(receipt.outcome, abi::Outcome::Rejected(_)),
            "the program refuses a suspend of no account: {:?}",
            receipt.outcome
        );
    }

    #[test]
    fn an_override_answers_the_windows_submit() {
        let net = Network::new();
        net.refuse::<Submit<identity::Identity>>("closed", "refused by the test");
        let height = net.height();
        let (body, frame) = signed(&net, tip(&net), None);
        let response = respond(&net, "POST", route::SUBMIT, frame);
        assert_eq!(response.status, 200);
        let receipt: wire::Receipt = abi::decode(&response.body).unwrap();
        assert_eq!(
            receipt.outcome,
            abi::Outcome::Rejected(abi::Refusal::new("closed", "refused by the test"))
        );
        assert_eq!(net.height(), height);
        assert_eq!(net.fresh(&body), Ok(()));
    }

    #[test]
    fn the_window_runs_a_frame_once_while_its_anchor_is_recent() {
        let net = Network::new();
        let anchor = tip(&net);
        let (_, frame) = signed(&net, anchor, None);
        assert_eq!(
            respond(&net, "POST", route::SUBMIT, frame.clone()).status,
            200
        );
        assert_eq!(
            refused(&respond(&net, "POST", route::SUBMIT, frame)),
            abi::reason::DUPLICATE
        );

        let (_, salted) = signed(&net, anchor, Some(1));
        assert_eq!(respond(&net, "POST", route::SUBMIT, salted).status, 200);

        let unknown = wire::Tip {
            id: [9; 32],
            ..anchor
        };
        let (_, forged) = signed(&net, unknown, None);
        assert_eq!(
            refused(&respond(&net, "POST", route::SUBMIT, forged)),
            abi::reason::ANCHOR
        );
    }
}
