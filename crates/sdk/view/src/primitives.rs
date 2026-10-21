//! The primitive elements a view draws with: anchored and deferred layers,
//! canvases, images and vector pictures.
mod anchored;
mod canvas;
mod deferred;
mod img;
mod svg;

pub use anchored::Anchored;
pub use anchored::anchored;
pub use canvas::Canvas;
pub use canvas::canvas;
pub use deferred::Deferred;
pub use deferred::deferred;
pub use img::{ImageSource, ImageStyle, Img, StyledImage, blob_img, img};
pub use svg::{Svg, Transformation, svg};
