# activity

the hook the kernel tells of every frame a block carries
([roles.md](../../../docs/roles.md), "Hooks"): it keeps each frame (its place, time,
signer, target and whether it applied) newest first network-wide (`Frames`), by the key
that signed it (`Signed`) and by the account it acted as (`Acted`), and counts each
account's frames by program (`Used`). It takes no op. explorer-view's transaction lists
and both views' lists of what an account did are its frames: a row is the program,
whether it applied, its place and its age, and no transaction is read to draw it
