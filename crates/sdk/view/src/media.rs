//! The camera and microphone pieces: a camera's picture and a microphone's
//! level, captured and drawn by the host ([`wire::Node::Camera`],
//! [`wire::Node::Microphone`]), for a view that declares
//! [`Capability::Camera`](crate::methods::Capability::Camera) or
//! [`Capability::Microphone`](crate::methods::Capability::Microphone); and a
//! call's: the same pieces publishing into a live track
//! ([`Camera::publish`], [`Microphone::publish`]), and the other people's
//! tracks played back ([`remote_video`], [`remote_audio`]), for a view that
//! also declares [`Capability::Live`](crate::methods::Capability::Live).
//! The view never sees a picture, a sound or a level: it places the pieces,
//! steers them, and hears how their devices and tracks are
//! ([`MediaState`]). What a track carries is the host's: it encodes, sends,
//! receives, decodes, draws and plays.
use crate::element::wire_id;
use crate::interactivity::EventListener;
use crate::methods::LiveTrack;
use crate::revised::Revised;
use crate::{App, Element, ElementId, IntoElement, Lowering, Window, wire};
use gpui::{ObjectFit, StyleRefinement, Styled};

/// What a piece tells its view: whether its device is live, refused, busy
/// or gone, the device's name, a microphone's recording, whether the person
/// is speaking, and where its live track stands.
pub use wire::{MediaState, MediaStatus, Playback, TrackState};

/// A camera's picture the host draws in this element's box: the whole
/// picture, centred, or as [`Camera::object_fit`] says; flipped left
/// to right with [`Camera::mirror`]. The camera opens the first time the
/// host sees the element under `id` and is released (its light off) when a
/// frame no longer has it: a view turns its camera off by not placing the
/// piece. Only a view whose manifest declares
/// [`Capability::Camera`](crate::methods::Capability::Camera) may place
/// one: the host refuses any other's frame.
///
/// ```
/// # use view::prelude::*;
/// let preview = camera("preview")
///     .mirror(true)
///     .on_state(|state: &MediaState, _, _| println!("{:?}", state.status))
///     .size_full();
/// ```
pub struct Camera {
    id: ElementId,
    device: Option<String>,
    mirror: bool,
    object_fit: ObjectFit,
    publish: Option<LiveTrack>,
    on_state: Option<EventListener<MediaState>>,
    style: Box<StyleRefinement>,
}

pub fn camera(id: impl Into<ElementId>) -> Camera {
    Camera {
        id: id.into(),
        device: None,
        mirror: false,
        object_fit: ObjectFit::Contain,
        publish: None,
        on_state: None,
        style: Box::default(),
    }
}

impl Camera {
    /// The camera to open, by an id `camera.devices` listed; without it,
    /// the system's default. A new id opens the new camera.
    pub fn device(mut self, id: impl Into<String>) -> Self {
        self.device = Some(id.into());
        self
    }

    /// Flips the picture left to right, as a mirror shows a face.
    pub fn mirror(mut self, mirror: bool) -> Self {
        self.mirror = mirror;
        self
    }

    /// How the picture fills the box, as an image's does: whole
    /// (`ObjectFit::Contain`, the default) or covering it, cut centred
    /// (`ObjectFit::Cover`).
    pub fn object_fit(mut self, object_fit: ObjectFit) -> Self {
        self.object_fit = object_fit;
        self
    }

    /// Publishes the picture into `track` of the view's room while the
    /// piece is placed: the host encodes what it captures and sends it
    /// (stream mode). The track goes up whatever the camera's status, and
    /// [`MediaState::track`] says where it stands. The published picture is
    /// never mirrored: [`mirror`](Self::mirror) and
    /// [`object_fit`](Self::object_fit) are the preview's. The view
    /// declares [`Capability::Live`](crate::methods::Capability::Live) too.
    ///
    /// ```
    /// # use view::prelude::*;
    /// # let (call, me) = ("call-1".to_string(), 3);
    /// let me = camera("me")
    ///     .publish(LiveTrack { room: call.clone(), name: format!("cam/{me}") })
    ///     .object_fit(ObjectFit::Cover)
    ///     .mirror(true);
    /// ```
    pub fn publish(mut self, track: LiveTrack) -> Self {
        self.publish = Some(track);
        self
    }

