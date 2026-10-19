//! A list in parts: a section per `group_by` value under a head the reader folds (its icon, its
//! count, how many rows each badge marks), and rows of a badge, a lead, the title, a chip and a
//! trail on their section's tone. One line a row under a pointer, two under a finger.
use serde_json::Value;
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use super::{Draw, key, scope};
use crate::Chat;
use crate::widget::{Badge, fill, tone};

// A row's parts as the file's templates fill them; an empty one is left out.
struct Parts<'a> {
    title: String,
    lead: Option<String>,
    chip: Option<String>,
    trail: Option<String>,
    badge: Option<&'a Badge>,
}

impl Parts<'_> {
    // The row as assistive tech reads it: its parts in their order, the badge by its label.
    fn label(&self) -> String {
        let badge = self
            .badge
            .map(|badge| badge.label.clone().unwrap_or_else(|| badge.mark.clone()));
        let parts = [
            self.lead.clone(),
            Some(self.title.clone()),
            self.chip.clone(),
        ];
        let parts = parts.into_iter().chain([self.trail.clone(), badge]);
        parts.flatten().collect::<Vec<_>>().join(", ")
    }
}

// A section's head as assistive tech reads it: "doing, 12 rows, 3 working now".
fn section_label(value: &str, rows: usize, marked: &[(&Badge, usize)]) -> String {
    let mut said = format!("{value}, {}", design::plural(rows as u64, "row", "rows"));
    for (badge, count) in marked {
        let label = badge.label.as_deref().unwrap_or(&badge.mark);
        said.push_str(&format!(", {count} {label}"));
    }
    said
}

