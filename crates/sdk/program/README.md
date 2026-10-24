# program

the program SDK, enough alone: the one `Program` trait (a program's `NAME`, the borsh
`Op`, `Query` and `Reply` it speaks, what it does with each, and what each op needs of
the access program and who owns a key, `access` and `resource`; a view names the program
by that type, `Query<chat::Chat>`, and `view-wire` re-exports it), `Ask` and `Reads`
(one question to a program as its own type, and the tables it reads), the `ExecCtx` and
`QueryCtx` contexts its entry points receive (env, raw state, blobs, events,
`set_return_data`, `verify`, and `emit` (a write another program runs in this frame) and
`query` (a read of one)), `ExecCtx::sender` (the `Principal` the host resolved: an
account, a program's too, or `Root`), `Env::actor` (the parties the access program
judges: the sender's account and, for a message, the sending program),
`ctx.resource(program, key)` (another program's owner of a key), `Env.roles` (the
program that fills each role, as the programs role answers), the `Env` origin checks
(`signer`, `sending_program`, `sent_by`) and `authority` (a message from the program
bound to the validators role), `export!`, `Awaiting` and `export_awaiting!` (an `async
fn execute(ctx: Ctx, op)` whose `ctx.call(target, &op).await` waits, parked between
blocks when it must, for the target's answer; `Call::ticket` starts that same call and
returns its ticket so its caller can release a ticket-scoped reply line), `ctx.schedule(height, target, &op)` and
`ctx.cancel(entry)` (a run of `target` at a later height, through the scheduler role,
and its cancel), `defer` and `resolve` (how a called program answers later), `Error` and
its constructors and `decoded` and `MockHost`, the native host the same contexts run
over in a test, and `MockChain`, which seats several programs over their own `MockHost`s
and runs a submission as one frame, its messages and replies as the kernel runs them,
and the scheduler role's due entries at the end of each block `at` moves past.
`src/kernel.rs` is the one place the kernel's names (`Refusal`, `ProgramId`, `ItemRef`,
`Scan`, …) become the SDK's (`Error`, `ProgramId`, `MessageId`, `Range`, …), byte for
byte; `kernel::error_from`/`refusal_from` convert an error. `examples/counter.rs` is a
program written with it alone

`MockChain` keeps preparation tickets separately from ordinary reads. `preparations`,
`answer_preparation`, `ready_preparation` and `refuse_preparation` let a test drive the
pure byte preparation lifecycle explicitly; ordinary `answer_reads` is unchanged.
