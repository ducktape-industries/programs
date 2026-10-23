# ownership

the ownership role: who may write each owned key a program holds. The kernel asks it
once per run that wrote owned keys (`role::ownership::Op::Guard`); it records each new
key's owner and the policy its owner's defaults choose, and admits a change or delete
only where the key's policy admits the actor. Its owners' ops: `Create`, `Edit`,
`Retire`, `Switch`, `Relink`, `Transfer`, `SetDefault`; and, one-time (J-011 ⑥),
`Adopt`, which a program sends to record owners for the owned rows it kept before the
guard (its `AdoptOwners`), each on the policy matching what the access program enforces
for it today