impl Draw<'_> {
    // The rows in parts, under a head per section when the file groups them.
    pub(super) fn parted(&self, rows: &[&Value], cx: &mut Context<Chat>) -> AnyElement {
        let grouped = self.shape.group_by.is_some();
        let mut drawn = Vec::new();
        for (index, (value, items)) in self.shape.sections(rows).into_iter().enumerate() {
            let folded = grouped && self.chat.widgets.folded(self.path, &value, self.shape);
            if grouped {
                drawn.push(self.head(index, &value, &items, folded, cx));
            }
            if folded {
                continue;
            }
            for (at, row) in items.iter().enumerate() {
                drawn.push(self.parted_row(format!("{index}-{at}"), &value, row, cx));
            }
        }
        div()
            .id(key(self.slot, "parts"))
            .flex()
            .flex_col()
            .children(drawn)
            .into_any_element()
    }

    // A section's head: pressed, it folds or opens the section.
    fn head(
        &self,
        index: usize,
        value: &str,
        items: &[&Value],
        folded: bool,
        cx: &mut Context<Chat>,
    ) -> AnyElement {
        let (theme, ladder) = (self.theme, self.ladder);
        let scope = scope(self.chat, None);
        let marked: Vec<(&Badge, usize)> = self
            .shape
            .badges
            .iter()
            .map(|badge| {
                let wears = |row: &&&Value| self.shape.badge_of(row, &scope) == Some(badge);
                (badge, items.iter().filter(wears).count())
            })
            .filter(|(_, count)| *count > 0)
            .collect();
        let counts: Vec<Div> = marked
            .iter()
            .map(|(badge, count)| {
                let color = tone(theme, badge.tone.as_deref()).unwrap_or(theme.muted);
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(design::space::XXS)
                    .child(div().text_color(color).child(badge.mark.clone()))
                    .child(count.to_string())
            })
            .collect();
        let label = section_label(value, items.len(), &marked);
        let (path, section) = (self.path.to_owned(), value.to_owned());
        let flip = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
            chat.flip_section(&path, &section);
            cx.notify();
        });
        let icon = self.shape.icons.get(value).cloned();
        let name = div()
            .flex_1()
            .min_w_0()
            .truncate()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.foreground)
            .child(value.to_owned());
        div()
            .id(key(self.slot, format!("section-{index}")))
            .flex()
            .items_center()
            .gap(design::space::SM)
            .when(index > 0, |head| head.mt(design::space::MD))
            .p(design::space::XS)
            .border_b_1()
            .border_color(theme.border)
            .text_type(ladder.secondary)
            .text_color(theme.muted)
            .role(Role::Button)
            .focusable()
            .cursor_pointer()
            .aria_expanded(!folded)
            .aria_label(label)
            .hover(|style| style.bg(theme.surface_raised))
            .on_click(flip)
            .child(div().flex_none().child(if folded { "▸" } else { "▾" }))
            .children(icon.map(|icon| div().flex_none().child(icon)))
            .child(name)
            .child(div().flex_none().child(items.len().to_string()))
            .children(counts)
            .into_any_element()
    }

    fn parts(&self, row: &Value) -> Parts<'_> {
        let scope = scope(self.chat, None);
        let filled = |template: &Option<String>| {
            let said = fill(template.as_deref()?, row, &scope);
            (!said.trim().is_empty()).then_some(said)
        };
        Parts {
            title: self.item(row),
            lead: filled(&self.shape.lead),
            chip: filled(&self.shape.chip),
            trail: filled(&self.shape.trail),
            badge: self.shape.badge_of(row, &scope),
        }
    }

    // A row on its section's tone: under a pointer one line (badge, lead, title, chip, trail),
    // under a finger the badge and title over a line of the rest.
    fn parted_row(
        &self,
        part: String,
        value: &str,
        row: &Value,
        cx: &mut Context<Chat>,
    ) -> AnyElement {
        let (theme, ladder) = (self.theme, self.ladder);
        let parts = self.parts(row);
        let mark = (!self.shape.badges.is_empty()).then(|| {
            let (glyph, color) = match parts.badge {
                Some(badge) => {
                    let color = tone(theme, badge.tone.as_deref()).unwrap_or(theme.foreground);
                    (badge.mark.clone(), color)
                }
                None => (String::new(), theme.muted),
            };
            div()
                .flex_none()
                .w(ladder.icon_row)
                .flex()
                .justify_center()
                .text_color(color)
                .child(glyph)
        });
        let title = div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_type(ladder.body)
            .text_color(theme.foreground)
            .child(parts.title.clone());
        let bar = tone(theme, self.shape.tones.get(value).map(String::as_str));
        let element = div()
            .id(key(self.slot, part))
            .py(design::space::XS)
            .pl(design::space::SM)
            .pr(design::space::XS)
            .when_some(bar, |row, color| row.border_l_2().border_color(color));
        let element = match ladder.touch {
            false => {
                let lead = parts.lead.clone().map(|lead| {
                    div()
                        .flex_none()
                        .text_type(ladder.secondary)
                        .text_color(theme.muted)
                        .child(lead)
                });
                let trail = parts.trail.clone().map(|trail| {
                    div()
                        .flex_none()
                        .text_type(ladder.caption)
                        .text_color(theme.muted)
                        .child(trail)
                });
                element
                    .flex()
                    .items_center()
                    .gap(design::space::SM)
                    .min_h(ladder.row)
                    .children(mark)
                    .children(lead)
                    .child(title)
                    .children(parts.chip.clone().map(|chip| self.chip(chip)))
                    .children(trail)
            }
            true => {
                let meta = [&parts.lead, &parts.chip, &parts.trail];
                let meta: Vec<String> = meta.into_iter().flatten().cloned().collect();
                let meta = (!meta.is_empty()).then(|| {
                    div()
                        .truncate()
                        .text_type(ladder.caption)
                        .text_color(theme.muted)
                        .child(meta.join(" · "))
                });
                let top = div()
                    .flex()
                    .items_center()
                    .gap(design::space::SM)
                    .children(mark)
                    .child(title);
                element
                    .flex()
                    .flex_col()
                    .gap(design::space::XXS)
                    .min_h(ladder.row_tall)
                    .child(top)
                    .children(meta)
            }
        };
        self.pressable(element, parts.label(), row, cx)
    }

    // An account as a small initial and its name.
    fn chip(&self, name: String) -> Div {
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(design::space::XXS)
            .text_type(self.ladder.caption)
            .text_color(self.theme.muted)
            .child(design::avatar(&name, self.ladder.avatar_inline, self.theme))
            .child(name)
    }

    // A row that opens where the file says pressing goes; any other stays words.
    fn pressable(
        &self,
        element: Stateful<Div>,
        label: String,
        row: &Value,
        cx: &mut Context<Chat>,
    ) -> AnyElement {
        let Some(target) = self.shape.target(row, &scope(self.chat, None)) else {
            return element.into_any_element();
        };
        let theme = self.theme;
        let open = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
            chat.open_widget_target(target.clone(), cx);
        });
        element
            .role(Role::Link)
            .focusable()
            .cursor_pointer()
            .aria_label(label)
            .hover(|style| style.bg(theme.surface_raised))
            .on_click(open)
            .into_any_element()
    }
}
