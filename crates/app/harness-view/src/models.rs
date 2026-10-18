//! The models form: harness's `SetModel`, a model id with the wire the model speaks, the egress
//! service that reaches it and the vendor's name for it, shown for the person's yes before it
//! is sent. The first account to set an id owns it; its owner alone changes it.
use serde::{Deserialize, Serialize};
use view::prelude::*;

use harness::{ModelRow, Wire};

use crate::{Agents, Screen, applied};

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct ModelDraft {
    pub id: TextField,
    pub wire: usize,
    pub service: Option<String>,
    pub name: TextField,
    /// the desktop's form is open under the list
    pub shown: bool,
    pub confirming: bool,
    pub problem: Option<String>,
    #[serde(skip)]
    pub sending: Option<Task<()>>,
}

impl ModelDraft {
    pub fn wire(&self) -> Wire {
        Wire::ALL
            .get(self.wire)
            .copied()
            .unwrap_or(Wire::AnthropicMessages)
    }

    pub fn checked(&self) -> Result<harness::Op, String> {
        let id = self.id.text().trim().to_owned();
        if id.is_empty() {
            return Err("Write the model id agents name in their configuration.".into());
        }
        let Some(service) = self.service.clone() else {
            return Err("Pick the service that reaches the model.".into());
        };
        let name = self.name.text().trim().to_owned();
        if name.is_empty() {
            return Err("Write the vendor's name for the model.".into());
        }
        Ok(harness::Op::SetModel {
            id,
            wire: self.wire(),
            service,
            name,
        })
    }
}

impl Agents {
    pub(crate) fn edit_model(&mut self, row: &ModelRow, cx: &mut Context<Self>) {
        self.model_draft = ModelDraft {
            id: TextField::new(row.id.clone()),
            wire: Wire::ALL
                .iter()
                .position(|wire| *wire == row.wire)
                .unwrap_or_default(),
            service: Some(row.service.clone()),
            name: TextField::new(row.name.clone()),
            shown: true,
            ..ModelDraft::default()
        };
        cx.notify();
    }

    pub(crate) fn confirm_model(&mut self, cx: &mut Context<Self>) {
        match self.model_draft.checked() {
            Ok(_) => {
                self.model_draft.problem = None;
                self.model_draft.confirming = true;
            }
            Err(problem) => self.model_draft.problem = Some(problem),
        }
        cx.notify();
    }

    pub(crate) fn set_model(&mut self, cx: &mut Context<Self>) {
        let Ok(op) = self.model_draft.checked() else {
            return;
        };
        if self.model_draft.sending.is_some() {
            return;
        }
        let submit = cx.host().ask::<Submit<harness::Harness>>(op);
        self.model_draft.sending = Some(cx.land(submit, |view, answer, cx| {
            view.model_draft.sending = None;
            view.model_draft.confirming = false;
            match applied(answer) {
                Ok(_) => {
                    view.model_draft = ModelDraft::default();
                    match view.screen {
                        // the phone's form page goes back to the models it was opened over
                        Screen::SetModel => view.go(Screen::Models, cx),
                        _ => view.read_screen(cx),
                    }
                }
                Err(problem) => view.model_draft.problem = Some(problem),
            }
            cx.notify();
        }));
        cx.notify();
    }
}
