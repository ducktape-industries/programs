//! The image element: a picture the view holds, lowered once per content
//! hash, or one the host reads from the network's blobs (`blob_img`).
use crate::interactivity::{
    InteractiveElement, Interactivity, Stateful, StatefulInteractiveElement,
};
use crate::{AnyElement, Element, IntoElement, Lowering, wire};
use gpui::{
    ImageSource as GpuiImageSource, ObjectFit, RenderImage, Resource, StyleRefinement, Styled,
};
use std::sync::Arc;

pub use gpui::ImageSource;

pub struct ImageStyle {
    grayscale: bool,
    object_fit: ObjectFit,
    fallback: Option<Box<dyn Fn() -> AnyElement>>,
    loading: Option<Box<dyn Fn() -> AnyElement>>,
}

impl Default for ImageStyle {
    fn default() -> Self {
        Self {
            grayscale: false,
            object_fit: ObjectFit::Contain,
            fallback: None,
            loading: None,
        }
    }
}

pub trait StyledImage: Sized {
    fn image_style(&mut self) -> &mut ImageStyle;
    fn grayscale(mut self, value: bool) -> Self {
        self.image_style().grayscale = value;
        self
    }
    fn object_fit(mut self, value: ObjectFit) -> Self {
        self.image_style().object_fit = value;
        self
    }
    fn with_fallback(mut self, fallback: impl Fn() -> AnyElement + 'static) -> Self {
        self.image_style().fallback = Some(Box::new(fallback));
        self
    }
    fn with_loading(mut self, loading: impl Fn() -> AnyElement + 'static) -> Self {
        self.image_style().loading = Some(Box::new(loading));
        self
    }
}

enum Source {
    Held(ImageSource),
    Blobs(Vec<String>),
}

pub struct Img {
    pub(crate) interactivity: Box<Interactivity>,
    source: Source,
    image_style: ImageStyle,
}

#[track_caller]
pub fn img(source: impl Into<ImageSource>) -> Img {
    Img {
        interactivity: Box::default(),
        source: Source::Held(source.into()),
        image_style: ImageStyle::default(),
    }
}

#[track_caller]
pub fn blob_img(ids: Vec<String>) -> Img {
    Img {
        interactivity: Box::default(),
        source: Source::Blobs(ids),
        image_style: ImageStyle::default(),
    }
}

impl Img {
    pub fn extensions() -> &'static [&'static str] {
        &[
            "avif", "jpg", "jpeg", "png", "gif", "webp", "tif", "tiff", "tga", "dds", "bmp", "ico",
            "hdr", "exr", "pbm", "pam", "ppm", "pgm", "ff", "farbfeld", "qoi", "svg",
        ]
    }
}

impl Styled for Img {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.interactivity.base_style
    }
}
impl StyledImage for Img {
    fn image_style(&mut self) -> &mut ImageStyle {
        &mut self.image_style
    }
}
impl StyledImage for Stateful<Img> {
    fn image_style(&mut self) -> &mut ImageStyle {
        &mut self.element.image_style
    }
}
impl InteractiveElement for Img {
    fn interactivity(&mut self) -> &mut Interactivity {
        &mut self.interactivity
    }
}
impl StatefulInteractiveElement for Img {}

impl Element for Img {
    fn id(&self) -> Option<gpui::ElementId> {
        self.interactivity.id.clone()
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        let Self {
            interactivity,
            source,
            image_style,
        } = *self;
        let style = lowering.style(&interactivity.base_style);
        let label = interactivity
            .interactions
            .as_ref()
            .and_then(|interactions| interactions.aria.label.as_ref())
            .map(ToString::to_string);
        let id = interactivity.id.as_ref().map(|_| lowering.own_id());
        let interactivity = interactivity.into_wire(lowering);
        let (hash, data) = match source {
            Source::Held(source) => image_data(source, lowering),
            Source::Blobs(ids) => blobs_data(ids, lowering),
        };
        let loading = image_style.loading.is_some();
        let fallback = image_style.fallback.is_some();
        let mut state_children = Vec::with_capacity(usize::from(loading) + usize::from(fallback));
        if let Some(child) = image_style.loading {
            state_children.push(lowering.lower_element(child()));
        }
        if let Some(child) = image_style.fallback {
            state_children.push(lowering.lower_element(child()));
        }
        wire::Node::Image {
            id,
            hash,
            data,
            label,
            image_style: wire::ImageStyle {
                grayscale: image_style.grayscale,
                object_fit: image_style.object_fit.into(),
            },
            loading,
            fallback,
            state_children,
            style,
            interactivity,
        }
    }
}

impl IntoElement for Img {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

fn image_data(source: ImageSource, lowering: &Lowering<'_>) -> (u64, Option<wire::ImageData>) {
    match source {
        GpuiImageSource::Image(image) => {
            let (hash, bytes) = lowering.image(&image);
            (hash, bytes.map(wire::ImageData::Encoded))
        }
        GpuiImageSource::Render(image) => render_image_data(&image, lowering),
        GpuiImageSource::Resource(resource) => refusal(match resource {
            Resource::Embedded(path) => format!("host asset unavailable: {path}"),
            Resource::Path(_) => "view image path refused".into(),
            Resource::Uri(_) => "view image URI refused".into(),
        }),
        GpuiImageSource::Custom(_) => {
            refusal("custom image loader cannot cross the view boundary".into())
        }
    }
}

fn blobs_data(ids: Vec<String>, lowering: &Lowering<'_>) -> (u64, Option<wire::ImageData>) {
    let (hash, first) = lowering.picture(format!("blobs\n{}", ids.join("\n")));
    (hash, first.map(|_| wire::ImageData::Blobs(ids)))
}

fn render_image_data(
    image: &Arc<RenderImage>,
    lowering: &Lowering<'_>,
) -> (u64, Option<wire::ImageData>) {
    match image.frame_count() {
        0 => refusal("render image has no frames".into()),
        1 => {
            let size = image.size(0);
            let Some(pixels) = image.as_bytes(0) else {
                return refusal("render image frame has no pixels".into());
            };
            let mut content = Vec::with_capacity(pixels.len() + 8);
            content.extend_from_slice(&u32::from(size.width).to_le_bytes());
            content.extend_from_slice(&u32::from(size.height).to_le_bytes());
            content.extend_from_slice(pixels);
            let (hash, first) = lowering.picture(content);
            (
                hash,
                first.map(|content| wire::ImageData::Rgba {
                    width: u32::from(size.width),
                    height: u32::from(size.height),
                    pixels: content[8..].to_vec(),
                }),
            )
        }
        count => refusal(format!(
            "animated render image with {count} frames is unsupported"
        )),
    }
}

fn refusal(reason: String) -> (u64, Option<wire::ImageData>) {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    reason.hash(&mut hasher);
    (hasher.finish(), Some(wire::ImageData::Refusal(reason)))
}

impl gpui::prelude::FluentBuilder for Img {}
