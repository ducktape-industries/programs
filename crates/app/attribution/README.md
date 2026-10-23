# attribution

which accounts each program's objects relate to, and every change to that. A program
reports an object of its own (`Attribute`: its relations as of a revision that only
grows; a mention, an author, an owner, an assignee, or a reason it names) and
attribution records what was added, withdrawn or transferred, indexed by recipient and
by object. Each change is handed to one program at most: when its recipient is an active
program account whose script names a program (identity's `SetScript`), and that program
registered (`Register`, with the prefix of its own `Op` attribution puts before the
change), attribution hands it the change as it is recorded; its refusal changes nothing.
A change about any other account is recorded and handed to no one, and is read back by
recipient (`ChangesTo`), by object (`ChangesOf`) or one at a time by its seq (`Change {
seq }`, `null` for a seq no change has): something off the network that follows
attribution's `to/<recipient>/<seq>` rows reads each change this way. chat reports each
message's mentions, in a direct message the counterpart (`chat::attributed::DIRECT`),
and in a thread reply everyone who took part in the thread before it
(`chat::attributed::THREAD`)
