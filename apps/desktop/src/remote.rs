use std::{error::Error, fmt};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RemoteSourceState {
    Inspecting,
    Downloading {
        received_bytes: u64,
        total_bytes: Option<u64>,
    },
    Preparing,
    Ready,
    Failed {
        message: String,
    },
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RemoteSourceEvent {
    InspectionCompleted {
        total_bytes: Option<u64>,
    },
    DownloadProgressed {
        received_bytes: u64,
        total_bytes: Option<u64>,
    },
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "the preview stops during download; a future adapter will complete it"
        )
    )]
    DownloadCompleted,
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "the preview does no media preparation; a future adapter will complete it"
        )
    )]
    PreparationCompleted,
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "the deterministic preview cannot fail; future adapters can"
        )
    )]
    Failed {
        message: String,
    },
    CancelRequested,
    RetryRequested,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RemoteTransitionError {
    InvalidTransition,
    ProgressRegressed,
    ProgressExceedsTotal,
    TotalChanged,
}

impl fmt::Display for RemoteTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidTransition => "event is not valid for the current remote source state",
            Self::ProgressRegressed => "download progress cannot move backwards",
            Self::ProgressExceedsTotal => "download progress cannot exceed its total",
            Self::TotalChanged => "download total cannot change after inspection",
        };
        formatter.write_str(message)
    }
}

impl Error for RemoteTransitionError {}

#[derive(Debug)]
pub(crate) struct RemoteSourceJob {
    display_source: String,
    state: RemoteSourceState,
}

impl RemoteSourceJob {
    pub(crate) fn new(display_source: impl Into<String>) -> Self {
        Self {
            display_source: display_source.into(),
            state: RemoteSourceState::Inspecting,
        }
    }

    pub(crate) fn state(&self) -> &RemoteSourceState {
        &self.state
    }

    pub(crate) fn display_source(&self) -> &str {
        &self.display_source
    }

    pub(crate) fn can_cancel(&self) -> bool {
        matches!(
            self.state,
            RemoteSourceState::Inspecting
                | RemoteSourceState::Downloading { .. }
                | RemoteSourceState::Preparing
        )
    }

    pub(crate) fn can_retry(&self) -> bool {
        matches!(self.state, RemoteSourceState::Failed { .. })
    }

    pub(crate) fn progress_fraction(&self) -> Option<f32> {
        match self.state {
            RemoteSourceState::Downloading {
                received_bytes,
                total_bytes: Some(total_bytes),
            } if total_bytes > 0 => Some(received_bytes as f32 / total_bytes as f32),
            _ => None,
        }
    }

