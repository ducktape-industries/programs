//! A person's picture by blob id: read once however many faces draw it,
//! the initial in its place until it lands, and where it is no picture.
use super::*;
use crate::methods::BlobGet;
use crate::pictures::Pictures;
use program::abi::BlobId;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nface";

// Four faces: ada and bo share a picture, cy's blob is no picture, di's
// is not held.
#[derive(Default, Serialize, Deserialize)]
struct Faces {
    #[serde(skip)]
    pictures: Option<Entity<Pictures>>,
}
impl View for Faces {
    const NAME: &'static str = "Faces";
    const CAPABILITIES: &'static [wire::methods::Capability] = &[wire::methods::Capability::Blob];
    fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            pictures: Some(cx.new(|_| Pictures::default())),
        }
    }
}
impl Render for Faces {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pictures = self.pictures.clone().expect("new built it");
        let theme = Theme::default();
        let faces = [("ada", 1), ("bo", 1), ("cy", 2), ("di", 3)].map(|(name, blob)| {
            let blob = BlobId::Sha256([blob; 32]);
            let picture = pictures.update(cx, |pictures, cx| pictures.get(&blob, cx));
            div()
                .id(name)
                .child(design::avatar_with(name, picture, px(32.), &theme))
        });
        div().id("faces").children(faces)
    }
}

fn blob(byte: u8) -> String {
    format!("sha256:{}", format!("{byte:02x}").repeat(32))
}

fn pictures_drawn(cx: &TestAppContext) -> usize {
    let mut drawn = 0;
    cx.whole_frame()
        .root
        .expect("a tree")
        .for_each_mut(&mut |node| {
            if matches!(node, wire::Node::Image { .. }) {
                drawn += 1;
            }
        });
    drawn
}

#[test]
fn a_picture_is_read_once_per_blob_and_drawn_where_it_is_one() {
    let mut cx = TestAppContext::new();
    cx.host().handle::<BlobGet>(|id| {
        Ok(match id {
            id if id == blob(1) => Some(PNG.to_vec()),
            id if id == blob(2) => Some(b"plain words".to_vec()),
            _ => None,
        })
    });
    cx.open::<Faces>();
    assert_eq!(
        cx.host().requests::<BlobGet>(),
        [blob(1), blob(2), blob(3)],
        "one read per blob, however many faces draw it"
    );
    assert_eq!(pictures_drawn(&cx), 2, "ada's and bo's");
    assert!(
        cx.has_text("C"),
        "a blob that is no picture keeps the initial"
    );
    assert!(cx.has_text("D"), "a blob not held keeps the initial");
    cx.tick(vec![]);
    assert_eq!(cx.host().requests::<BlobGet>().len(), 3, "never read again");
}

#[test]
fn every_face_draws_its_initial_while_its_picture_is_read() {
    let mut cx = TestAppContext::new();
    cx.host().never::<BlobGet>();
    cx.open::<Faces>();
    assert_eq!(pictures_drawn(&cx), 0);
    for initial in ["A", "B", "C", "D"] {
        assert!(cx.has_text(initial), "{initial}");
    }
}
