//! Writes tally's `ducktape.schema` and `ducktape.tools` sections into the folder it is given, for
//! tools/pack.sh.
fn main() -> std::process::ExitCode {
    borsh_json::tools::interface_main::<tally::Tally>()
}
