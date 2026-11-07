use std::path::{Path, PathBuf};

use crate::remote::{RemoteSourceEvent, RemoteSourceJob, RemoteSourceState, RemoteTransitionError};
use crate::yt_dlp::YtDlpEvent;

pub(crate) struct RemoteTransferCoordinator {
    job: RemoteSourceJob,
    completed_path: Option<PathBuf>,
}

impl RemoteTransferCoordinator {
    pub(crate) fn new(display_source: impl Into<String>) -> Self {
        Self {
            job: RemoteSourceJob::new(display_source),
            completed_path: None,
        }
    }

    pub(crate) const fn job(&self) -> &RemoteSourceJob {
        &self.job
    }

    pub(crate) fn completed_path(&self) -> Option<&Path> {
        self.completed_path.as_deref()
    }

    pub(crate) fn apply(&mut self, event: YtDlpEvent) -> Result<(), RemoteTransitionError> {
        match event {
            YtDlpEvent::Progress {
                transferred_bytes,
                total_bytes,
            } => {
                if matches!(self.job.state(), RemoteSourceState::Inspecting) {
                    self.job
                        .apply(RemoteSourceEvent::InspectionCompleted { total_bytes })?;
                }
                let visible_total = match self.job.state() {
                    RemoteSourceState::Downloading { total_bytes, .. } => *total_bytes,
                    _ => return Err(RemoteTransitionError::InvalidTransition),
                };
                self.job.apply(RemoteSourceEvent::DownloadProgressed {
                    received_bytes: transferred_bytes,
                    total_bytes: visible_total,
                })
            }
            YtDlpEvent::Completed { path } => {
                if matches!(self.job.state(), RemoteSourceState::Inspecting) {
                    self.job
                        .apply(RemoteSourceEvent::InspectionCompleted { total_bytes: None })?;
                }
                self.job.apply(RemoteSourceEvent::DownloadCompleted)?;
                self.job.apply(RemoteSourceEvent::PreparationCompleted)?;
                self.completed_path = Some(path);
                Ok(())
            }
            YtDlpEvent::Cancelled => self.job.apply(RemoteSourceEvent::CancelRequested),
            YtDlpEvent::Failed(error) => self.job.apply(RemoteSourceEvent::Failed {
                message: error.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::remote::RemoteSourceState;
    use crate::yt_dlp::{YtDlpError, YtDlpEvent};

    use super::RemoteTransferCoordinator;

    #[test]
    fn first_progress_starts_transfer_and_keeps_its_initial_total() {
        let mut coordinator = RemoteTransferCoordinator::new("yt-dlp source");

        coordinator
            .apply(YtDlpEvent::Progress {
                transferred_bytes: 420,
                total_bytes: Some(1_000),
            })
            .expect("first adapter progress should enter the transfer state");
        coordinator
            .apply(YtDlpEvent::Progress {
                transferred_bytes: 840,
                total_bytes: Some(1_100),
            })
            .expect("later external total changes should not break visible progress");

        assert_eq!(
            coordinator.job().state(),
            &RemoteSourceState::Downloading {
                received_bytes: 840,
                total_bytes: Some(1_000),
            }
        );
    }

    #[test]
    fn completed_adapter_result_becomes_a_validated_ready_path() {
        let mut coordinator = RemoteTransferCoordinator::new("yt-dlp source");
        let path = PathBuf::from("/Music/Cosmic Signal.opus");

        coordinator
            .apply(YtDlpEvent::Completed { path: path.clone() })
            .expect("a validated adapter result should complete the remote job");

        assert_eq!(coordinator.job().state(), &RemoteSourceState::Ready);
        assert_eq!(coordinator.completed_path(), Some(path.as_path()));
    }

    #[test]
    fn confirmed_adapter_cancellation_marks_the_job_cancelled() {
        let mut coordinator = RemoteTransferCoordinator::new("yt-dlp source");

        coordinator
            .apply(YtDlpEvent::Cancelled)
            .expect("adapter cancellation should terminate an active job");

        assert_eq!(coordinator.job().state(), &RemoteSourceState::Cancelled);
    }

    #[test]
    fn adapter_failure_is_presented_by_the_provider_agnostic_job() {
        let mut coordinator = RemoteTransferCoordinator::new("yt-dlp source");

        coordinator
            .apply(YtDlpEvent::Failed(YtDlpError::MissingOutput))
            .expect("adapter failure should terminate an active job");

        assert_eq!(
            coordinator.job().state(),
            &RemoteSourceState::Failed {
                message: "yt-dlp did not report a completed file".to_owned(),
            }
        );
    }
}