    /// Hears the camera: starting, live, refused, busy, gone; and its
    /// track, with [`publish`](Self::publish).
    pub fn on_state(
        mut self,
        listener: impl Fn(&MediaState, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_state = Some(Box::new(listener));
        self
    }
}

/// What a view asks of a [`microphone`] piece, kept until the frame that
/// carries it: give it to the piece with [`Microphone::track`], call
/// [`record`](Self::record) or [`stop`](Self::stop), and notify. Each
/// reaches the host once.
#[derive(Clone, Default)]
pub struct MicHandle(std::rc::Rc<std::cell::RefCell<Revised<wire::MicCommand>>>);

impl MicHandle {
    pub fn new() -> Self {
        Self::default()
    }

    fn push(&self, command: wire::MicCommand) {
        self.0.borrow_mut().push(command);
    }

    /// Records `seconds`, then plays the recording on the piece's output.
    pub fn record(&self, seconds: u8) {
        self.push(wire::MicCommand::Record { seconds });
    }

    /// Ends a recording or its playback.
    pub fn stop(&self) {
        self.push(wire::MicCommand::Stop);
    }
}

/// A microphone's level the host draws as a meter in this element's box:
/// a bar from the left in the element's text colour over its background,
/// moving as the person speaks, with no frame of the view's. The
/// microphone opens and is released as a [`camera`]'s camera is: a view
/// mutes by not placing the piece. Only a view whose manifest declares
/// [`Capability::Microphone`](crate::methods::Capability::Microphone) may
/// place one.
///
/// ```
/// # use view::prelude::*;
/// let mic = MicHandle::new();
/// let meter = microphone("meter").track(&mic).h(px(8.)).w_full();
/// mic.record(5);
/// ```
pub struct Microphone {
    id: ElementId,
    device: Option<String>,
    output: Option<String>,
    monitor: bool,
    handle: Option<MicHandle>,
    publish: Option<LiveTrack>,
    muted: bool,
    on_state: Option<EventListener<MediaState>>,
    style: Box<StyleRefinement>,
}

pub fn microphone(id: impl Into<ElementId>) -> Microphone {
    Microphone {
        id: id.into(),
        device: None,
        output: None,
        monitor: false,
        handle: None,
        publish: None,
        muted: false,
        on_state: None,
        style: Box::default(),
    }
}

impl Microphone {
    /// The microphone to open, by an input `microphone.devices` listed;
    /// without it, the system's default.
    pub fn device(mut self, id: impl Into<String>) -> Self {
        self.device = Some(id.into());
        self
    }

    /// The speaker a recording plays on and the monitor loops to, by an
    /// output `microphone.devices` listed; without it, the system's
    /// default.
    pub fn output(mut self, id: impl Into<String>) -> Self {
        self.output = Some(id.into());
        self
    }

    /// Plays what the microphone hears straight back while set. Through
    /// speakers it feeds back: set it only on the person's explicit choice,
    /// and tell them to wear headphones. The host stops it when the sound
    /// falls to a loudspeaker ([`MediaState::monitor_stopped`]): turn your
    /// switch off on it.
    pub fn monitor(mut self, monitor: bool) -> Self {
        self.monitor = monitor;
        self
    }

    /// The piece takes its recordings from `handle`.
    pub fn track(mut self, handle: &MicHandle) -> Self {
        self.handle = Some(handle.clone());
        self
    }

    /// Publishes the voice into `track` of the view's room while the piece
    /// is placed: the host encodes what it hears and sends it (datagram
    /// mode, one 20 ms frame per object). The track goes up whatever the
    /// microphone's status, so the person is in the room even with no
    /// working microphone; [`MediaState::track`] says where it stands. The
    /// view declares [`Capability::Live`](crate::methods::Capability::Live)
    /// too.
    ///
    /// ```
    /// # use view::prelude::*;
    /// # let (call, me, muted) = ("call-1".to_string(), 3, false);
    /// let me = microphone("me")
    ///     .publish(LiveTrack { room: call.clone(), name: format!("mic/{me}") })
    ///     .muted(muted);
    /// ```
    pub fn publish(mut self, track: LiveTrack) -> Self {
        self.publish = Some(track);
        self
    }

