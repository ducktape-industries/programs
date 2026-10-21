//! An image node's picture on the wire: its bytes, its pixels, a host key,
//! a refusal, or the network's blobs the host reads it from.
use serde::{Deserialize, Serialize};

/// Copied raster data or an opaque host image resource. Native allocations never cross.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ImageData {
    Encoded(
        #[serde(
            serialize_with = "crate::codec::bin::serialize",
            deserialize_with = "crate::codec::bin::deserialize"
        )]
        Vec<u8>,
    ),
    Rgba {
        width: u32,
        height: u32,
        #[serde(
            serialize_with = "crate::codec::bin::serialize",
            deserialize_with = "crate::codec::bin::deserialize"
        )]
        pixels: Vec<u8>,
    },
    /// A host-issued image key, resolved at paint time rather than cached pixels.
    /// This is neither a filesystem path nor a URL.
    Resource(String),
    /// A visible refusal produced before a native resource can be accessed.
    Refusal(String),
    Blobs(Vec<String>),
}

fn blob_id(id: &str) -> bool {
    id.strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

impl ImageData {
    pub fn byte_len(&self) -> usize {
        match self {
            Self::Resource(key) | Self::Refusal(key) => key.len(),
            Self::Encoded(bytes) => bytes.len(),
            Self::Rgba { pixels, .. } => pixels.len(),
            Self::Blobs(ids) => ids.iter().map(String::len).sum(),
        }
    }

    pub fn valid_rgba(&self) -> bool {
        match self {
            Self::Resource(key) | Self::Refusal(key) => !key.is_empty() && !key.contains('\0'),
            Self::Encoded(_) => true,
            Self::Blobs(ids) => !ids.is_empty() && ids.iter().all(|id| blob_id(id)),
            Self::Rgba {
                width,
                height,
                pixels,
            } => {
                *width != 0
                    && *height != 0
                    && u64::from(*width)
                        .checked_mul(u64::from(*height))
                        .and_then(|count| count.checked_mul(4))
                        == Some(pixels.len() as u64)
            }
        }
    }

    pub(crate) fn sanitize(data: &mut Option<Self>) {
        let invalid = data
            .as_ref()
            .is_some_and(|value| !matches!(value, Self::Refusal(_)) && !value.valid_rgba());
        if invalid {
            *data = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_rgba_is_dropped() {
        let mut invalid = Some(ImageData::Rgba {
            width: u32::MAX,
            height: u32::MAX,
            pixels: vec![0; 4],
        });
        ImageData::sanitize(&mut invalid);
        assert_eq!(invalid, None);
        let mut valid = Some(ImageData::Rgba {
            width: 1,
            height: 1,
            pixels: vec![255; 4],
        });
        ImageData::sanitize(&mut valid);
        assert!(valid.is_some());
    }

    #[test]
    fn blobs_are_kept_only_as_sha256_ids() {
        let id = format!("sha256:{}", "ab".repeat(32));
        let mut kept = Some(ImageData::Blobs(vec![id.clone(), id.clone()]));
        ImageData::sanitize(&mut kept);
        assert_eq!(kept, Some(ImageData::Blobs(vec![id.clone(), id.clone()])));
        for ids in [
            vec![],
            vec![id.clone(), "sha256:ab".into()],
            vec![format!("sha1:{}", "ab".repeat(20))],
            vec![format!("sha256:{}", "zz".repeat(32))],
        ] {
            let mut dropped = Some(ImageData::Blobs(ids.clone()));
            ImageData::sanitize(&mut dropped);
            assert_eq!(dropped, None, "{ids:?}");
        }
    }
}
