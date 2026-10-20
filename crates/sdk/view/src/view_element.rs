use crate::element::{AnyElement, Element, IntoElement, Lowering, RenderOnce};
use crate::wire;

type ViewRenderer = Box<dyn FnOnce(&mut crate::Window, &mut crate::App) -> AnyElement>;

/// Any entity that renders, type-erased, as native GPUI's `AnyView`: a
/// child entity in its parent's tree, or a tooltip's content.
pub struct AnyView {
    render: ViewRenderer,
    entity: u64,
    cached: bool,
}

impl<V: crate::Render> From<crate::Entity<V>> for AnyView {
    fn from(entity: crate::Entity<V>) -> Self {
        let id = entity.id;
        Self {
            render: Box::new(move |window, app| {
                entity.update_in_window(app, window, |view, window, cx| {
                    view.render(window, cx).into_any_element()
                })
            }),
            entity: id,
            cached: false,
        }
    }
}

impl AnyView {
    // Kept as it was last lowered while its entity, and every entity drawn
    // inside it, has not notified: gpui's `cached`. Its render reads only
    // its own state and what it observes, and its root has an id; a debug
    // build lowers each reuse again and fails where the two differ.
    pub fn cached(mut self) -> Self {
        self.cached = true;
        self
    }
}

impl<V: crate::Render> IntoElement for crate::Entity<V> {
    type Element = ViewElement<AnyView>;

    fn into_element(self) -> Self::Element {
        AnyView::from(self).into_element()
    }
}

impl RenderOnce for AnyView {
    fn render(self, window: &mut crate::Window, cx: &mut crate::App) -> impl IntoElement {
        let entity = self.entity;
        let inner = match self.cached {
            true => {
                let version = cx.inner.memos.borrow().version(entity);
                crate::memo::Memo {
                    id: crate::memo::MemoId::Entity(entity),
                    deps: Box::new(version),
                    same: crate::memo::same::<u64>,
                    build: self.render,
                }
                .into_any_element()
            }
            false => (self.render)(window, cx),
        };
        Entered { entity, inner }
    }
}

struct Entered {
    entity: u64,
    inner: AnyElement,
}

impl Element for Entered {
    fn defers(&self) -> bool {
        true
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        let Self { entity, inner } = *self;
        lowering.entity(entity, |lowering| lowering.lower_element(inner))
    }
}

impl IntoElement for Entered {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl IntoElement for AnyView {
    type Element = ViewElement<Self>;

    fn into_element(self) -> Self::Element {
        ViewElement::new(self)
    }
}

/// The view counterpart of GPUI's `ViewElement`: it defers a `RenderOnce`
/// component until the lowering pass reaches this element.
#[doc(hidden)]
pub struct ViewElement<V: RenderOnce> {
    view: V,
}

impl<V: RenderOnce> ViewElement<V> {
    #[track_caller]
    pub fn new(view: V) -> Self {
        Self { view }
    }
}

impl<V: RenderOnce> Element for ViewElement<V> {
    fn defers(&self) -> bool {
        true
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        lowering.render_once(self.view)
    }
}

impl<V: RenderOnce> IntoElement for ViewElement<V> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<V: RenderOnce> gpui::prelude::FluentBuilder for ViewElement<V> {}