    /// With [`publish`](Self::publish): the device released (the system's
    /// mark off) and nothing sent, the track kept up, so a muted person
    /// stays in the room. Without it, ignored: mute a plain piece by not
    /// placing it.
    pub fn muted(mut self, muted: bool) -> Self {
        self.muted = muted;
        self
    }

    /// Hears the microphone and its recording; and its track, with
    /// [`publish`](Self::publish).
    pub fn on_state(
        mut self,
        listener: impl Fn(&MediaState, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_state = Some(Box::new(listener));
        self
    }
}

/// A person's picture from a call, drawn by the host in this element's box
/// as an image's is by [`object_fit`](Self::object_fit): the host subscribes
/// to `track` of the view's room the first time it sees the element, from
/// the group it holds (its keyframe), and lets go when a frame no longer
/// has it. [`MediaState`] says how it plays: `Starting` until the first
/// picture, `Live` while they come, `Gone` when the track ended,
/// `Unsupported` where this build decodes no video; its `track` says why.
/// Only a view whose manifest declares
/// [`Capability::Live`](crate::methods::Capability::Live) may place one.
///
/// ```
/// # use view::prelude::*;
/// # let (room, them) = ("call-1".to_string(), 4_u64);
/// let tile = remote_video(("tile", them), LiveTrack { room, name: format!("cam/{them}") })
///     .object_fit(ObjectFit::Cover)
///     .size_full();
/// ```
pub struct RemoteVideo {
    id: ElementId,
    track: LiveTrack,
    object_fit: ObjectFit,
    on_state: Option<EventListener<MediaState>>,
    style: Box<StyleRefinement>,
}

pub fn remote_video(id: impl Into<ElementId>, track: LiveTrack) -> RemoteVideo {
    RemoteVideo {
        id: id.into(),
        track,
        object_fit: ObjectFit::Contain,
        on_state: None,
        style: Box::default(),
    }
}

impl RemoteVideo {
    /// How the picture fills the box: whole (`ObjectFit::Contain`, the
    /// default) or covering it, cut centred (`ObjectFit::Cover`).
    pub fn object_fit(mut self, object_fit: ObjectFit) -> Self {
        self.object_fit = object_fit;
        self
    }

    /// Hears the picture and its track.
    pub fn on_state(
        mut self,
        listener: impl Fn(&MediaState, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_state = Some(Box::new(listener));
        self
    }
}

/// A person's voice from a call, played by the host: it subscribes to
/// `track` of the view's room the first time it sees the element, from now
/// on, mixes it with the room's other voices on
/// [`output`](Self::output), and lets go when a frame no longer has it.
/// It draws nothing, and takes no room: zero-size and out of the layout's
/// flow, so place one per person wherever is handy, whoever the stage
/// shows. [`MediaState`] says how it plays, and `speaking` whether the
/// person speaks. Only a view whose manifest declares
/// [`Capability::Live`](crate::methods::Capability::Live) may place one.
///
/// ```
/// # use view::prelude::*;
/// # let (room, them) = ("call-1".to_string(), 4_u64);
/// let voice = remote_audio(("voice", them), LiveTrack { room, name: format!("mic/{them}") })
///     .on_state(|state: &MediaState, _, _| println!("{}", state.speaking));
/// ```
pub struct RemoteAudio {
    id: ElementId,
    track: LiveTrack,
    output: Option<String>,
    on_state: Option<EventListener<MediaState>>,
    style: Box<StyleRefinement>,
}

pub fn remote_audio(id: impl Into<ElementId>, track: LiveTrack) -> RemoteAudio {
    RemoteAudio {
        id: id.into(),
        track,
        output: None,
        on_state: None,
        style: Box::default(),
    }
    .absolute()
    .size_0()
}

impl RemoteAudio {
    /// The speaker the voice plays on, by an output `microphone.devices`
    /// listed; without it, the system's default.
    pub fn output(mut self, id: impl Into<String>) -> Self {
        self.output = Some(id.into());
        self
    }

