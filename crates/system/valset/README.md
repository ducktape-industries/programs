# valset

the other boot programs, the same shape: types always built, the wasm exports behind
`program`. valset holds the members and runs their rules: enroll (by invite or an open
door, with the halo address other members dial), a member moving its halo address,
leave, and the validators' vote on members, the door, and an op of the program that
fills a role (`Motion::Role`: programs changes, which include publishing a program with
the roles it fills, and halo's lanes).

Only member-signed `ClearHalo` marks that member without a direct halo address. It
preserves its registered halo and membership; `HaloCleared` answers the marked keys
in key order for current core's node and egress seating. `SetHalo`, re-enrollment,
leaving and removal erase the choice. A core pin or program Set opts no member in.
