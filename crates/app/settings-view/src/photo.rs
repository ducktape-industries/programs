//! The picture the reader picked, as the crop sheet holds it: decoded upright
//! (its EXIF orientation applied), the square they cut from it, and the
//! JPEG of that square their avatar is saved as, with no EXIF written.
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageDecoder, ImageReader, RgbImage, RgbaImage, imageops};
use std::io::Cursor;
use std::sync::Arc;
use view::{Image, ImageFormat};

// The saved picture's side, in pixels (orthory, #engineering 950).
pub(crate) const SIDE: u32 = 256;
// The crop frame's side on screen: one point for each saved pixel.
pub(crate) const FRAME: f32 = SIDE as f32;

pub(crate) struct Photo {
    upright: RgbaImage,
    // the whole picture at the frame's size for zoom 1, which layout moves
    // and scales: a drag or a zoom costs no pixels
    pub(crate) preview: Arc<Image>,
    pub(crate) crop: Crop,
}

impl Photo {
    // The picture in `bytes`, upright; `None` for bytes `image` reads as no
    // picture (a HEIC, a document).
    pub(crate) fn decode(bytes: &[u8]) -> Option<Self> {
        let reader = ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .ok()?;
        let mut decoder = reader.into_decoder().ok()?;
        let orientation = decoder.orientation().ok()?;
        let mut picture = DynamicImage::from_decoder(decoder).ok()?;
        picture.apply_orientation(orientation);
        let upright = picture.into_rgba8();
        let preview = jpeg(&shrunk(&upright, SIDE));
        Some(Self {
            crop: Crop::whole(upright.dimensions()),
            preview: Arc::new(Image::from_bytes(ImageFormat::Jpeg, preview)),
            upright,
        })
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        self.upright.dimensions()
    }

    // The square cut from the whole picture, at most `SIDE` across, as JPEG.
    pub(crate) fn saved(&self) -> Vec<u8> {
        let (x, y, side) = self.crop.square(self.size());
        let cut = imageops::crop_imm(&self.upright, x, y, side, side).to_image();
        jpeg(&shrunk(&cut, SIDE))
    }

    // The square moved as a drag of `by` points over the frame moves the
    // picture under it.
    pub(crate) fn dragged(&mut self, by: (f32, f32)) {
        let size = self.size();
        let scale = self.crop.side(size) / FRAME;
        let (x, y) = self.crop.centre;
        self.crop.centre = (x - by.0 * scale, y - by.1 * scale);
        self.crop = self.crop.held(size);
    }

    // Zoomed by `fraction` of the slider's travel: equal travel, equal
    // ratio of zoom.
    pub(crate) fn slid(&mut self, fraction: f32) {
        let size = self.size();
        let most = Crop::most(size);
        if most <= 1. {
            return;
        }
        let at = self.crop.zoom.ln() / most.ln() + fraction;
        self.crop.zoom = most.powf(at.clamp(0., 1.));
        self.crop = self.crop.held(size);
    }

    // Zoomed as a pinch of `delta` zooms (0.1: a tenth more).
    pub(crate) fn pinched(&mut self, delta: f32) {
        self.crop.zoom *= 1. + delta;
        self.crop = self.crop.held(self.size());
    }
}

// The square cut from a picture: at `zoom` 1 it is the picture's short side
// across, and `centre` is where its middle sits, in the picture's pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Crop {
    pub(crate) zoom: f32,
    pub(crate) centre: (f32, f32),
}

impl Crop {
    fn whole((width, height): (u32, u32)) -> Self {
        Self {
            zoom: 1.,
            centre: (width as f32 / 2., height as f32 / 2.),
        }
    }

    // The most a picture of `size` zooms: until the square is `SIDE` pixels
    // across, so a saved picture is never enlarged. One no wider than that
    // does not zoom.
    pub(crate) fn most((width, height): (u32, u32)) -> f32 {
        (width.min(height) as f32 / SIDE as f32).max(1.)
    }

    // The square's side, in the picture's pixels.
    pub(crate) fn side(&self, (width, height): (u32, u32)) -> f32 {
        width.min(height) as f32 / self.zoom
    }

    // The zoom within 1 and the most, and the square within the picture.
    pub(crate) fn held(self, size @ (width, height): (u32, u32)) -> Self {
        let zoom = self.zoom.clamp(1., Self::most(size));
        let half = width.min(height) as f32 / zoom / 2.;
        let within = |at: f32, length: u32| at.clamp(half, length as f32 - half);
        Self {
            zoom,
            centre: (within(self.centre.0, width), within(self.centre.1, height)),
        }
    }

