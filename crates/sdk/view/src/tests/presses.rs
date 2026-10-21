//! A design button inside a pressable row: a press is the button's, and
//! the row around it hears nothing, under a pointer and under a finger.
use serde::{Deserialize, Serialize};

use crate::design;
use crate::prelude::*;
use crate::testing::TestAppContext;
use crate::{Role, View};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
enum Control {
    #[default]
    Button,
    Small,
    Icon,
}

#[derive(Default, Serialize, Deserialize)]
struct Row {
    control: Control,
    heard: Vec<String>,
}
impl View for Row {
    const NAME: &'static str = "Row";
}
impl Render for Row {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        let ladder = design::ladder(window.touch());
        let press = cx.listener(|row: &mut Self, _: &ClickEvent, _, cx| {
            row.heard.push("press".into());
            cx.notify();
        });
        let control = match self.control {
            Control::Button => {
                design::button("press", "Press", &ladder, &theme, press).into_any_element()
            }
            Control::Small => design::button("press", "Press", &ladder, &theme, press)
                .small()
                .into_any_element(),
            Control::Icon => design::icon_button("press", "+", "Press", &ladder, &theme, press)
                .into_any_element(),
        };
        div()
            .id("row")
            .role(Role::Group)
            .aria_label("Row")
            .focusable()
            .on_click(cx.listener(|row: &mut Self, _: &ClickEvent, _, cx| {
                row.heard.push("row".into());
                cx.notify();
            }))
            .child(control)
    }
}

#[test]
fn a_design_buttons_press_is_its_own_and_the_row_around_it_hears_nothing() {
    let mut cx = TestAppContext::new();
    let row = cx.open::<Row>();
    for touch in [false, true] {
        cx.simulate_touch(touch);
        for control in [Control::Button, Control::Small, Control::Icon] {
            cx.update(&row, |view, _, cx| {
                view.control = control;
                view.heard.clear();
                cx.notify();
            });
            cx.run_until_parked();
            cx.simulate_click("press");
            row.read(|view| {
                assert_eq!(view.heard, ["press"], "{control:?}, touch {touch}");
            });
        }
    }
}
