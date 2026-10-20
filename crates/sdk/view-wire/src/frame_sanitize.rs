use crate::*;

/// A tree deeper than this is cut off: a view cannot make the host's
/// layout recurse without bound.
pub const MAX_DEPTH: usize = 64;

/// The frame's style entries go into `styles`, the table the host holds
/// beside the tree: each is read and bounded once, here, and the frame is
/// left without them. A whole tree's entries replace the table. A node that
/// names a style the table does not hold refuses the frame, and a refused
/// frame leaves the table as it was: it is still the table of the tree the
/// host holds.
///
/// A frame that arrived as bytes has passed [`decode`] first, which refuses
/// one nested deeper than this walk goes. A frame that carries `patches`
/// instead of a tree is bounded by [`apply`], since every bound is on the
/// tree the patches make and only the host holds it.
pub fn sanitize(frame: &mut Frame, styles: &mut Styles) -> Result<SanitizeReport, Refused> {
    let entries = std::mem::take(&mut frame.styles);
    if frame.root.is_some() {
        let mut table = Styles::default();
        table.extend(entries)?;
        let report = sanitize_against(frame, &table)?;
        *styles = table;
        return Ok(report);
    }
    let held = styles.len();
    let taken = styles
        .extend(entries)
        .map_err(Refused::from)
        .and_then(|()| sanitize_against(frame, styles));
    if taken.is_err() {
        styles.truncate(held);
    }
    taken
}

fn sanitize_against(frame: &mut Frame, styles: &Styles) -> Result<SanitizeReport, Refused> {
    let mut walk = Walk::new(styles);
    if let Some(root) = &mut frame.root {
        sanitize_tree_with(root, &mut walk)?;
    }
    walk.in_tooltip = true;
    for response in &mut frame.tooltip_responses {
        if let Some(content) = &mut response.content {
            sanitize_node(
                content,
                1,
                &mut walk,
                &mut identity::Scopes::default(),
                None,
            )?;
        }
    }
    Ok(walk.cuts)
}

pub(crate) fn sanitize_tree(root: &mut Node, styles: &Styles) -> Result<SanitizeReport, Refused> {
    let mut walk = Walk::new(styles);
    sanitize_tree_with(root, &mut walk)?;
    Ok(walk.cuts)
}

fn sanitize_tree_with(root: &mut Node, walk: &mut Walk) -> Result<(), Refused> {
    sanitize_node(root, 0, walk, &mut identity::Scopes::default(), None).map_err(|refused| {
        let node = walk
            .at
            .iter()
            .try_fold(&*root, |node, &child| node.children().get(child as usize));
        Refused::At {
            kind: node.map_or("?", <&str>::from),
            key: node.and_then(Node::key).map(str::to_owned),
            at: std::mem::take(&mut walk.at),
            refused: Box::new(refused),
        }
    })
}

/// Why the host refuses a frame or a patch: a bound or a shape it breaks,
/// or one typed id claimed twice, with the site that claimed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    Invalid(&'static str),
    Duplicate(identity::DuplicateIdentity),
    /// What a tree's walk refused, at the node it refused: its kind, its
    /// key, and its child indices from the top of the tree (`[]` is the
    /// top itself, as a [`Patch`] path counts).
    At {
        kind: &'static str,
        key: Option<String>,
        at: Vec<u32>,
        refused: Box<Refused>,
    },
}

impl Refused {
    /// The refusal itself, wherever it fell.
    pub fn reason(&self) -> &Refused {
        match self {
            Self::At { refused, .. } => refused.reason(),
            reason => reason,
        }
    }
}

impl From<&'static str> for Refused {
    fn from(reason: &'static str) -> Self {
        Self::Invalid(reason)
    }
}

impl From<identity::DuplicateIdentity> for Refused {
    fn from(duplicate: identity::DuplicateIdentity) -> Self {
        Self::Duplicate(duplicate)
    }
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(reason) => f.write_str(reason),
            Self::Duplicate(duplicate) => duplicate.fmt(f),
            Self::At {
                kind,
                key: Some(key),
                at,
                refused,
            } => write!(f, "{kind} `{key}` at {at:?}: {refused}"),
            Self::At {
                kind, at, refused, ..
            } => write!(f, "{kind} at {at:?}: {refused}"),
        }
    }
}

impl std::error::Error for Refused {}

pub(crate) struct Walk {
    styles: usize,
    pub(crate) active_descendant: bool,
    pub(crate) in_sheet: bool,
    pub(crate) sheet_seen: bool,
    pub(crate) in_tooltip: bool,
    pub(crate) at: Vec<u32>,
    pub(crate) cuts: SanitizeReport,
}

impl Walk {
    pub(crate) fn new(styles: &Styles) -> Self {
        Self {
            styles: styles.len(),
            active_descendant: false,
            in_sheet: false,
            sheet_seen: false,
            in_tooltip: false,
            at: Vec::new(),
            cuts: SanitizeReport::default(),
        }
    }

    pub(crate) fn style(&self, id: StyleId) -> Result<(), &'static str> {
        match (id.0 as usize) < self.styles {
            true => Ok(()),
            false => Err("a node names a style its table does not hold"),
        }
    }

    pub(crate) fn cut(&mut self, kind: fn(&mut SanitizeReport) -> &mut usize, count: usize) {
        if count == 0 {
            return;
        }
        *kind(&mut self.cuts) += count;
        if self.cuts.first.is_none() {
            self.cuts.first = Some(self.at.clone());
        }
    }
}

mod node;
use node::sanitize_node;

mod numbers;
pub(crate) use numbers::{bounded, finite};

mod interactivity;
use interactivity::sanitize_interactivity;
