//! Preparation tickets in the native test chain, separate from ordinary reads.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenPreparation {
    pub requester: ProgramId,
    pub preparation: reads::Preparation,
    pub made: u64,
    pub answered: Option<(reads::Read, Vec<reads::Entry>)>,
}

impl MockChain {
    pub fn preparations(&self) -> Vec<(Ticket, OpenPreparation)> {
        self.preparations
            .borrow()
            .iter()
            .map(|(id, open)| (*id, open.clone()))
            .collect()
    }

    fn open_preparation(&self, id: Ticket) -> Result<OpenPreparation, Error> {
        self.preparations
            .borrow()
            .get(&id)
            .cloned()
            .ok_or_else(|| Error::new(code::NOT_FOUND, format!("no preparation {id} is open")))
    }

    pub fn answer_preparation(&self, id: Ticket) -> Result<(), Error> {
        let open = self.open_preparation(id)?;
        if open.answered.is_some() {
            return Err(Error::new(
                code::WRONG_STATE,
                format!("preparation {id} is answered already"),
            ));
        }
        if !self.awaited(id) {
            self.preparations.borrow_mut().remove(&id);
            return Ok(());
        }
        let preparation = open.preparation;
        let chunk_bytes = usize::try_from(preparation.chunk_bytes)
            .map_err(|_| crate::invalid("chunk size does not fit this host"))?;
        let mut chunks = Vec::new();
        for body in preparation.body.chunks(chunk_bytes) {
            let framed = blob::frame(&preparation.kind, body).map_err(crate::kernel::error_from)?;
            let id = crate::blob_id(preparation.hash, &preparation.kind, body)?;
            self.held.borrow_mut().insert(
                id,
                Blob {
                    kind: preparation.kind.clone(),
                    body: body.to_vec(),
                },
            );
            chunks.push(reads::Entry {
                id,
                len: framed.len() as u64,
                kind: preparation.kind.clone(),
            });
        }
        self.share_blobs();
        let prepared = reads::Prepared {
            size: preparation.body.len() as u64,
            chunks: chunks.clone(),
        };
        let body = abi::encode(&prepared);
        let framed = blob::frame(reads::KIND, &body).map_err(crate::kernel::error_from)?;
        let entry = reads::Entry {
            id: self.put_blob(reads::KIND, body)?,
            len: framed.len() as u64,
            kind: reads::KIND.into(),
        };
        let read = reads::Read {
            entry,
            at: self.clock.get().0,
        };
        if let Some(open) = self.preparations.borrow_mut().get_mut(&id) {
            open.answered = Some((read, chunks));
        }
        Ok(())
    }

    pub fn ready_preparation(&self, id: Ticket) -> Result<(), Error> {
        let open = self.open_preparation(id)?;
        let Some((read, products)) = open.answered else {
            return Err(Error::new(
                code::WRONG_STATE,
                format!("preparation {id} is not answered yet"),
            ));
        };
        let complete = std::iter::once(&read.entry)
            .chain(products.iter())
            .all(|entry| self.held.borrow().contains_key(&entry.id));
        if !complete {
            return Err(Error::new(
                code::WRONG_STATE,
                "preparation products are not held",
            ));
        }
        self.preparations.borrow_mut().remove(&id);
        if !self.awaited(id) {
            return Ok(());
        }
        let line = [b"read/".as_slice(), &id.to_be_bytes()].concat();
        let requester = self.seated(&open.requester)?;
        requester
            .host
            .borrow_mut()
            .lines
            .insert((open.requester, line), read.entry.id);
        self.resolve_read(id, Ok(abi::encode(&read)))
    }

    pub fn refuse_preparation(&self, id: Ticket, error: Error) -> Result<(), Error> {
        self.open_preparation(id)?;
        self.preparations.borrow_mut().remove(&id);
        if self.awaited(id) {
            self.resolve_read(id, Err(error))
        } else {
            Ok(())
        }
    }

    pub(super) fn ask_preparation(
        &self,
        env: &Env,
        ticket: Ticket,
        preparation: reads::Preparation,
    ) -> Result<(), Error> {
        if preparation.chunk_bytes == 0 {
            return Err(crate::invalid("chunk size is zero"));
        }
        if self.unserved.get() {
            return Err(crate::kernel::error_from(reads::unserved()));
        }
        self.preparations.borrow_mut().insert(
            ticket,
            OpenPreparation {
                requester: env.program.clone(),
                preparation,
                made: env.height,
                answered: None,
            },
        );
        Ok(())
    }

    pub(super) fn expire_preparations(&self) {
        let height = self.clock.get().0;
        let due: Vec<Ticket> = self
            .preparations
            .borrow()
            .iter()
            .filter(|(_, open)| open.made.saturating_add(reads::WAIT) <= height)
            .map(|(id, _)| *id)
            .collect();
        for id in due {
            let _ = self.refuse_preparation(id, crate::kernel::error_from(reads::expired(id)));
        }
    }
}
