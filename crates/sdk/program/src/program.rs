//! What a program is ([`Program`]): its name, the three borsh types it
//! speaks and what it does with each; the questions a caller asks it
//! ([`Ask`], [`Reads`]); the plain functions that decode an invocation and
//! answer it; and [`export!`](crate::export), which only points the two
//! wasm exports at them.

use std::fmt::Debug;

use abi::hook::Record;
use borsh::{BorshDeserialize, BorshSerialize};

use crate::{Cause, Error, ExecCtx, MessageId, Need, Outcome, QueryCtx, Resource, code, decoded};

/// A program: its name on the node, the types it speaks and what it does
/// with each op and query. A read-only program names `()` as its `Op`. The
/// bounds are borsh on both sides: a program's own request rides a method
/// with no second encoding around it. A view names the program by this
/// type (`Query<chat::Chat>`, `Submit<forge::Forge>`,
/// `Changes<identity::Identity>`), and the method encodes `NAME` as the
/// target.
pub trait Program {
    /// The id the program runs under: the target a view's method names.
    const NAME: &'static str;
    type Op: BorshSerialize + BorshDeserialize + Debug;
    type Query: BorshSerialize + BorshDeserialize + Debug;
    type Reply: BorshSerialize + BorshDeserialize;

    // What each op variant's output is the borsh of, for whoever reads the program's schema
    // (`schema::returns::<T>("Variant")`). An op left out returns bytes no schema names.
    const RETURNS: &'static [crate::schema::Return] = &[];

    /// Genesis, with the params the network was founded with. Nothing by
    /// default; override only where genesis needs it.
    fn init(_ctx: &ExecCtx, _params: &[u8]) -> Result<(), Error> {
        Ok(())
    }

    fn execute(_ctx: &ExecCtx, _op: Self::Op) -> Result<(), Error> {
        Err(Error::new(code::UNSUPPORTED, "this program executes no op"))
    }

    /// The outcome of message `id`, which this program emitted with
    /// [`Reply::Wanted`](crate::Reply::Wanted). A refusal here fails the
    /// frame; Ok absorbs the outcome. By default an applied message is
    /// absorbed and a refused one's refusal is returned, failing the frame
    /// as if the message had been sent with [`Reply::None`](crate::Reply::None);
    /// override to accept a refusal.
    fn reply(_ctx: &ExecCtx, _id: &MessageId, outcome: &Outcome) -> Result<(), Error> {
        match outcome {
            Outcome::Applied { .. } => Ok(()),
            Outcome::Rejected(refusal) => Err(refusal.clone()),
        }
    }

    fn query(ctx: &QueryCtx, query: Self::Query) -> Result<Self::Reply, Error>;

    fn access(_ctx: &QueryCtx, _op: &Self::Op) -> Result<Vec<Need>, Error> {
        Ok(Vec::new())
    }

    fn resource(_ctx: &QueryCtx, _key: &str) -> Result<Resource, Error> {
        Ok(Resource::default())
    }

    fn hook(_ctx: &ExecCtx, _record: Record) -> Result<(), Error> {
        Err(Error::new(code::UNSUPPORTED, "this program is no hook"))
    }

    /// The bytes a query is answered with: [`Program::query`]'s `Reply` as
    /// borsh. Override only for a query whose answer is not a `Reply` at
    /// all (forge serves git's own wire through two of its queries); every
    /// other program leaves it alone.
    fn answer(ctx: &QueryCtx, query: Self::Query) -> Result<Vec<u8>, Error> {
        Ok(abi::encode(&Self::query(ctx, query)?))
    }
}

/// One question to a program, as its own type: the program it asks, the
/// reply that answers it, and how it rides the wire (`into` the program's
/// `Query`, so its bytes are the enum's). A caller asks it and gets that
/// reply, typed, with no arm for the others.
///
/// `#[derive(Ask)]` on a program's `Query` writes them: `#[ask(Program)]`
/// on the enum, and on each variant a caller asks alone the reply that
/// answers it, `#[ask(Reply::List(PageResponse<Row>))]` (or a reply's
/// named fields, `#[ask(Reply::Thread { root: Option<Row>, replies:
/// PageResponse<Row> })]`, answered as their tuple). Each such variant
/// becomes a type of the same name and fields in a `pub mod ask` beside
/// the enum (`identity::ask::List { page }`). A field type that shares a
/// variant's name is written by its full path (`crate::Profile`).
pub trait Ask: Into<<Self::Program as Program>::Query> {
    type Program: Program;
    type Reply;
    /// The answer, out of the program's reply; `None` when it answers
    /// another question.
    fn answer(reply: <Self::Program as Program>::Reply) -> Option<Self::Reply>;
}

