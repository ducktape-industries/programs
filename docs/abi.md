# Kernel ABI

the borsh bytes ABI a program and the host share: `ProgramCall`, `HostOp`/`HostReply`,
`Env`, `Refusal`, `Principal`, `Roles`, and the `role::programs`, `role::validators`,
`role::identity`, `role::awaits`, `role::halo`, `role::scheduler`, `role::access` and
`role::ownership` interfaces the kernel calls (`role::access` also holds `Party`,
`Ownership`, `Need`, `Resource`, `Writers` and the `writers` rule over a key's
`/`-separated ancestors), the `role::egress` interface the node reads, the
`role::crosspond` interface a linked network's relay speaks (the blobs role's contract
is ducktape's `blob::contract`, which programs link), under the kernel's names. Not in
this repo: the workspace takes it from ducktape by git, at the rev it pins the host to,
so the host and every program share one abi
