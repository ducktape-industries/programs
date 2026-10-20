//! What both designs draw alike, once: an agent's face, and the three
//! answers an agent can take. Each design passes its own ladder; neither
//! design's tree reaches into the other's.
use std::sync::Arc;
use view::design::{Face, Ladder};
use view::prelude::*;
use view::{Div, Stateful};

use crate::Settings;
use crate::queries::Agent;
use crate::state::Choice;
use crate::words;

pub(crate) fn agent_face(
    agent: &Agent,
    picture: Option<Arc<Image>>,
    face: impl Into<Face>,
    theme: &Theme,
) -> Div {
    let ringed = picture.is_some();
    design::avatar_with(&agent.name, picture, face, theme)
        .bg(theme.agent_soft)
        .text_color(theme.agent)
        .when(ringed, |face| face.border_2().border_color(theme.agent))
}

pub(crate) fn ownership_choices(
    agent: &Agent,
    shown: Choice,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let number = agent.number;
    design::segmented(
        format!("settings/agents/{number}/answers"),
        words::who_answers(&agent.name),
        ladder,
        theme,
        Choice::ALL.iter().position(|choice| *choice == shown),
        Choice::ALL.map(|choice| {
            (
                format!("settings/agents/{number}/answers/{}", choice.slug()).into(),
                choice.label().into(),
            )
        }),
        cx.processor(move |v: &mut Settings, index: usize, _, cx| {
            v.pick_ownership(number, Choice::ALL[index], cx)
        }),
    )
}
