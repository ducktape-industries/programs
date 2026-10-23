# state-proof

a QMDB op proof checked inside a program: `holds(root, location, proof, key, value)`
answers whether the proof shows that write at that location under that root, as
ducktape's `state` crate commits it, and `update(key, value)` is that write's op bytes.
It links commonware-storage's merkle verifier without `std`, so it builds for wasm32
with no imports. crosspond checks packets and validator sets with it
