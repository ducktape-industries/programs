//! These drive a view against a host that does with each frame what the
//! app's does — sanitize it or apply its patches, then hold each picture's
//! bytes by hash and keep the tree without them — and count the frames
//! until every picture drawn is held.
use super::*;
use std::collections::HashSet;

const KIB: usize = 1 << 10;

#[derive(Default, Serialize, Deserialize)]
struct Gallery(Vec<usize>);
impl View for Gallery {
    const NAME: &'static str = "Gallery";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self::default()
    }
}
impl Render for Gallery {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("gallery")
            .children(self.0.iter().enumerate().map(|(index, size)| {
                img(Arc::new(Image::from_bytes(
                    ImageFormat::Png,
                    vec![index as u8; *size],
                )))
            }))
    }
}

#[derive(Default)]
struct Host {
    tree: Option<wire::Node>,
    styles: wire::Styles,
    held: HashSet<u64>,
    drawn: HashSet<u64>,
    dropped: usize,
}

impl Host {
    fn take(&mut self, frame: &wire::Frame) -> bool {
        let mut carried = 0;
        let mut count = |node: &wire::Node| {
            node.clone()
                .for_each_mut(&mut |node| carried += payload(node).map_or(0, |bytes| bytes.len()))
        };
        frame.root.iter().for_each(&mut count);
        frame.patches.iter().for_each(|patch| match patch {
            wire::Patch::Replace { node, .. }
            | wire::Patch::Insert { node, .. }
            | wire::Patch::Props { node, .. } => count(node),
            wire::Patch::Remove { .. } | wire::Patch::Move { .. } => {}
        });
        let mut taken = wire::Frame {
            root: frame.root.clone(),
            styles: frame.styles.clone(),
            ..Default::default()
        };
        wire::sanitize(&mut taken, &mut self.styles).expect("the host takes the frame");
        if taken.root.is_some() {
            self.tree = taken.root;
        } else if !frame.patches.is_empty() {
            let tree = self.tree.as_mut().expect("a patch frame has a tree");
            wire::apply(tree, frame.patches.clone(), &self.styles)
                .expect("the host takes the patches");
        }
        let (held, drawn) = (&mut self.held, &mut self.drawn);
        let mut kept = 0;
        drawn.clear();
        if let Some(tree) = &mut self.tree {
            tree.for_each_mut(&mut |node| {
                let Some(hash) = picture_hash(node) else {
                    return;
                };
                drawn.insert(hash);
                if let Some(bytes) = take_payload(node) {
                    kept += bytes;
                    held.insert(hash);
                }
            });
        }
        self.dropped += carried - kept;
        frame.busy
    }

    fn missing(&self) -> usize {
        self.drawn.difference(&self.held).count()
    }

    fn resync(&mut self) -> Vec<wire::Event> {
        self.tree = None;
        self.held.clear();
        vec![wire::Event::Resync]
    }
}

fn picture_hash(node: &wire::Node) -> Option<u64> {
    match node {
        wire::Node::Image { hash, .. }
        | wire::Node::Svg {
            source: wire::SvgSource::Data { hash, .. },
            ..
        } => Some(*hash),
        _ => None,
    }
}

fn payload(node: &wire::Node) -> Option<&[u8]> {
    match node {
        wire::Node::Image {
            data: Some(wire::ImageData::Encoded(bytes)),
            ..
        }
        | wire::Node::Svg {
            source: wire::SvgSource::Data {
                bytes: Some(bytes), ..
            },
            ..
        } => Some(bytes),
        _ => None,
    }
}

fn take_payload(node: &mut wire::Node) -> Option<usize> {
    let len = payload(node)?.len();
    match node {
        wire::Node::Image { data, .. } => *data = None,
        wire::Node::Svg {
            source: wire::SvgSource::Data { bytes, .. },
            ..
        } => *bytes = None,
        _ => unreachable!(),
    }
    Some(len)
}

fn frames_to_park(
    driver: &mut Driver<Gallery>,
    host: &mut Host,
    events: Vec<wire::Event>,
) -> usize {
    let mut events = events;
    let mut frames = 1;
    while driver.tick_with(std::mem::take(&mut events), |frame| host.take(frame)) {
        frames += 1;
    }
    frames
}

fn show(driver: &mut Driver<Gallery>, sizes: Vec<usize>) {
    driver.entity().update(driver.app_mut(), |view, cx| {
        view.0 = sizes;
        cx.notify();
    });
}

#[test]
fn every_picture_drawn_reaches_the_host_in_one_frame() {
    let mut driver = Driver::<Gallery>::new();
    let mut host = Host::default();
    show(&mut driver, vec![384 * KIB; 8]);
    let frames = frames_to_park(&mut driver, &mut host, vec![]);
    assert_eq!(host.drawn.len(), 8);
    assert_eq!(
        host.missing(),
        0,
        "pictures drawn that the host does not hold"
    );
    assert_eq!(
        host.dropped, 0,
        "picture bytes the host's sanitizer dropped"
    );
    assert_eq!(frames, 1);
}

#[test]
fn every_picture_drawn_is_held_again_after_a_resync() {
    let mut driver = Driver::<Gallery>::new();
    let mut host = Host::default();
    let mut sizes = Vec::new();
    for _ in 0..4 {
        sizes.push(768 * KIB);
        show(&mut driver, sizes.clone());
        frames_to_park(&mut driver, &mut host, vec![]);
    }
    assert_eq!((host.drawn.len(), host.missing()), (4, 0));
    let events = host.resync();
    let frames = frames_to_park(&mut driver, &mut host, events);
    assert_eq!(host.drawn.len(), 4);
    assert_eq!(
        host.missing(),
        0,
        "pictures drawn that the host does not hold"
    );
    assert_eq!(
        host.dropped, 0,
        "picture bytes the host's sanitizer dropped"
    );
    assert_eq!(frames, 1);
}
