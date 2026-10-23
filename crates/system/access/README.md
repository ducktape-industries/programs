# access

the program that fills the access role: who may write each key another program names.
Before every op it executes, the kernel asks the target program what the op needs
(`Program::access`: a `Need::write(key)` or `Need::delete(key)` per key) and, with any
need, runs this program's `Op::Check { program, actor, needs }` (the kernel's alone). A
program keeps its owners itself and answers a key's owner and default list from what it
stores (`Program::resource`: `Resource { owner, default }`); this program keeps only the
lists set on keys (`LISTS`): `Op::Set { program, key, list }`, by the key's owner or the
program itself, and `Op::Move { program, from, to }`, by the program alone, when a path
is renamed. A write is allowed by the nearest list at or above the key (its
`/`-separated ancestors, the key first), else that key's default, else anyone; a delete
also by the owner of any key above it, and drops the lists at and below it. Queries:
`List`, `Resource`, `Writers`, `Lists { program, page }`.
[roles.md](../../../docs/roles.md), "Who may write a key", has each program's keys