/// Which tables a query reads, so a follower re-reads it only when a block
/// wrote to one of them: a view asks a block's change whether it touches
/// the query (`change.touches::<P, _>(&query)`, `P` the program whose
/// block it is) and never maps keys to queries itself.
///
/// `#[derive(Ask)]` writes it from `#[reads(TABLE, ..)]` on each variant:
/// the `Map`/`Set`/`Item` consts the program answers that question from
/// (`#[reads(MESSAGES, REACTIONS)]`), each asked `owns(&key)`. A question
/// answered partly from another program's tables names them in a second
/// attribute, that program first (`#[reads(chat::Chat:
/// chat::tables::ANSWERED, chat::tables::MESSAGES)]`), so a follower of
/// that program's blocks re-reads the question too. The program states it,
/// since only the program knows what it reads. A variant with no `#[reads]`
/// is touched by every block of every program a follower asks about, which
/// is what a follower did before, so a program adopts this one query at a
/// time. A variant with ask types ([`Ask`]) gives them the same answer.
pub trait Reads {
    /// Whether a block of `program` (its [`Program::NAME`]) that wrote
    /// `keys` can have changed this query's answer.
    fn touched_by(&self, program: &str, keys: &[Vec<u8>]) -> bool;
}

/// Whether `name` is a program id as the network spells one: `1..=64`
/// bytes of lowercase ASCII letters, digits, `-` and `_`. One spelling per
/// id, so no id reads as another (no capital, no look-alike letter, no
/// bidi control). The one rule for a manifest's `targets`, a host's
/// `Call.target` and an id the programs role lands.
pub const fn is_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 {
        return false;
    }
    let mut i = 0;
    while i < bytes.len() {
        if !matches!(bytes[i], b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_') {
            return false;
        }
        i += 1;
    }
    true
}

/// An execute's payload decoded as `M::Op` (refused as invalid input when it
/// does not decode), then run; a [`Cause::Reply`] carries no op (the kernel
/// sends an empty payload) and runs [`Program::reply`] instead.
pub fn execute<M: Program>(ctx: &ExecCtx, payload: &[u8]) -> Result<(), Error> {
    if let Cause::Reply { id, outcome } = &ctx.env().cause {
        return M::reply(ctx, id, outcome);
    }
    let op = decoded::<M::Op>(&ctx.env().program, "Op", payload)?;
    M::execute(ctx, op)
}

pub fn hook<M: Program>(ctx: &ExecCtx, record: Record) -> Result<(), Error> {
    ctx.env().kernel()?;
    M::hook(ctx, record)
}

/// A query's request decoded as `M::Query`, and its [`Program::answer`]
/// handed to the host.
pub fn query<M: Program>(ctx: &QueryCtx, request: &[u8]) -> Result<(), Error> {
    let query = decoded::<M::Query>(&ctx.env().program, "Query", request)?;
    ctx.respond(M::answer(ctx, query)?);
    Ok(())
}

pub fn access<M: Program>(ctx: &QueryCtx, payload: &[u8]) -> Result<(), Error> {
    let op = decoded::<M::Op>(&ctx.env().program, "Op", payload)?;
    ctx.respond(abi::encode(&M::access(ctx, &op)?));
    Ok(())
}

pub fn resource<M: Program>(ctx: &QueryCtx, key: &str) -> Result<(), Error> {
    ctx.respond(abi::encode(&M::resource(ctx, key)?));
    Ok(())
}

/// The two wasm exports' bodies, over the host's calling convention: the
/// host writes an [`Invocation`](abi::Invocation) into memory `alloc` gave
/// it, and `call` answers a [`ProgramReply`](abi::ProgramReply) as `ptr << 32 | len`.
#[cfg(target_arch = "wasm32")]
pub mod exports {
    use abi::{Invocation, ProgramCall, ProgramReply, Refusal, reason};

    use super::{Program, access, execute, hook, query, resource};
    use crate::{ExecCtx, QueryCtx};

