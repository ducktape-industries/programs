//! tally-view: the total tally keeps and a button that adds one, read again whenever a block writes
//! tally. The example view of docs/ducktape-for-self-improvement.md, beside its program, tally.
use serde::{Deserialize, Serialize};
use tally::Tally;
use view::prelude::*;

#[derive(Default, Serialize, Deserialize)]
pub struct Counter {
    total: Reread<u64>,
    #[serde(skip)]
    failed: Option<String>,
    #[serde(skip)]
    adding: Option<Task<()>>,
}

impl View for Counter {
    const NAME: &'static str = "Tally";
    const DESCRIPTION: &'static str = "The total everyone has added, and a button that adds one.";
    const CAPABILITIES: &'static [Capability] =
        &[Capability::Host, Capability::Program, Capability::Op];

    fn attach(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        cx.follow::<Changes<Tally>>((), |view, change, cx| match change {
            Ok(_) => view.reread(cx),
            Err(refusal) => cx.log_refused("the changes", &refusal),
        })
        .detach();
        self.reread(cx);
    }
}

impl Counter {
    fn reread(&mut self, cx: &mut Context<Self>) {
        cx.reread(
            self,
            |view| &mut view.total,
            |_, cx| total(cx.host()),
            Self::reread,
        );
    }

    fn add(&mut self, cx: &mut Context<Self>) {
        if self.adding.is_some() {
            return;
        }
        self.failed = None;
        let submit = cx.host().ask::<Submit<Tally>>(tally::Op::Add { by: 1 });
        self.adding = Some(cx.land(submit, |view, answer, cx| {
            view.adding = None;
            view.failed = design::failure(&answer);
            cx.notify();
        }));
        cx.notify();
    }
}

async fn total(host: Host) -> Result<u64, Error> {
    host.query(tally::ask::Total).await
}

impl Render for Counter {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        let ladder = &design::ladder(window.touch());
        let total = match &*self.total {
            Loadable::Idle | Loadable::Loading(_) => {
                design::quiet("Reading…", &theme).into_any_element()
            }
            Loadable::Failed(refusal) => {
                let retry =
                    cx.listener(|view: &mut Counter, _: &ClickEvent, _, cx| view.reread(cx));
                design::refused("total", design::sentence(refusal), ladder, &theme, retry)
                    .into_any_element()
            }
            Loadable::Ready(total) | Loadable::Reloading(total, _) => div()
                .id("total")
                .role(Role::Status)
                .aria_live(view::accesskit::Live::Polite)
                .child(format!("Total: {total}"))
                .into_any_element(),
        };
        let add = cx.listener(|view: &mut Counter, _: &ClickEvent, _, cx| view.add(cx));
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(design::space::MD)
            .p(design::space::LG)
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_size(design::text::BODY)
            .child(total)
            .child(
                design::button("add", "Add one", ladder, &theme, add).busy(self.adding.is_some()),
            )
            .children(
                self.failed
                    .clone()
                    .map(|why| div().text_color(theme.danger).child(why)),
            )
    }
}

export_view!(Counter);

#[cfg(test)]
mod tests;
