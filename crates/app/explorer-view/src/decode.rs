//! A transaction's payload as the operation it names, and the few formats
//! a chain reads in: short hashes, grouped numbers, dates and ages.
//!
//! A payload is described by the program it targets, through the host
//! (`program.describe`: the describe module the program's own code
//! carries). A program with none, or bytes it cannot read, reads as its
//! size and a short hex preview: [`bytes`].
use view::prelude::*;

const MAX_VALUE: usize = 160;

/// An op no describe module read: its program, its size, its first bytes.
pub fn bytes(program: &str, payload: &[u8]) -> Description {
    Description {
        title: sized(program, payload.len() as u64),
        fields: vec![Field {
            label: "bytes".into(),
            value: Value::bytes(payload),
        }],
    }
}

pub fn sized(program: &str, size: u64) -> String {
    format!("{program} · {}", plural(size, "byte", "bytes"))
}

/// Bytes as a person reads a size: `12 bytes`, `450 KB`, `1.8 MB`.
pub fn size(bytes: u64) -> String {
    match bytes {
        0..=1023 => plural(bytes, "byte", "bytes"),
        1024..=1_048_575 => format!("{} KB", bytes / 1024),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

/// The bytes a preview shows whole, as a describe module keeps them
/// (`Value::bytes`): a longer run shows its length and its ends.
pub const PREVIEW: usize = 32;

pub fn preview(len: u64, preview: &[u8]) -> String {
    match len {
        0 => "0 bytes".into(),
        len if len <= PREVIEW as u64 => short_hex(&abi::hex(preview)),
        _ => format!("{len} bytes · {}", short_hex(&abi::hex(preview))),
    }
}

/// Bytes held whole (a program's params) as [`preview`] shows them: the
/// first [`PREVIEW`] of them stand for the run.
pub fn bytes_preview(bytes: &[u8]) -> String {
    preview(bytes.len() as u64, &bytes[..bytes.len().min(PREVIEW)])
}

/// `value / 10^decimals`, the whole part grouped.
pub fn amount(value: u128, decimals: u8) -> String {
    let Some(scale) = 10u128.checked_pow(u32::from(decimals)) else {
        return value.to_string();
    };
    let whole = value / scale;
    let whole = match u64::try_from(whole) {
        Ok(whole) => grouped(whole),
        Err(_) => whole.to_string(),
    };
    match decimals {
        0 => whole,
        _ => format!(
            "{whole}.{:0width$}",
            value % scale,
            width = decimals as usize
        ),
    }
}

pub fn clip(text: &str) -> String {
    match text.char_indices().nth(MAX_VALUE) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_string(),
    }
}

/// A hash or key as [`short_hex`] shows it.
pub fn short(raw: &[u8]) -> String {
    short_hex(&abi::hex(raw))
}

pub use view::design::{ago, date, grouped, plural, short_hex};

/// How long `blocks` take at the node's block time, in whole minutes.
pub fn minutes(blocks: u64, block_time_ms: u64) -> u64 {
    (blocks * block_time_ms).div_ceil(60_000)
}

/// A signing scheme by its short name: `ed25519`, `p256`.
pub fn scheme(scheme: abi::Scheme) -> String {
    match scheme {
        abi::Scheme::Secp256r1 => "p256".into(),
        other => format!("{other:?}").to_lowercase(),
    }
}