    // The square in whole pixels, its corner and side, inside the picture.
    pub(crate) fn square(&self, size @ (width, height): (u32, u32)) -> (u32, u32, u32) {
        let side = (self.side(size).round() as u32).clamp(1, width.min(height));
        let corner = |at: f32, length: u32| {
            ((at - side as f32 / 2.).round().max(0.) as u32).min(length - side)
        };
        (
            corner(self.centre.0, width),
            corner(self.centre.1, height),
            side,
        )
    }
}

// `picture` made smaller in proportion until its short side is `side`; one
// already no bigger, as it is.
fn shrunk(picture: &RgbaImage, side: u32) -> RgbaImage {
    let (width, height) = picture.dimensions();
    let short = width.min(height);
    if short <= side {
        return picture.clone();
    }
    let scaled = |length: u32| (u64::from(length) * u64::from(side) / u64::from(short)) as u32;
    imageops::thumbnail(picture, scaled(width).max(1), scaled(height).max(1))
}

// `picture` as JPEG at the encoder's own quality; a JPEG holds no alpha.
fn jpeg(picture: &RgbaImage) -> Vec<u8> {
    let opaque: RgbImage = DynamicImage::ImageRgba8(picture.clone()).into_rgb8();
    let mut bytes = Vec::new();
    opaque
        .write_with_encoder(JpegEncoder::new(&mut bytes))
        .expect("a JPEG encodes into memory");
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDE: (u32, u32) = (600, 400);

    #[test]
    fn the_square_is_the_short_side_at_first_and_never_leaves_the_picture() {
        let whole = Crop::whole(WIDE);
        assert_eq!(whole.square(WIDE), (100, 0, 400));
        let wild = Crop {
            zoom: 9.,
            centre: (-5., 1e6),
        };
        let held = wild.held(WIDE);
        assert_eq!(held.zoom, 400. / 256.);
        assert_eq!(held.centre, (128., 272.));
        assert_eq!(held.square(WIDE), (0, 144, 256));
        assert_eq!(Crop::most((200, 120)), 1., "a small picture does not zoom");
    }

    #[test]
    fn a_drag_moves_the_picture_under_the_frame() {
        let mut photo = Photo::decode(&encoded(600, 400, None)).unwrap();
        photo.dragged((64., 0.));
        assert_eq!(photo.crop.centre, (200., 200.), "the square moved left");
        photo.dragged((-1e4, 0.));
        assert_eq!(photo.crop.centre, (400., 200.), "held at the edge");
        photo.slid(1.);
        assert_eq!(photo.crop.zoom, 400. / 256.);
        photo.pinched(-0.5);
        assert_eq!(photo.crop.zoom, 1., "no smaller than the short side");
    }

    #[test]
    fn a_photo_is_turned_upright_and_saved_with_no_exif() {
        let turned = encoded(40, 20, Some(6));
        assert!(contains(&turned, b"Exif"));
        let photo = Photo::decode(&turned).unwrap();
        assert_eq!(photo.size(), (20, 40), "orientation 6 is a quarter turn");
        let saved = photo.saved();
        assert!(!contains(&saved, b"Exif"));
        let read = image::load_from_memory(&saved).unwrap();
        assert_eq!((read.width(), read.height()), (20, 20));
        assert!(Photo::decode(b"not a picture").is_none());
    }

    fn contains(bytes: &[u8], part: &[u8]) -> bool {
        bytes.windows(part.len()).any(|window| window == part)
    }

    // A JPEG `width` by `height`, with an EXIF orientation when given: an
    // APP1 segment after the start of image holding one TIFF entry,
    // Orientation (0x0112), a SHORT.
    fn encoded(width: u32, height: u32, orientation: Option<u8>) -> Vec<u8> {
        let picture = RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([x as u8, y as u8, 0, 255])
        });
        let jpeg = jpeg(&picture);
        let Some(orientation) = orientation else {
            return jpeg;
        };
        let mut tiff = b"MM\0\x2a\0\0\0\x08\0\x01".to_vec();
        tiff.extend_from_slice(&[0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, orientation, 0, 0]);
        tiff.extend_from_slice(&[0; 4]);
        let mut segment = b"Exif\0\0".to_vec();
        segment.extend_from_slice(&tiff);
        let length = (segment.len() + 2) as u16;
        let mut turned = jpeg[..2].to_vec();
        turned.extend_from_slice(&[0xff, 0xe1]);
        turned.extend_from_slice(&length.to_be_bytes());
        turned.extend_from_slice(&segment);
        turned.extend_from_slice(&jpeg[2..]);
        turned
    }
}