    /// Hears the voice and its track, and whether the person speaks.
    pub fn on_state(
        mut self,
        listener: impl Fn(&MediaState, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_state = Some(Box::new(listener));
        self
    }
}

impl Element for Camera {
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        wire::Node::Camera {
            id: wire_id(self.id),
            style: lowering.style(&self.style),
            device: self.device,
            mirror: self.mirror,
            object_fit: self.object_fit.into(),
            on_state: self
                .on_state
                .map(|listener| lowering.route(crate::slots::Kind::MediaState, listener)),
            publish: self.publish,
        }
    }
}

impl Element for RemoteVideo {
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        wire::Node::Remote {
            id: wire_id(self.id),
            style: lowering.style(&self.style),
            track: self.track,
            media: wire::RemoteMedia::Video {
                object_fit: self.object_fit.into(),
            },
            on_state: self
                .on_state
                .map(|listener| lowering.route(crate::slots::Kind::MediaState, listener)),
        }
    }
}

impl Element for RemoteAudio {
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        wire::Node::Remote {
            id: wire_id(self.id),
            style: lowering.style(&self.style),
            track: self.track,
            media: wire::RemoteMedia::Audio {
                output: self.output,
            },
            on_state: self
                .on_state
                .map(|listener| lowering.route(crate::slots::Kind::MediaState, listener)),
        }
    }
}

impl Element for Microphone {
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        let (revision, commands) = self
            .handle
            .map(|handle| handle.0.borrow_mut().take())
            .unwrap_or_default();
        wire::Node::Microphone {
            id: wire_id(self.id),
            style: lowering.style(&self.style),
            device: self.device,
            output: self.output,
            monitor: self.monitor,
            revision,
            commands,
            on_state: self
                .on_state
                .map(|listener| lowering.route(crate::slots::Kind::MediaState, listener)),
            publish: self.publish,
            muted: self.muted,
        }
    }
}

impl IntoElement for Camera {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl IntoElement for Microphone {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl IntoElement for RemoteVideo {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl IntoElement for RemoteAudio {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Styled for Camera {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Styled for Microphone {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Styled for RemoteVideo {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Styled for RemoteAudio {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl gpui::prelude::FluentBuilder for Camera {}
impl gpui::prelude::FluentBuilder for Microphone {}
impl gpui::prelude::FluentBuilder for RemoteVideo {}
impl gpui::prelude::FluentBuilder for RemoteAudio {}

impl crate::testing::TestAppContext {
    /// The camera, microphone or remote piece placed under `id` (the first
    /// in the tree, depth first) reports `state`, as the app tells it when
    /// its device starts, fails or goes, a recording moves, or its track
    /// does; told only when it changes what the host held. A `track` left
    /// [`TrackState::None`] keeps the one the host holds for the piece (a
    /// [`Relay`](crate::testing::Relay) moves it): a test tells a device's
    /// status without undoing the room's. A piece that publishes or plays a
    /// track never reports `None`.
    pub fn simulate_media_state(&mut self, id: impl Into<ElementId>, state: MediaState) {
        let id = wire_id(id.into());
        let chain = crate::testing::chain(self.root(), &mut |chain| {
            chain.last().unwrap().identity() == Some(&id)
        })
        .unwrap_or_default();
        assert!(
            matches!(
                chain.last(),
                Some(
                    wire::Node::Camera {
                        on_state: Some(_),
                        ..
                    } | wire::Node::Microphone {
                        on_state: Some(_),
                        ..
                    } | wire::Node::Remote {
                        on_state: Some(_),
                        ..
                    }
                )
            ),
            "{id:?} is no camera, microphone or remote piece with a state route"
        );
        let path = crate::testing::authored_path(&chain);
        self.host().media(&path, |held| {
            let track = match state.track {
                TrackState::None => std::mem::take(&mut held.track),
                told => told,
            };
            *held = MediaState { track, ..state };
        });
        self.run_until_parked();
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
    struct Check {
        #[serde(skip)]
        mic: MicHandle,
        heard: Vec<MediaStatus>,
    }

    impl View for Check {
        const NAME: &'static str = "Check";
        const CAPABILITIES: &'static [Capability] = &[Capability::Camera, Capability::Microphone];
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self::default()
        }
    }

