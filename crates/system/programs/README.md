# programs

the boot set's root: `programs`, the program that fills the programs role (its `Op`,
`Query`, `Reply`). It keeps the network's role table (`Query::Roles`): genesis fills it
from the roles each founding program names, and the validators' vote changes it by
publishing a program with the roles it fills (`Change::Publish`). Each block the kernel
asks `Query::At(height)` and runs what it answers, and on a roster change
`Query::Roles`; a client asks `Query::Roster`, what runs and is listed at the query's
own height. An update is a `Change::Set` of held code under a running id: the kernel
swaps the code and keeps the program's state and account, and `init` does not run again.
Each id, a program's or a view's, has at most one holder (`Query::Owner`): the
validators' vote for every id genesis founds and every id that ran, was listed or had a
change waiting before holders were recorded; for a free id, the first account to `Set`,
`Publish` or `SetView` it, from the block that change is scheduled. Only its holder or
the vote changes or cancels a change to an id, a removal keeps the holder, and `Own {
program, holder }`, sent by a holding account or the vote, hands it on to an account, a
list of accounts (any of which acts as the holder) or the vote. The types, rules and
`Programs` program are always built; a view links them with `program` off, and the
`program` feature adds the wasm exports. Its `tests/system.rs` founds ducktape's host
over the bytes `make wasm-programs` built and drives every system program