    pub(crate) fn apply(&mut self, event: RemoteSourceEvent) -> Result<(), RemoteTransitionError> {
        match (&self.state, event) {
            (
                RemoteSourceState::Inspecting
                | RemoteSourceState::Downloading { .. }
                | RemoteSourceState::Preparing,
                RemoteSourceEvent::Failed { message },
            ) => {
                self.state = RemoteSourceState::Failed { message };
                Ok(())
            }
            (RemoteSourceState::Failed { .. }, RemoteSourceEvent::RetryRequested) => {
                self.state = RemoteSourceState::Inspecting;
                Ok(())
            }
            (
                RemoteSourceState::Inspecting
                | RemoteSourceState::Downloading { .. }
                | RemoteSourceState::Preparing,
                RemoteSourceEvent::CancelRequested,
            ) => {
                self.state = RemoteSourceState::Cancelled;
                Ok(())
            }
            (
                RemoteSourceState::Inspecting,
                RemoteSourceEvent::InspectionCompleted { total_bytes },
            ) => {
                self.state = RemoteSourceState::Downloading {
                    received_bytes: 0,
                    total_bytes,
                };
                Ok(())
            }
            (
                RemoteSourceState::Downloading {
                    received_bytes: current_bytes,
                    total_bytes: current_total,
                },
                RemoteSourceEvent::DownloadProgressed {
                    received_bytes,
                    total_bytes,
                },
            ) => {
                if total_bytes != *current_total {
                    return Err(RemoteTransitionError::TotalChanged);
                }
                if received_bytes < *current_bytes {
                    return Err(RemoteTransitionError::ProgressRegressed);
                }
                if total_bytes.is_some_and(|total| received_bytes > total) {
                    return Err(RemoteTransitionError::ProgressExceedsTotal);
                }
                self.state = RemoteSourceState::Downloading {
                    received_bytes,
                    total_bytes,
                };
                Ok(())
            }
            (RemoteSourceState::Downloading { .. }, RemoteSourceEvent::DownloadCompleted) => {
                self.state = RemoteSourceState::Preparing;
                Ok(())
            }
            (RemoteSourceState::Preparing, RemoteSourceEvent::PreparationCompleted) => {
                self.state = RemoteSourceState::Ready;
                Ok(())
            }
            _ => Err(RemoteTransitionError::InvalidTransition),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RemoteSourceEvent, RemoteSourceJob, RemoteSourceState};

    #[test]
    fn inspected_source_starts_downloading() {
        let mut job = RemoteSourceJob::new("Preview source");

        assert_eq!(job.state(), &RemoteSourceState::Inspecting);

        job.apply(RemoteSourceEvent::InspectionCompleted {
            total_bytes: Some(1_000),
        })
        .expect("inspection completion should start downloading");

        assert_eq!(
            job.state(),
            &RemoteSourceState::Downloading {
                received_bytes: 0,
                total_bytes: Some(1_000),
            }
        );
    }

    #[test]
    fn known_download_progress_is_monotonic_and_bounded() {
        let mut job = RemoteSourceJob::new("Preview source");
        job.apply(RemoteSourceEvent::InspectionCompleted {
            total_bytes: Some(1_000),
        })
        .unwrap();

        job.apply(RemoteSourceEvent::DownloadProgressed {
            received_bytes: 420,
            total_bytes: Some(1_000),
        })
        .expect("forward progress within the total should be accepted");

        assert_eq!(job.progress_fraction(), Some(0.42));
        assert!(
            job.apply(RemoteSourceEvent::DownloadProgressed {
                received_bytes: 419,
                total_bytes: Some(1_000),
            })
            .is_err()
        );
        assert!(
            job.apply(RemoteSourceEvent::DownloadProgressed {
                received_bytes: 1_001,
                total_bytes: Some(1_000),
            })
            .is_err()
        );
    }

    #[test]
    fn active_job_can_be_cancelled_but_terminal_job_cannot() {
        let mut job = RemoteSourceJob::new("Preview source");

        assert!(job.can_cancel());
        job.apply(RemoteSourceEvent::CancelRequested)
            .expect("active inspection should be cancellable");

        assert_eq!(job.state(), &RemoteSourceState::Cancelled);
        assert!(!job.can_cancel());
        assert!(job.apply(RemoteSourceEvent::CancelRequested).is_err());
    }

    #[test]
    fn completed_download_is_prepared_before_becoming_ready() {
        let mut job = RemoteSourceJob::new("Preview source");
        job.apply(RemoteSourceEvent::InspectionCompleted { total_bytes: None })
            .unwrap();

        job.apply(RemoteSourceEvent::DownloadCompleted)
            .expect("a completed download should begin preparation");
        assert_eq!(job.state(), &RemoteSourceState::Preparing);
        assert!(job.can_cancel());

        job.apply(RemoteSourceEvent::PreparationCompleted)
            .expect("completed preparation should make media ready");
        assert_eq!(job.state(), &RemoteSourceState::Ready);
        assert!(!job.can_cancel());
    }

    #[test]
    fn failed_job_can_retry_from_inspection() {
        let mut job = RemoteSourceJob::new("Preview source");
        job.apply(RemoteSourceEvent::Failed {
            message: "provider unavailable".into(),
        })
        .expect("active work may report a failure");

        assert_eq!(
            job.state(),
            &RemoteSourceState::Failed {
                message: "provider unavailable".into(),
            }
        );
        assert!(job.can_retry());
        assert!(!job.can_cancel());

        job.apply(RemoteSourceEvent::RetryRequested)
            .expect("failed work should be retryable");
        assert_eq!(job.state(), &RemoteSourceState::Inspecting);
        assert!(!job.can_retry());
    }

    #[test]
    fn unknown_download_total_remains_indeterminate() {
        let mut job = RemoteSourceJob::new("Preview source");
        job.apply(RemoteSourceEvent::InspectionCompleted { total_bytes: None })
            .unwrap();
        job.apply(RemoteSourceEvent::DownloadProgressed {
            received_bytes: 4_200,
            total_bytes: None,
        })
        .unwrap();

        assert_eq!(job.progress_fraction(), None);
    }
}
