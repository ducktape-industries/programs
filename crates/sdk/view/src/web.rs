//! The web piece: a page the host's browser engine draws ([`wire::Node::Web`]),
//! for a view that declares [`Capability::Web`](crate::methods::Capability::Web);
//! and the artifact piece ([`wire::Node::Artifact`]), for one that declares `artifact`.
use crate::element::wire_id;
use crate::interactivity::EventListener;
use crate::revised::Revised;
use crate::{App, Element, ElementId, IntoElement, Lowering, Window, wire};
use gpui::{StyleRefinement, Styled};

/// What a [`web`] page tells its view: its address now, its title,
/// whether it loads, whether it can go back or forward.
pub use wire::WebState;

/// The navigations a view asks of a [`web`] page, kept until the frame
/// that carries them: give it to the piece with [`Web::track`], call
/// `load`, `back`, `forward`, `reload` or `stop`, and notify. Each
/// reaches the page once.
#[derive(Clone, Default)]
pub struct WebHandle(std::rc::Rc<std::cell::RefCell<Revised<wire::WebCommand>>>);

impl WebHandle {
    pub fn new() -> Self {
        Self::default()
    }

    fn push(&self, command: wire::WebCommand) {
        self.0.borrow_mut().push(command);
    }

    /// Opens `url`. The host loads only an `https://` page, or an
    /// `http://` one on this machine (`localhost`, a loopback address),
    /// with no user name or password in it.
    pub fn load(&self, url: impl Into<String>) {
        self.push(wire::WebCommand::Load(url.into()));
    }
    pub fn back(&self) {
        self.push(wire::WebCommand::Back);
    }
    pub fn forward(&self) {
        self.push(wire::WebCommand::Forward);
    }
    pub fn reload(&self) {
        self.push(wire::WebCommand::Reload);
    }
    pub fn stop(&self) {
        self.push(wire::WebCommand::Stop);
    }
}

/// A web page the host draws in this element's box, opened on `url` the
/// first time the host sees the element under `id`, and closed when a frame
/// no longer has it. The page is the reader's to press, type and scroll
/// in, so the element takes no listeners but [`Web::on_state`]. Only a
/// view whose manifest declares [`Capability::Web`](crate::methods::Capability::Web)
/// may draw one: the host refuses any other's frame.
///
/// ```
/// # use view::prelude::*;
/// let nav = WebHandle::new();
/// let page = web("page", "https://example.com")
///     .track(&nav)
///     .on_state(|state: &WebState, _, _| println!("{}", state.url))
///     .size_full();
/// nav.back();
/// ```
pub struct Web {
    id: ElementId,
    url: String,
    handle: Option<WebHandle>,
    on_state: Option<EventListener<WebState>>,
    style: Box<StyleRefinement>,
}

pub fn web(id: impl Into<ElementId>, url: impl Into<String>) -> Web {
    Web {
        id: id.into(),
        url: url.into(),
        handle: None,
        on_state: None,
        style: Box::default(),
    }
}

impl Web {
    /// The page takes its navigations from `handle`.
    pub fn track(mut self, handle: &WebHandle) -> Self {
        self.handle = Some(handle.clone());
        self
    }

    /// Hears where the page is: when it first reports, and whenever its
    /// address, title, loading or back and forward change.
    pub fn on_state(
        mut self,
        listener: impl Fn(&WebState, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_state = Some(Box::new(listener));
        self
    }
}

impl IntoElement for Web {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Web {
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        let (revision, commands) = self
            .handle
            .map(|handle| handle.0.borrow_mut().take())
            .unwrap_or_default();
        wire::Node::Web {
            id: wire_id(self.id),
            url: self.url,
            style: lowering.style(&self.style),
            revision,
            commands,
            on_state: self
                .on_state
                .map(|listener| lowering.route(crate::slots::Kind::WebState, listener)),
        }
    }
}

impl Styled for Web {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl gpui::prelude::FluentBuilder for Web {}

// An artifact the app draws at `link` (`duck://<chain>/files/<path>`, which
// it reloads when the file changes, or `duck://<chain>/blobs/<hex>`): the
// page, as tall as its content, where the app has a web view; the link
// alone where it has none. The app loads and reloads it, so the element
// takes no listeners. Only a view whose manifest declares
// `Capability::Artifact` may draw one: the host refuses any other's frame.
pub struct Artifact {
    id: ElementId,
    link: String,
    style: Box<StyleRefinement>,
}

pub fn artifact(id: impl Into<ElementId>, link: impl Into<String>) -> Artifact {
    Artifact {
        id: id.into(),
        link: link.into(),
        style: Box::default(),
    }
}

impl IntoElement for Artifact {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Artifact {
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        wire::Node::Artifact {
            id: wire_id(self.id),
            link: self.link,
            style: lowering.style(&self.style),
        }
    }
}

impl Styled for Artifact {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl gpui::prelude::FluentBuilder for Artifact {}

impl crate::testing::TestAppContext {
    /// The page of the web piece `name` reports `state`, as the app tells
    /// it when the page moves (a link followed, a load done, a title set).
    pub fn simulate_web_state(&mut self, name: &str, state: WebState) {
        let wire::Node::Web {
            on_state: Some(handler),
            ..
        } = self.node(name)
        else {
            panic!("{name:?} is no web piece with a state route");
        };
        let handler = *handler;
        self.simulate_event(wire::Event::Web { handler, state });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::methods::Capability;
    use crate::prelude::*;
    use crate::testing::TestAppContext;
    use crate::{View, div};
    #[derive(Default, serde::Deserialize, serde::Serialize)]
    struct WebView {
        #[serde(skip)]
        nav: WebHandle,
        heard: Vec<String>,
    }

    impl View for WebView {
        const NAME: &'static str = "WebView";
        const CAPABILITIES: &'static [Capability] = &[Capability::Web];
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self::default()
        }
    }