    pub fn alloc(len: u32) -> u32 {
        let mut buffer = Vec::<u8>::with_capacity(len as usize);
        let ptr = buffer.as_mut_ptr();
        core::mem::forget(buffer);
        ptr as u32
    }

    pub fn call<M: Program>(ptr: u32, len: u32) -> u64 {
        let request = unsafe { Vec::from_raw_parts(ptr as *mut u8, len as usize, len as usize) };
        let reply = match abi::decode::<Invocation>(&request) {
            Ok(invocation) => dispatch::<M>(invocation),
            Err(fault) => Err(Refusal::new(reason::PROTOCOL, fault.sentence)),
        };
        leak(abi::encode(&reply))
    }

    fn dispatch<M: Program>(Invocation { env, call }: Invocation) -> ProgramReply {
        let env = env.into();
        let reply = match call {
            ProgramCall::Init(params) => M::init(&ExecCtx::new(env), &params),
            ProgramCall::Execute(payload) => execute::<M>(&ExecCtx::new(env), &payload),
            ProgramCall::Query(request) => query::<M>(&QueryCtx::new(env), &request),
            ProgramCall::Access(payload) => access::<M>(&QueryCtx::new(env), &payload),
            ProgramCall::Resource(key) => resource::<M>(&QueryCtx::new(env), &key),
            ProgramCall::Hook(record) => hook::<M>(&ExecCtx::new(env), record),
            ProgramCall::Resume { .. } => Err(crate::Error::new(
                crate::code::UNSUPPORTED,
                "this program awaits nothing",
            )),
        };
        reply.map_err(crate::kernel::refusal_from)
    }

    pub(crate) fn leak(bytes: Vec<u8>) -> u64 {
        let len = bytes.len() as u64;
        let ptr = bytes.as_ptr() as u64;
        core::mem::forget(bytes);
        (ptr << 32) | len
    }
}

