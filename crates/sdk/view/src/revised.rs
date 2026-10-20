//! What a piece's handle keeps for the host: the commands a view gave it
//! and a revision that moves with each, for the web page's navigations
//! ([`crate::WebHandle`]) and the microphone's recordings
//! ([`crate::MicHandle`]).

/// The commands since the frame that last carried the piece. The host
/// applies the commands of a revision it has not seen, once.
pub(crate) struct Revised<C> {
    revision: u64,
    commands: Vec<C>,
}

impl<C> Default for Revised<C> {
    fn default() -> Self {
        Self {
            revision: 0,
            commands: Vec::new(),
        }
    }
}

impl<C> Revised<C> {
    pub(crate) fn push(&mut self, command: C) {
        self.revision = self.revision.wrapping_add(1);
        self.commands.push(command);
    }

    pub(crate) fn take(&mut self) -> (u64, Vec<C>) {
        (self.revision, std::mem::take(&mut self.commands))
    }
}