    impl Render for WebView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            web("page", "https://example.com/")
                .track(&self.nav)
                .on_state(cx.listener(|view, state: &WebState, _, cx| {
                    view.heard.push(state.url.clone());
                    cx.notify();
                }))
                .size_full()
        }
    }

    /// A web piece carries its first address and a state route; a navigation
    /// crosses once, under a revision the host has not seen, and the page's
    /// state reaches the view's listener.
    #[test]
    fn a_web_piece_carries_its_navigations_once_and_hears_its_page() {
        let mut cx = TestAppContext::new();
        let view = cx.open::<WebView>();
        let wire::Node::Web {
            url,
            revision,
            commands,
            on_state: Some(_),
            ..
        } = cx.node("page").clone()
        else {
            panic!("no web piece with a state route: {:?}", cx.root());
        };
        assert_eq!(url, "https://example.com/");
        assert!(commands.is_empty());
        cx.update(&view, |view, _, cx| {
            view.nav.load("https://example.org/");
            view.nav.back();
            cx.notify();
        });
        cx.run_until_parked();
        let wire::Node::Web {
            revision: moved,
            commands,
            ..
        } = cx.node("page").clone()
        else {
            unreachable!()
        };
        assert_eq!(moved, revision + 2);
        assert_eq!(
            commands,
            vec![
                wire::WebCommand::Load("https://example.org/".into()),
                wire::WebCommand::Back
            ]
        );
        cx.simulate_web_state(
            "page",
            WebState {
                url: "https://example.org/a".into(),
                ..Default::default()
            },
        );
        // the commands went with the frame that carried them
        let wire::Node::Web {
            revision: kept,
            commands,
            ..
        } = cx.node("page").clone()
        else {
            unreachable!()
        };
        assert_eq!(kept, moved);
        assert!(commands.is_empty());
        cx.update(&view, |view, _, _| {
            assert_eq!(view.heard, ["https://example.org/a"]);
        });
    }

    /// Draws a web piece, which its manifest leaves out.
    #[derive(Default, serde::Deserialize, serde::Serialize)]
    struct Undeclared;

    impl View for Undeclared {
        const NAME: &'static str = "Undeclared";
        const CAPABILITIES: &'static [Capability] = &[Capability::Host];
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self
        }
    }

    impl Render for Undeclared {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().child(web("page", "https://example.com/"))
        }
    }

    #[test]
    #[should_panic(expected = "undeclared_capability")]
    fn a_web_piece_the_manifest_leaves_out_fails_the_test() {
        TestAppContext::new().open::<Undeclared>();
    }

    const PLAN: &str = "duck://dognet-b5b6ea90/files/shared/plan.html";

    // Embeds an artifact, under the capability named by `ARTIFACT`.
    #[derive(Default, serde::Deserialize, serde::Serialize)]
    struct Embeds<const ARTIFACT: bool>;

    impl<const ARTIFACT: bool> View for Embeds<ARTIFACT> {
        const NAME: &'static str = "Embeds";
        const CAPABILITIES: &'static [Capability] = match ARTIFACT {
            true => &[Capability::Artifact],
            false => &[Capability::Web],
        };
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self
        }
    }

    impl<const ARTIFACT: bool> Render for Embeds<ARTIFACT> {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().child(artifact("plan", PLAN).w_full())
        }
    }

    // An artifact piece carries its link and nothing a page reports.
    #[test]
    fn an_artifact_piece_carries_its_link() {
        let mut cx = TestAppContext::new();
        cx.open::<Embeds<true>>();
        let wire::Node::Artifact { link, .. } = cx.node("plan").clone() else {
            panic!("no artifact piece: {:?}", cx.root());
        };
        assert_eq!(link, PLAN);
    }

    // Drawing pages is not embedding artifacts: `web` alone is refused.
    #[test]
    #[should_panic(expected = "undeclared_capability")]
    fn an_artifact_piece_needs_its_own_capability() {
        TestAppContext::new().open::<Embeds<false>>();
    }
}