    impl Render for Check {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let heard = cx.listener(|view: &mut Self, state: &MediaState, _, cx| {
                view.heard.push(state.status.clone());
                cx.notify();
            });
            div()
                .child(
                    camera("camera")
                        .device("1")
                        .mirror(true)
                        .object_fit(ObjectFit::Cover)
                        .on_state(heard),
                )
                .child(
                    microphone("mic")
                        .output("headphones")
                        .monitor(true)
                        .track(&self.mic),
                )
        }
    }

    #[test]
    fn media_pieces_carry_their_devices_and_a_recording_once() {
        let mut cx = TestAppContext::new();
        let view = cx.open::<Check>();
        let wire::Node::Camera {
            device,
            mirror: true,
            object_fit: wire::ImageObjectFit::Cover,
            on_state: Some(_),
            ..
        } = cx.node("camera").clone()
        else {
            panic!("no mirrored camera with a state route: {:?}", cx.root());
        };
        assert_eq!(device.as_deref(), Some("1"));
        let wire::Node::Microphone {
            device: None,
            output,
            monitor: true,
            revision,
            commands,
            on_state: None,
            ..
        } = cx.node("mic").clone()
        else {
            panic!("no monitoring microphone: {:?}", cx.root());
        };
        assert_eq!(output.as_deref(), Some("headphones"));
        assert!(commands.is_empty());
        cx.update(&view, |view, _, cx| {
            view.mic.record(5);
            cx.notify();
        });
        cx.run_until_parked();
        let wire::Node::Microphone {
            revision: moved,
            commands,
            ..
        } = cx.node("mic").clone()
        else {
            unreachable!()
        };
        assert_eq!(moved, revision + 1);
        assert_eq!(commands, vec![wire::MicCommand::Record { seconds: 5 }]);
        cx.simulate_media_state(
            "camera",
            MediaState {
                status: MediaStatus::Denied,
                ..Default::default()
            },
        );
        let wire::Node::Microphone { commands, .. } = cx.node("mic").clone() else {
            unreachable!()
        };
        assert!(commands.is_empty(), "the recording went with its frame");
        cx.update(&view, |view, _, _| {
            assert_eq!(view.heard, [MediaStatus::Denied]);
        });
    }

    #[derive(Default, serde::Deserialize, serde::Serialize)]
    struct Meter {
        heard: Vec<(MediaStatus, Playback, bool)>,
    }

    impl View for Meter {
        const NAME: &'static str = "Meter";
        const CAPABILITIES: &'static [Capability] = &[Capability::Microphone];
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self::default()
        }
    }

    impl Render for Meter {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            microphone("mic").on_state(cx.listener(|view, state: &MediaState, _, cx| {
                view.heard
                    .push((state.status.clone(), state.playback.clone(), state.speaking));
                cx.notify();
            }))
        }
    }

    #[test]
    fn a_microphone_hears_its_device_and_its_recording() {
        let mut cx = TestAppContext::new();
        let view = cx.open::<Meter>();
        cx.simulate_media_state(
            "mic",
            MediaState {
                status: MediaStatus::Live,
                playback: Playback::Recording,
                speaking: true,
                ..Default::default()
            },
        );
        cx.update(&view, |view, _, _| {
            assert_eq!(view.heard, [(MediaStatus::Live, Playback::Recording, true)]);
        });
    }

    #[derive(Default, serde::Deserialize, serde::Serialize)]
    struct NoCamera;

    impl View for NoCamera {
        const NAME: &'static str = "NoCamera";
        const CAPABILITIES: &'static [Capability] = &[Capability::Microphone];
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self
        }
    }

    impl Render for NoCamera {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().child(camera("camera"))
        }
    }

    #[test]
    #[should_panic(expected = "undeclared_capability")]
    fn a_camera_the_manifest_leaves_out_fails_the_test() {
        TestAppContext::new().open::<NoCamera>();
    }

    #[derive(Default, serde::Deserialize, serde::Serialize)]
    struct NoMicrophone;

    impl View for NoMicrophone {
        const NAME: &'static str = "NoMicrophone";
        const CAPABILITIES: &'static [Capability] = &[Capability::Camera];
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self
        }
    }

    impl Render for NoMicrophone {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().child(microphone("mic"))
        }
    }

