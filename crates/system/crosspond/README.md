# crosspond

Links this network to other ducktape networks, so an account here can run an op on a program there and an account there can run one here. The program fills the `crosspond` role (`abi::role::crosspond`, whose `Op`, `Query` and `Reply` it speaks). It keeps one row per link, the packets this network owes each link, the receipts and settled rows of the packets it took, and the names it heard from each network. It runs a light client of each linked network: a packet another network delivers runs here only once a certificate of that network's validators and a state proof show the other network committed it, and an ack or a low-water mark is taken only the same way. Shapes: `abi::role::crosspond`; checks: `src/rules.rs` and `src/light.rs`; queries: `src/queries.rs`; tables: `src/state.rs`.

## Who may call each op
| Op | Who |
|---|---|
| `Open { network, name, epoch_length, epoch, kind, validators }` | the validators' vote alone (`env.authority()`, a message from the program bound to the validators role). Refused `already_exists` for a network linked and open, and `invalid_input` for an empty or doubled validator set or an epoch of no blocks. `name` is the other network's name, the one its validators sign under, and `kind` is the certificate kind they sign with. On a closed link it opens the link again: the name, epoch length, kind and trusted set are the ones given, no header is held, and the link keeps its `next_in`, `next_out`, the packets it owes and the receipts it holds, so no sequence number is taken or used twice |
| `Repin { network, epoch, kind, validators }` | the validators' vote alone. Trusts this set, signing with this certificate kind, at this epoch from now on, for a link whose headers stall because the other network's set changed too much |
| `Close { network }` | the validators' vote alone. A closed link takes no send, header or delivery until an `Open` opens it again |
| `Port { role, from }` | the validators' vote alone. The networks whose packets may run on the program bound to `role` here, kept sorted and once each; an empty `from` closes the port |
| `Send { to, port, op, deadline }` | any account, as itself (`sender_account`: a key the chain holds; a foreign sender is refused). Returns the packet's sequence number |
| `SendFor { to, port, op, deadline, account }` | the program bound to the `port` role here alone, for one of its accounts. The packet carries `via = Some(port)`. Returns the packet's sequence number |
| `Pump { to }` | crosspond's own scheduled run alone (`Cause::Scheduled`, sent by crosspond) |
| `Header { from, header }` | anyone: the checks below decide |
| `DeliverProven { from, epoch, low, packets }` | anyone: each packet's proof and the low-water mark's decide |

## Sending
A send commits the packet as this program's state at `commitment_key(destination, seq)` (the `out/` table's key for that pair), records its status `Pending`, and tells of it with a `Changed { to, seq, status }` event. The packet names its sender's account and the name identity holds for it. A packet whose encoding is past `max_packet` of the link's network name is refused `capacity`: that is the node's block size less what a relay frame to that network carries besides the packet (its envelopes, the network's name, a crosspond program id at its longest, the fields of a `DeliverProven` with no low-water proof and the longest op proof). Sequence numbers count from 1 per link.

The first send to a link with nothing being relayed schedules one `Pump` at the next height. While a pump is asking, further sends schedule none: the running pump asks again until nothing is owed. A pump the kernel cancelled is replaced by the first send once its request could have expired (`DEFAULT_WAIT` blocks after it asked).

`Pump` asks egress for `Request::Crosspond { link }`, answered whole, and waits for the answer, parked between blocks. The answer is `Acked { epoch, header, acks }`: the other network's settled rows (`Settled { source, destination, seq, packet, result }` at `settled_key(source, seq)`), each with an op proof under that network's crosspond root at the close of `epoch`. The root is the one the link holds for `epoch`, or else `header`'s, which passes the checks of `Header` below and is then the link's (a closed link takes none). An ack is taken only when its proof holds under that root and its row names this network as source, the link as destination, and the seq and sha256 of the packet's borsh of a packet still owed. It then leaves the outbox and its status becomes `Done` with the other network's result. Any other ack, and every ack of an answer whose header fails or does not decode, is not taken: its packet stays owed with its status as it was, the link keeps why as `refused`, and the pump asks again. Each time the lowest packet owed to the link rises, to n, crosspond writes the row `low_key(link, n)` (`low/<link>/<n>`, holding no bytes) and deletes the one before: it holds the acks of every packet below n. n only rises, so each such row is written once. Then the pump asks again while packets are owed. An error tells three cases apart by when it lands:

- in the block the pump asked: egress refused the request itself, for instance because no node serves crosspond. Every owed packet is recorded `Refused` with the reason, the link keeps it as `refused`, and nothing asks until the next send.
- before the request's wait is over: the node answered that the whole relay failed. Every owed packet is recorded `Refused` with the node's reason, and the pump asks again.
- once the wait is over: the request expired. Nothing is recorded, and the pump asks again.

## Taking a header
`Header` moves the link's trust to a later epoch of the other network. The header names its certificate kind as its first byte. A kind this abi does not name does not decode, so the frame is refused before any of the certificate is read. The light client then matches the header's kind with the link's, and runs that kind's checks. The one kind, `Ed25519SimplexFinalize`, is ed25519 signatures under `<name>_FINALIZE` in a commonware simplex finalization certificate. Its checks, in order:

1. The validator set is not empty and names each key once; the certificate is a simplex finalization over that set. Its epoch must be later than the trusted one, else `stale`.
2. The certificate's payload is the sha256 of the header's block.
3. The block's height is the first of its epoch (`epoch × epoch_length`), and its last frame is the `Settled` record of the height below.
4. The sha256 of the borsh of `roots` is the root that record settles.
5. The validators are the ones the other network recorded at `validators/<epoch>`, by an op proof under its `$network` root.
6. A quorum of those validators signed: n − (n − 1)/3 of n, each signature an ed25519 one under `<name>_FINALIZE` over the proposal.
7. More than a third of the set the link trusts signed, else `not_yet`: the relay sends the headers of the epochs between first, or the vote repins the link.

The link then trusts the new epoch and its set, and keeps the crosspond program's root from `roots` as the header packets are proven against.

## Taking a delivery
`DeliverProven` carries the packets the other network committed, each with its proof at the epoch of the header the link holds (`not_yet` before any header, `stale` for another epoch). The whole frame is refused `invalid_input` if one proof does not hold, a packet is from or for another network, or the packets are not in rising sequence order. `DeliverProven`'s `low`, when it is there, is the other network's row `low/<this network>/<n>` with its op proof under the same header's crosspond root, and the frame is refused `invalid_input` if that proof does not hold; it may come with no packets. A proven n prunes the receipts and settled rows below n whose result is written: the other network holds their acks. A receipt whose run has not replied is never pruned. The link keeps the highest n it was proven, and a proof of an n no higher prunes nothing. Op tag 8, the `Deliver` an earlier build took, is retired: those bytes decode as no op and are refused whole. Then, packet by packet:

- a packet below the next one expected runs nothing: once more if its receipt is kept, `duplicate` once it was pruned.
- a packet past the next one expected is `not_yet`: the ones before come first.
- the next one expected is taken. Its sender's name is kept. A packet past its deadline gets a `stale` receipt, one for a port that takes no packets from that network an `unauthorized` one, and one for a role no program fills an `unsupported` one. Otherwise its op runs on the program bound to the port role as `Principal::Foreign { network, account }`, and the reply settles its receipt with the result or the refusal.

A receipt settles once, when its result is final: crosspond then writes the packet's row `settled_key(from, seq)` (`settled/<from>/<seq>`), `Settled` with this network as destination, the packet's sha256 and the result, which the other network's relay proves. A second write to that key is refused `wrong_state`; sequence numbers are never taken twice, an `Open` that opens a closed link again included, so none is written. It is deleted with its receipt once a proven low-water mark is past it.

## Queries
| Query | Answer |
|---|---|
| `Link { network }` | the link's row, or none |
| `Outbound { to, after }` | the packets owed to `to` after a sequence number, in order |
| `Receipts { from, after }` | the receipts kept for `from` after a sequence number, in order; a receipt whose run has not replied holds no result |
| `Status { to, seq }` | a sent packet's status |
| `Ports` | each open port and the networks it takes |
| `Name(foreign)` | the name last heard for a foreign account |
| `Named { network, name }` | the foreign accounts of a network last heard under a name |
| `Home` | this network's id |
| `Linked { name }` | the ids of every link whose last `Open` named it so, closed ones too |

## This network's id
Packets name networks by the digest of their genesis block. At founding, init with no params takes this network's own from its name and genesis time; params naming another id are refused. Published later, crosspond takes the id as its params, and init refuses none.

## Builds
The rules and the light client build only with the `program` feature, which the program's own build turns on. Without it the crate is crosspond's contract: its `Program` impl with its types and its queries. A view links that and nothing of the light client's commonware crates. A test that runs crosspond's ops on a mock chain turns `program` on in its dev-dependency.
