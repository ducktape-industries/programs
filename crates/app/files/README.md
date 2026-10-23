# files

a replicated filesystem as a program: a commit edits one tree of directories, files and
symlinks and names the result by a snapshot; each tree records, beside every entry, the
account number that first made it with `Put`, `Mkdir` or `Symlink` (and the directories
that write made); a move carries it with the entry and a remove drops it. Who writes a
path is the access program's to decide, with the path as the key: files answers a path's
owner, its creator in the head tree, and for a home folder `/home/<name>` the one
account the identity role names `<name>` (`Profiles`). `Put`, `Mkdir` and `Symlink`
write their path, `Remove` deletes it, `Move` writes both ends and moves the lists at
and below the old path to the new one (`access::Op::Move`), and `Unpin` deletes the
pin's key (`files::pin_key(name)`, owned by the account that made the pin). A program
that sends `Watch { deliver }` hears every commit, each as `deliver ++
borsh(SnapshotRow)` (an empty `deliver` stops it); its refusal never fails the commit. A
write is allowed by the nearest list at or above the path, which its owner sets with
`access::Op::Set` (`program::Ownership`: `Owner`, its creator alone; `Listed`, its
creator and the parties listed, each a `Party::Account` by number or a `Party::Program`
by id; `Anyone`), and with none by anyone, but for a file beneath a chat message
(`/chat/<channel>/<message>/…`), which with no list is its creator's alone, as the
message reads it where it stands; the owner of a folder above a path may also remove it.
A path its owner guards (a `files.path` row) needs the guard's ownership policy as well
as the access list while both are bound, and with no default of its owner's that policy
is their "Only me": harness's `files_set_list` sets both. An account matches a list by
its number; a program matches when the write is its message (`Origin::Program`),
whatever account it acts as, so a list naming harness allows every agent's write through
harness. A commit by an account at the namespace roots themselves, `/home`, `/shared`
and each `/home/<name>`, is refused; the chain's own writes are never checked. A list
only controls writing: every member holds every file. `Change::SetOwnership` is always
refused. A file's bytes are `chunk` blobs of at most `CHUNK_SIZE` (1 MiB), put through
the node's blob upload (a view's `op.put`) and held by the network before one commit
names them: no transaction carries a file's bytes; trees, files and snapshots are blobs
too, each held on its path's line (a node's line is `n` and its path's segments, each a
u32 BE length then its bytes, then `0x00`; the snapshot's is `s`), so a newer put there
lets the old version go and a remove or a move lets go of every line beneath it in one
prefix release (a move puts its subtree again on the lines where it lands). Its state
keeps the head, the history, a snapshot's place in it, the named pins, and for each
commit the paths it changed (`changed/`: each path it names and their ancestors), the
paths it names (`touched/`) and its own diff (`diff/<seq>`). A commit on a base is stale
when a path it touches changed since, or a folder above that path was itself written
since, read from those rows with no tree read; a base older than the first commit that
wrote them is stale. A stale refusal on a base files records names the newest commit
since that base that changed the path or wrote a folder above it: its snapshot,
sequence, author, block and message, read from those rows and never from old file
bytes. A commit with no base that finds the path already there, and a commit on a base
older than the records, are refused naming no commit. `HoldHead { after }` puts one folder of the head (the first after
`after` in path order; the root and the snapshot when none) and each file object in it
on their lines, then books the next call a block later, so the one-time step for objects
put before files held them there runs as a chain of bounded calls. Reads (stat, list,
read, find, grep) take the head or any snapshot it minted, and an object a newer commit
let go answers `not_found`, pruned on this node; a diff folds the recorded `diff/` rows,
where a removed or moved folder is one entry. `files-view` browses it: a folder at the
head (a folder or file read again from the head when a later read of its snapshot
answers pruned, as harness-view and chat-view read a file's chunks), a file's text (a
`.md` file drawn as its document by the SDK's `markdown`, its relative links opening the
files they name), the history and its pins, who writes a path (the access program's
`Writers`), and the writes the reader may make there (a new folder or a new text file,
an edit, a rename or move, a removal), each one commit with a message; a new folder or
file may carry a write list of its own, which files-view sets in the access program once
the commit lands