    #[test]
    #[should_panic(expected = "undeclared_capability")]
    fn a_microphone_the_manifest_leaves_out_fails_the_test() {
        TestAppContext::new().open::<NoMicrophone>();
    }

    fn call(name: &str) -> LiveTrack {
        LiveTrack {
            room: "call-1".into(),
            name: name.into(),
        }
    }

    #[derive(Default, serde::Deserialize, serde::Serialize)]
    struct Huddle {
        muted: bool,
        heard: Vec<(MediaStatus, TrackState)>,
    }

    impl View for Huddle {
        const NAME: &'static str = "Huddle";
        const CAPABILITIES: &'static [Capability] =
            &[Capability::Camera, Capability::Microphone, Capability::Live];
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self::default()
        }
    }

    impl Render for Huddle {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let heard = cx.listener(|view: &mut Self, state: &MediaState, _, cx| {
                view.heard.push((state.status.clone(), state.track.clone()));
                cx.notify();
            });
            div()
                .child(camera("cam").publish(call("cam/1")).mirror(true))
                .child(microphone("mic").publish(call("mic/1")).muted(self.muted))
                .child(
                    remote_video("tile", call("cam/2"))
                        .object_fit(ObjectFit::Cover)
                        .size_full(),
                )
                .child(
                    remote_audio(("voice", 2_u64), call("mic/2"))
                        .output("headphones")
                        .on_state(heard),
                )
        }
    }

    #[test]
    fn call_pieces_carry_their_tracks() {
        let mut cx = TestAppContext::new();
        let view = cx.open::<Huddle>();
        let wire::Node::Camera {
            publish: Some(cam),
            mirror: true,
            ..
        } = cx.node("cam").clone()
        else {
            panic!("no publishing camera: {:?}", cx.root());
        };
        assert_eq!(cam, call("cam/1"));
        let wire::Node::Microphone {
            publish: Some(mic),
            muted: false,
            ..
        } = cx.node("mic").clone()
        else {
            panic!("no publishing microphone: {:?}", cx.root());
        };
        assert_eq!(mic, call("mic/1"));
        cx.update(&view, |view, _, cx| {
            view.muted = true;
            cx.notify();
        });
        cx.run_until_parked();
        assert!(matches!(
            cx.node("mic"),
            wire::Node::Microphone { muted: true, .. }
        ));
        let pieces: Vec<_> = crate::testing::pieces(cx.root())
            .into_iter()
            .filter_map(|(_, node)| match node {
                wire::Node::Remote { track, media, .. } => Some((track.clone(), media.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            pieces,
            [
                (
                    call("cam/2"),
                    wire::RemoteMedia::Video {
                        object_fit: wire::ImageObjectFit::Cover
                    }
                ),
                (
                    call("mic/2"),
                    wire::RemoteMedia::Audio {
                        output: Some("headphones".into())
                    }
                ),
            ]
        );
        let voice = ("voice", 2_u64);
        cx.simulate_media_state(voice, MediaState::default());
        let up = TrackState::Up { subscribers: 0 };
        let state = MediaState {
            track: up.clone(),
            ..Default::default()
        };
        cx.simulate_media_state(voice, state.clone());
        cx.simulate_media_state(voice, state);
        let live = MediaState {
            status: MediaStatus::Live,
            ..Default::default()
        };
        cx.simulate_media_state(voice, live);
        cx.update(&view, |view, _, _| {
            assert_eq!(
                view.heard,
                [
                    (MediaStatus::Starting, TrackState::None),
                    (MediaStatus::Starting, up.clone()),
                    (MediaStatus::Live, up),
                ]
            )
        });
    }

    #[derive(Default, serde::Deserialize, serde::Serialize)]
    struct NoLive;

    impl View for NoLive {
        const NAME: &'static str = "NoLive";
        const CAPABILITIES: &'static [Capability] = &[Capability::Microphone];
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self
        }
    }

    impl Render for NoLive {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().child(remote_audio("voice", call("mic/2")))
        }
    }

    #[test]
    #[should_panic(expected = "undeclared_capability")]
    fn a_remote_piece_without_live_fails_the_test() {
        TestAppContext::new().open::<NoLive>();
    }
}
