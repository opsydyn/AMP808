use std::error::Error;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResumeAfterSeek {
    Paused,
    Playing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransportState {
    Empty,
    Loading,
    Ready,
    Playing,
    Paused,
    Buffering { resume_after_seek: ResumeAfterSeek },
    Ended,
    Failed(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransportEvent {
    LoadRequested,
    Loaded,
    PlayRequested,
    PauseRequested,
    SeekRequested,
    SeekReady,
    EndReached,
    Failed(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransitionError {
    from: TransportState,
    event: TransportEvent,
}

impl TransitionError {
    pub fn from(&self) -> &TransportState {
        &self.from
    }

    pub fn event(&self) -> TransportEvent {
        self.event.clone()
    }
}

impl fmt::Display for TransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot apply transport event {:?} while in state {:?}",
            self.event, self.from
        )
    }
}

impl Error for TransitionError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Transport {
    state: TransportState,
}

impl Transport {
    pub const fn new() -> Self {
        Self {
            state: TransportState::Empty,
        }
    }

    pub const fn state(&self) -> &TransportState {
        &self.state
    }

    pub fn apply(&mut self, event: TransportEvent) -> Result<(), TransitionError> {
        let next = match (&self.state, &event) {
            (_, TransportEvent::Failed(message)) => TransportState::Failed(message.clone()),
            (TransportState::Empty, TransportEvent::LoadRequested)
            | (TransportState::Ready, TransportEvent::LoadRequested)
            | (TransportState::Playing, TransportEvent::LoadRequested)
            | (TransportState::Paused, TransportEvent::LoadRequested)
            | (TransportState::Ended, TransportEvent::LoadRequested)
            | (TransportState::Failed(_), TransportEvent::LoadRequested) => TransportState::Loading,
            (TransportState::Loading, TransportEvent::Loaded) => TransportState::Ready,
            (TransportState::Ready, TransportEvent::PlayRequested)
            | (TransportState::Paused, TransportEvent::PlayRequested) => TransportState::Playing,
            (TransportState::Playing, TransportEvent::PauseRequested) => TransportState::Paused,
            (TransportState::Playing, TransportEvent::SeekRequested) => TransportState::Buffering {
                resume_after_seek: ResumeAfterSeek::Playing,
            },
            (
                TransportState::Ready | TransportState::Paused | TransportState::Ended,
                TransportEvent::SeekRequested,
            ) => TransportState::Buffering {
                resume_after_seek: ResumeAfterSeek::Paused,
            },
            (
                TransportState::Buffering {
                    resume_after_seek: ResumeAfterSeek::Playing,
                },
                TransportEvent::SeekReady,
            ) => TransportState::Playing,
            (
                TransportState::Buffering {
                    resume_after_seek: ResumeAfterSeek::Paused,
                },
                TransportEvent::SeekReady,
            ) => TransportState::Paused,
            (TransportState::Playing, TransportEvent::EndReached) => TransportState::Ended,
            _ => {
                return Err(TransitionError {
                    from: self.state.clone(),
                    event,
                });
            }
        };

        self.state = next;
        Ok(())
    }
}

impl Default for Transport {
    fn default() -> Self {
        Self::new()
    }
}