/// The program's two wasm exports, `alloc` and `call`, forwarding to
/// `exports::alloc` (wasm32 only) and `exports::call::<$program>`, where
/// the decoding, the error mapping and the response encoding live.
/// `program::export!(Counter);` expands to:
///
/// ```ignore
/// #[cfg(target_arch = "wasm32")]
/// #[unsafe(no_mangle)]
/// pub extern "C" fn alloc(len: u32) -> u32 {
///     program::exports::alloc(len)
/// }
///
/// #[cfg(target_arch = "wasm32")]
/// #[unsafe(no_mangle)]
/// pub extern "C" fn call(ptr: u32, len: u32) -> u64 {
///     program::exports::call::<Counter>(ptr, len)
/// }
/// ```
///
/// The exports are global symbols, so a wasm crate has one `export!`. A
/// program crate another crate links (forge links chat) puts its `export!`
/// behind a feature only its own wasm build turns on.
#[macro_export]
macro_rules! export {
    ($program:ty) => {
        #[cfg(target_arch = "wasm32")]
        #[unsafe(no_mangle)]
        pub extern "C" fn alloc(len: u32) -> u32 {
            $crate::exports::alloc(len)
        }

        #[cfg(target_arch = "wasm32")]
        #[unsafe(no_mangle)]
        pub extern "C" fn call(ptr: u32, len: u32) -> u64 {
            $crate::exports::call::<$program>(ptr, len)
        }
    };
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::{MockHost, code};

    #[test]
    fn a_program_id_has_one_spelling() {
        assert!(is_name("programs") && is_name("a-b_c9"));
        for name in ["", "a b", "System", "\u{0441}hat", "chat\u{202e}"] {
            assert!(!is_name(name), "{name:?}");
        }
        assert!(is_name(&"x".repeat(64)) && !is_name(&"x".repeat(65)));
    }

    /// A program whose op, a `u64`, never decodes from a reply's empty
    /// payload, and which keeps the replies it gets.
    struct Asker;

    impl Program for Asker {
        const NAME: &'static str = "asker";
        type Op = u64;
        type Query = ();
        type Reply = ();

        fn execute(_: &ExecCtx, _: u64) -> Result<(), Error> {
            Err(Error::new(code::WRONG_STATE, "a reply is not an op"))
        }

        fn reply(ctx: &ExecCtx, id: &MessageId, outcome: &Outcome) -> Result<(), Error> {
            ctx.put("reply", &(id, outcome));
            Ok(())
        }

        fn query(_: &QueryCtx, (): ()) -> Result<(), Error> {
            Ok(())
        }
    }

    #[test]
    fn a_reply_runs_reply_with_its_id_and_outcome_not_the_op() {
        let id = MessageId {
            program: "asker".into(),
            seq: 3,
        };
        let outcome = Outcome::Rejected(Error::new(code::NOT_FOUND, "no such thing"));
        let env = crate::Env {
            cause: Cause::Reply {
                id: id.clone(),
                outcome: outcome.clone(),
            },
            ..MockHost::env("asker").from_program("target", Some(7))
        };
        let host = MockHost::default();
        execute::<Asker>(&host.exec(env.clone()), &[]).unwrap();
        let kept: Option<(MessageId, Outcome)> = host.query(env.clone()).record("reply").unwrap();
        assert_eq!(kept, Some((id.clone(), outcome.clone())));
        // the default reply returns a refused message's refusal
        let Outcome::Rejected(refusal) = &outcome else {
            unreachable!()
        };
        assert_eq!(
            Silent::reply(&host.exec(env), &id, &outcome),
            Err(refusal.clone())
        );
    }

    /// Answers a query with bytes of its own, as forge serves git's wire:
    /// its `Reply` is never what the host gets.
    struct Raw;
    impl Program for Raw {
        const NAME: &'static str = "raw";
        type Op = ();
        type Query = ();
        type Reply = u64;

        fn execute(_: &ExecCtx, (): ()) -> Result<(), Error> {
            Ok(())
        }

        fn query(_: &QueryCtx, (): ()) -> Result<u64, Error> {
            Ok(7)
        }

        fn answer(_: &QueryCtx, (): ()) -> Result<Vec<u8>, Error> {
            Ok(b"git's own bytes".to_vec())
        }
    }

    #[test]
    fn a_query_is_answered_by_answer_not_by_querys_reply_as_borsh() {
        let host = MockHost::default();
        let request = abi::encode(&());
        query::<Raw>(&host.query(MockHost::env("raw")), &request).unwrap();
        assert_eq!(host.take_response(), b"git's own bytes");
        query::<Silent>(&host.query(MockHost::env("silent")), &request).unwrap();
        assert_eq!(host.take_response(), abi::encode(&()));
    }

    struct Tap;
    impl Program for Tap {
        const NAME: &'static str = "tap";
        type Op = ();
        type Query = ();
        type Reply = ();

        fn execute(_: &ExecCtx, (): ()) -> Result<(), Error> {
            Ok(())
        }

        fn query(_: &QueryCtx, (): ()) -> Result<(), Error> {
            Ok(())
        }

        fn hook(ctx: &ExecCtx, record: Record) -> Result<(), Error> {
            ctx.put("told", &record);
            Ok(())
        }
    }

    #[test]
    fn only_the_kernel_calls_a_hook_and_a_program_is_none_unless_it_says() {
        let record = Record {
            height: 4,
            position: 1,
            signer: vec![1; 32],
            account: Some(1),
            target: "chat".into(),
            applied: true,
        };
        let host = MockHost::default();
        for env in [
            MockHost::env("tap"),
            MockHost::env("tap").from_program("chat", Some(8)),
        ] {
            let refused = host.refused(|| hook::<Tap>(&host.exec(env), record.clone()));
            assert_eq!(refused.code, code::UNAUTHORIZED);
        }
        hook::<Tap>(&host.exec(MockHost::env("tap").root()), record.clone()).unwrap();
        let told: Option<Record> = host.query(MockHost::env("tap")).record("told").unwrap();
        assert_eq!(told, Some(record.clone()));
        let silent =
            host.refused(|| hook::<Silent>(&host.exec(MockHost::env("silent").root()), record));
        assert_eq!(silent.code, code::UNSUPPORTED);
    }

    /// Keeps [`Program::reply`]'s default.
    struct Silent;
    impl Program for Silent {
        const NAME: &'static str = "silent";
        type Op = ();
        type Query = ();
        type Reply = ();

        fn execute(_: &ExecCtx, (): ()) -> Result<(), Error> {
            Ok(())
        }

        fn query(_: &QueryCtx, (): ()) -> Result<(), Error> {
            Ok(())
        }
    }
}
