// SPDX-License-Identifier: MIT OR Apache-2.0

//! Local session lifecycle (P2-SEC-01E, O-032, P1-01G §5): the write stage
//! that «End session» must never interrupt, and the session generation that
//! ends every request and plan admitted under the old session.
//!
//! Erasing the in-memory Zabbix secrets themselves is done by their owners
//! (for example [`crate::connections::ConnectionService::erase_session_secrets`]);
//! saved drafts, profiles and the operating-system credential store are not
//! touched.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::MessageId;

/// Generation of the local browser session a request or plan was admitted
/// under. It changes when the session ends, so old work cannot write.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SessionGeneration(u64);

/// `session.end` command input: the user confirmed the dialog
/// `session.end.confirm.*`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EndSessionRequest {
    pub confirmed: bool,
}

/// Why the session was not ended. The session stays active.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionEndError {
    /// The request did not carry the user's confirmation.
    NotConfirmed,
    /// A write stage is running; ending would leave its outcome unseen.
    Writing,
}

impl SessionEndError {
    #[must_use]
    pub const fn message_id(self) -> MessageId {
        match self {
            Self::NotConfirmed => MessageId::SESSION_END_NOT_CONFIRMED,
            Self::Writing => MessageId::SESSION_END_UNAVAILABLE_WRITING,
        }
    }
}

/// A write stage cannot begin: the session it was admitted under ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionEnded;

impl SessionEnded {
    #[must_use]
    pub const fn message_id(self) -> MessageId {
        MessageId::HTTP_SECURITY_SESSION_INVALID
    }
}

#[derive(Debug, Default)]
struct SessionState {
    generation: u64,
    writes: usize,
}

/// Process-wide local session state shared by the HTTP boundary and the
/// publication use case.
#[derive(Clone, Debug, Default)]
pub struct LocalSession {
    state: Arc<Mutex<SessionState>>,
}

impl LocalSession {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Generation to attach to a newly admitted request.
    #[must_use]
    pub fn current(&self) -> SessionGeneration {
        SessionGeneration(self.lock().generation)
    }

    #[must_use]
    pub fn is_current(&self, generation: SessionGeneration) -> bool {
        self.lock().generation == generation.0
    }

    /// Whether a write stage is running (from sending the write request to
    /// its outcome or unknown outcome).
    #[must_use]
    pub fn is_writing(&self) -> bool {
        self.lock().writes > 0
    }

    /// Enter the write stage. Publication holds the returned guard from the
    /// first write request until the outcome is known or recorded as
    /// unknown; while it is held the session cannot be ended.
    pub fn begin_write(&self, admitted: SessionGeneration) -> Result<WriteStage, SessionEnded> {
        let mut state = self.lock();
        if state.generation != admitted.0 {
            return Err(SessionEnded);
        }
        state.writes += 1;
        Ok(WriteStage {
            session: self.clone(),
            generation: admitted,
        })
    }

    /// `session.end`: refused without confirmation and during a write
    /// stage; otherwise every request, plan and write admitted under the
    /// current generation is ended at once. Returns the ended generation.
    pub fn end(&self, request: EndSessionRequest) -> Result<SessionGeneration, SessionEndError> {
        if !request.confirmed {
            return Err(SessionEndError::NotConfirmed);
        }
        let mut state = self.lock();
        if state.writes > 0 {
            return Err(SessionEndError::Writing);
        }
        let ended = SessionGeneration(state.generation);
        state.generation = state.generation.wrapping_add(1);
        Ok(ended)
    }

    fn lock(&self) -> MutexGuard<'_, SessionState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Held for the duration of a write stage; dropping it ends the stage.
#[must_use = "the write stage ends when this guard is dropped"]
#[derive(Debug)]
pub struct WriteStage {
    session: LocalSession,
    generation: SessionGeneration,
}

impl WriteStage {
    /// Generation the stage was admitted under; a plan built under another
    /// generation cannot use this stage.
    #[must_use]
    pub const fn generation(&self) -> SessionGeneration {
        self.generation
    }
}

impl Drop for WriteStage {
    fn drop(&mut self) {
        let mut state = self.session.lock();
        state.writes = state.writes.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIRMED: EndSessionRequest = EndSessionRequest { confirmed: true };

    #[test]
    fn end_requires_confirmation_and_keeps_the_session() {
        let session = LocalSession::new();
        let admitted = session.current();
        assert_eq!(
            session.end(EndSessionRequest { confirmed: false }),
            Err(SessionEndError::NotConfirmed)
        );
        assert!(session.is_current(admitted));
    }

    #[test]
    fn end_is_refused_while_writing_and_the_write_is_not_interrupted() {
        let session = LocalSession::new();
        let admitted = session.current();
        let stage = session
            .begin_write(admitted)
            .unwrap_or_else(|_| panic!("write must start in the current session"));
        assert_eq!(session.end(CONFIRMED), Err(SessionEndError::Writing));
        assert_eq!(
            SessionEndError::Writing.message_id().as_str(),
            "session.end.unavailable_writing"
        );
        // The running write keeps its session until it finishes.
        assert!(session.is_current(admitted));
        assert!(session.is_writing());
        drop(stage);
        assert!(!session.is_writing());
        assert_eq!(session.end(CONFIRMED), Ok(admitted));
        assert!(!session.is_current(admitted));
    }

    #[test]
    fn work_admitted_under_an_ended_session_cannot_start_a_write() {
        let session = LocalSession::new();
        let admitted = session.current();
        assert_eq!(session.end(CONFIRMED), Ok(admitted));
        assert!(session.begin_write(admitted).is_err());
        assert!(!session.is_writing());
        let next = session.current();
        assert_ne!(next, admitted);
        assert!(session.begin_write(next).is_ok());
    }
}
