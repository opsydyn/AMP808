use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::io::{BufRead as _, BufReader, Read as _};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const AUDIO_FORMAT: &str = "bestaudio[protocol=https]/bestaudio[protocol=http]/bestaudio";
const OUTPUT_TEMPLATE: &str = "%(title)s [%(id)s].%(ext)s";
const PROGRESS_TEMPLATE: &str = "download:AMP808_PROGRESS:%(progress.downloaded_bytes)s:%(progress.total_bytes,progress.total_bytes_estimate|)s";
const FINAL_PATH_TEMPLATE: &str = "after_move:AMP808_FILE:%(filepath)s";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct YtDlpRequest {
    source_url: String,
    destination: PathBuf,
}

impl YtDlpRequest {
    pub(crate) fn new(source_url: String, destination: PathBuf) -> Result<Self, YtDlpError> {
        let source_url = source_url.trim();
        if source_url.is_empty() {
            return Err(YtDlpError::EmptySource);
        }
        if !source_url.starts_with("https://") && !source_url.starts_with("http://") {
            return Err(YtDlpError::InvalidSource(source_url.to_owned()));
        }
        if !destination.is_dir() {
            return Err(YtDlpError::InvalidDestination(destination));
        }
        Ok(Self {
            source_url: source_url.to_owned(),
            destination,
        })
    }

    fn command_args(&self) -> Vec<OsString> {
        [
            OsString::from("--ignore-config"),
            OsString::from("--no-playlist"),
            OsString::from("--newline"),
            OsString::from("--progress"),
            OsString::from("--format"),
            OsString::from(AUDIO_FORMAT),
            OsString::from("--paths"),
            self.destination.as_os_str().to_owned(),
            OsString::from("--output"),
            OsString::from(OUTPUT_TEMPLATE),
            OsString::from("--trim-filenames"),
            OsString::from("180"),
            OsString::from("--progress-template"),
            OsString::from(PROGRESS_TEMPLATE),
            OsString::from("--print"),
            OsString::from(FINAL_PATH_TEMPLATE),
            OsString::from(&self.source_url),
        ]
        .into()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum YtDlpEvent {
    Progress {
        transferred_bytes: u64,
        total_bytes: Option<u64>,
    },
    Completed {
        path: PathBuf,
    },
    Cancelled,
    Failed(YtDlpError),
}

#[derive(Clone)]
pub(crate) struct YtDlpCancellation {
    requested: Arc<AtomicBool>,
}

impl YtDlpCancellation {
    pub(crate) fn cancel(&self) {
        self.requested.store(true, Ordering::Release);
    }
}

pub(crate) struct YtDlpTransfer {
    receiver: Receiver<YtDlpEvent>,
    cancellation: YtDlpCancellation,
    worker: Option<JoinHandle<()>>,
}

impl YtDlpTransfer {
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "the adapter is wired to GPUI in the next approved slice"
        )
    )]
    pub(crate) fn start(request: YtDlpRequest) -> Result<Self, YtDlpError> {
        Self::start_with_program(request, PathBuf::from("yt-dlp"))
    }

    fn start_with_program(request: YtDlpRequest, program: PathBuf) -> Result<Self, YtDlpError> {
        let (sender, receiver) = mpsc::channel();
        let cancellation = YtDlpCancellation {
            requested: Arc::new(AtomicBool::new(false)),
        };
        let worker_cancellation = Arc::clone(&cancellation.requested);
        let worker = thread::Builder::new()
            .name("amp808-yt-dlp".to_owned())
            .spawn(move || run_transfer(request, program, &sender, &worker_cancellation))
            .map_err(|error| YtDlpError::StartWorker(error.to_string()))?;
        Ok(Self {
            receiver,
            cancellation,
            worker: Some(worker),
        })
    }

    pub(crate) fn cancellation_handle(&self) -> YtDlpCancellation {
        self.cancellation.clone()
    }

    pub(crate) fn recv_timeout(&self, timeout: Duration) -> Result<YtDlpEvent, RecvTimeoutError> {
        self.receiver.recv_timeout(timeout)
    }
}

impl Drop for YtDlpTransfer {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum YtDlpError {
    EmptySource,
    InvalidSource(String),
    InvalidDestination(PathBuf),
    StartWorker(String),
    StartProcess(String),
    ReadOutput(String),
    MalformedProgress(String),
    ProcessFailed { code: Option<i32>, stderr: String },
    MissingOutput,
    InvalidOutput(PathBuf),
}

impl fmt::Display for YtDlpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySource => formatter.write_str("yt-dlp source URL is empty"),
            Self::InvalidSource(source) => {
                write!(formatter, "yt-dlp source is not an HTTP(S) URL: {source}")
            }
            Self::InvalidDestination(path) => {
                write!(
                    formatter,
                    "yt-dlp destination is not a directory: {}",
                    path.display()
                )
            }
            Self::StartWorker(message) => write!(formatter, "starting yt-dlp worker: {message}"),
            Self::StartProcess(message) => write!(formatter, "starting yt-dlp: {message}"),
            Self::ReadOutput(message) => write!(formatter, "reading yt-dlp output: {message}"),
            Self::MalformedProgress(line) => {
                write!(formatter, "yt-dlp reported malformed progress: {line}")
            }
            Self::ProcessFailed { code, stderr } => {
                write!(formatter, "yt-dlp exited with {code:?}: {stderr}")
            }
            Self::MissingOutput => formatter.write_str("yt-dlp did not report a completed file"),
            Self::InvalidOutput(path) => {
                write!(
                    formatter,
                    "yt-dlp reported an invalid output: {}",
                    path.display()
                )
            }
        }
    }
}

impl Error for YtDlpError {}

#[derive(Clone, Debug, Eq, PartialEq)]
enum OutputRecord {
    Progress {
        transferred_bytes: u64,
        total_bytes: Option<u64>,
    },
    FinalPath(PathBuf),
    MalformedProgress(String),
}

fn parse_stdout_line(line: &str) -> Option<OutputRecord> {
    if let Some(path) = line.strip_prefix("AMP808_FILE:") {
        return (!path.is_empty()).then(|| OutputRecord::FinalPath(PathBuf::from(path)));
    }
    let fields = line.strip_prefix("AMP808_PROGRESS:")?;
    let Some((transferred, total)) = fields.split_once(':') else {
        return Some(OutputRecord::MalformedProgress(line.to_owned()));
    };
    let Ok(transferred_bytes) = transferred.parse() else {
        return Some(OutputRecord::MalformedProgress(line.to_owned()));
    };
    let total_bytes = if total.is_empty() {
        None
    } else {
        let Ok(total_bytes) = total.parse() else {
            return Some(OutputRecord::MalformedProgress(line.to_owned()));
        };
        Some(total_bytes)
    };
    Some(OutputRecord::Progress {
        transferred_bytes,
        total_bytes,
    })
}

fn run_transfer(
    request: YtDlpRequest,
    program: PathBuf,
    sender: &mpsc::Sender<YtDlpEvent>,
    cancellation: &AtomicBool,
) {
    let mut child = match Command::new(program)
        .args(request.command_args())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            let _ = sender.send(YtDlpEvent::Failed(YtDlpError::StartProcess(
                error.to_string(),
            )));
            return;
        }
    };

    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        let _ = sender.send(YtDlpEvent::Failed(YtDlpError::ReadOutput(
            "stdout pipe is unavailable".to_owned(),
        )));
        return;
    };
    let (output_sender, output_receiver) = mpsc::channel();
    let mut output_worker = Some(thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(line) => {
                    if let Some(record) = parse_stdout_line(&line) {
                        let _ = output_sender.send(Ok(record));
                    }
                }
                Err(error) => {
                    let _ = output_sender.send(Err(error.to_string()));
                    return;
                }
            }
        }
    }));
    let mut stderr_worker = child.stderr.take().map(|mut stderr| {
        thread::spawn(move || {
            let mut message = String::new();
            let _ = stderr.read_to_string(&mut message);
            message
        })
    });

    let mut final_path = None;
    let status = loop {
        if cancellation.load(Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait();
            join_output(&mut output_worker);
            let _ = join_stderr(&mut stderr_worker);
            let _ = sender.send(YtDlpEvent::Cancelled);
            return;
        }

        match output_receiver.recv_timeout(Duration::from_millis(10)) {
            Ok(record) => {
                if let Err(error) = handle_output_record(record, sender, &mut final_path) {
                    let _ = child.kill();
                    let _ = child.wait();
                    join_output(&mut output_worker);
                    let _ = join_stderr(&mut stderr_worker);
                    let _ = sender.send(YtDlpEvent::Failed(error));
                    return;
                }
            }
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {}
        }

        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                join_output(&mut output_worker);
                let _ = join_stderr(&mut stderr_worker);
                let _ = sender.send(YtDlpEvent::Failed(YtDlpError::ReadOutput(
                    error.to_string(),
                )));
                return;
            }
        }
    };

    join_output(&mut output_worker);
    for record in output_receiver {
        if let Err(error) = handle_output_record(record, sender, &mut final_path) {
            let _ = join_stderr(&mut stderr_worker);
            let _ = sender.send(YtDlpEvent::Failed(error));
            return;
        }
    }
    let stderr = join_stderr(&mut stderr_worker);
    if !status.success() {
        let _ = sender.send(YtDlpEvent::Failed(YtDlpError::ProcessFailed {
            code: status.code(),
            stderr: stderr.trim().to_owned(),
        }));
        return;
    }

    let Some(path) = final_path else {
        let _ = sender.send(YtDlpEvent::Failed(YtDlpError::MissingOutput));
        return;
    };
    let Ok(destination) = request.destination.canonicalize() else {
        let _ = sender.send(YtDlpEvent::Failed(YtDlpError::InvalidOutput(path)));
        return;
    };
    let Ok(path) = path.canonicalize() else {
        let _ = sender.send(YtDlpEvent::Failed(YtDlpError::InvalidOutput(path)));
        return;
    };
    if !path.is_file() || !path.starts_with(&destination) {
        let _ = sender.send(YtDlpEvent::Failed(YtDlpError::InvalidOutput(path)));
        return;
    }
    let _ = sender.send(YtDlpEvent::Completed { path });
}

fn handle_output_record(
    record: Result<OutputRecord, String>,
    sender: &mpsc::Sender<YtDlpEvent>,
    final_path: &mut Option<PathBuf>,
) -> Result<(), YtDlpError> {
    match record {
        Ok(OutputRecord::Progress {
            transferred_bytes,
            total_bytes,
        }) => {
            let _ = sender.send(YtDlpEvent::Progress {
                transferred_bytes,
                total_bytes,
            });
            Ok(())
        }
        Ok(OutputRecord::FinalPath(path)) => {
            *final_path = Some(path);
            Ok(())
        }
        Ok(OutputRecord::MalformedProgress(line)) => Err(YtDlpError::MalformedProgress(line)),
        Err(message) => Err(YtDlpError::ReadOutput(message)),
    }
}

fn join_output(worker: &mut Option<JoinHandle<()>>) {
    if let Some(worker) = worker.take() {
        let _ = worker.join();
    }
}

fn join_stderr(worker: &mut Option<JoinHandle<String>>) -> String {
    worker
        .take()
        .and_then(|worker| worker.join().ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use std::time::Duration;

    use tempfile::tempdir;

    use super::{YtDlpError, YtDlpEvent, YtDlpRequest, YtDlpTransfer};

    #[test]
    fn request_builds_a_config_isolated_single_file_transfer() {
        let _production_start: fn(YtDlpRequest) -> Result<YtDlpTransfer, YtDlpError> =
            YtDlpTransfer::start;
        let destination = tempdir().expect("destination directory is created");
        let request = YtDlpRequest::new(
            "https://soundcloud.com/artist/track".to_owned(),
            destination.path().to_path_buf(),
        )
        .expect("existing destination is valid");

        let expected: Vec<OsString> = vec![
            "--ignore-config".into(),
            "--no-playlist".into(),
            "--newline".into(),
            "--progress".into(),
            "--format".into(),
            "bestaudio[protocol=https]/bestaudio[protocol=http]/bestaudio".into(),
            "--paths".into(),
            destination.path().as_os_str().to_owned(),
            "--output".into(),
            "%(title)s [%(id)s].%(ext)s".into(),
            "--trim-filenames".into(),
            "180".into(),
            "--progress-template".into(),
            "download:AMP808_PROGRESS:%(progress.downloaded_bytes)s:%(progress.total_bytes,progress.total_bytes_estimate|)s".into(),
            "--print".into(),
            "after_move:AMP808_FILE:%(filepath)s".into(),
            "https://soundcloud.com/artist/track".into(),
        ];

        assert_eq!(request.command_args(), expected.as_slice());
    }

    #[test]
    fn request_rejects_empty_source_and_non_directory_destination() {
        let destination = tempdir().expect("destination directory is created");
        assert_eq!(
            YtDlpRequest::new("  ".to_owned(), destination.path().to_path_buf()),
            Err(YtDlpError::EmptySource)
        );
        assert_eq!(
            YtDlpRequest::new(
                "--exec=touch /tmp/amp808-should-not-exist".to_owned(),
                destination.path().to_path_buf()
            ),
            Err(YtDlpError::InvalidSource(
                "--exec=touch /tmp/amp808-should-not-exist".to_owned()
            ))
        );

        let file = destination.path().join("not-a-directory");
        fs::write(&file, "fixture").unwrap();
        assert_eq!(
            YtDlpRequest::new(
                "https://soundcloud.com/artist/track".to_owned(),
                file.clone()
            ),
            Err(YtDlpError::InvalidDestination(file))
        );
    }

    #[test]
    fn progress_record_maps_to_typed_bytes() {
        assert_eq!(
            super::parse_stdout_line("AMP808_PROGRESS:420:1000"),
            Some(super::OutputRecord::Progress {
                transferred_bytes: 420,
                total_bytes: Some(1_000),
            })
        );
        assert_eq!(
            super::parse_stdout_line("AMP808_PROGRESS:420:"),
            Some(super::OutputRecord::Progress {
                transferred_bytes: 420,
                total_bytes: None,
            })
        );
        assert_eq!(super::parse_stdout_line("[download] 42%"), None);
    }

    #[test]
    fn final_path_record_preserves_path_characters() {
        assert_eq!(
            super::parse_stdout_line("AMP808_FILE:/Music/A: B.opus"),
            Some(super::OutputRecord::FinalPath(std::path::PathBuf::from(
                "/Music/A: B.opus"
            )))
        );
    }

    #[test]
    fn malformed_progress_is_an_explicit_protocol_error() {
        assert_eq!(
            super::parse_stdout_line("AMP808_PROGRESS:not-a-number:1000"),
            Some(super::OutputRecord::MalformedProgress(
                "AMP808_PROGRESS:not-a-number:1000".to_owned()
            ))
        );
    }

    #[test]
    fn fake_process_reports_malformed_progress_as_a_failure() {
        let fixture = tempdir().expect("fixture directory is created");
        let destination = tempdir().expect("destination directory is created");
        let executable = fixture.path().join("fake-yt-dlp");
        fs::write(
            &executable,
            "#!/bin/sh\nprintf 'AMP808_PROGRESS:not-a-number:1000\\n'\n",
        )
        .expect("fake executable is written");
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();
        let request = YtDlpRequest::new(
            "https://soundcloud.com/artist/track".to_owned(),
            destination.path().to_path_buf(),
        )
        .unwrap();

        let transfer =
            YtDlpTransfer::start_with_program(request, executable).expect("fake transfer starts");

        assert_eq!(
            transfer.recv_timeout(Duration::from_secs(2)).unwrap(),
            YtDlpEvent::Failed(YtDlpError::MalformedProgress(
                "AMP808_PROGRESS:not-a-number:1000".to_owned()
            ))
        );
    }

    #[test]
    fn cancellation_terminates_a_silent_process_and_emits_cancelled_once() {
        let fixture = tempdir().expect("fixture directory is created");
        let destination = tempdir().expect("destination directory is created");
        let executable = fixture.path().join("fake-yt-dlp");
        fs::write(&executable, "#!/bin/sh\nexec sleep 30\n").expect("fake executable is written");
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();
        let request = YtDlpRequest::new(
            "https://soundcloud.com/artist/track".to_owned(),
            destination.path().to_path_buf(),
        )
        .unwrap();
        let transfer =
            YtDlpTransfer::start_with_program(request, executable).expect("fake transfer starts");

        let cancellation = transfer.cancellation_handle();
        cancellation.clone().cancel();

        assert_eq!(
            transfer.recv_timeout(Duration::from_secs(2)).unwrap(),
            YtDlpEvent::Cancelled
        );
        assert_eq!(
            transfer.recv_timeout(Duration::from_millis(100)),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
        );
    }

    #[test]
    fn missing_executable_is_a_structured_start_failure() {
        let fixture = tempdir().expect("fixture directory is created");
        let destination = tempdir().expect("destination directory is created");
        let request = YtDlpRequest::new(
            "https://soundcloud.com/artist/track".to_owned(),
            destination.path().to_path_buf(),
        )
        .unwrap();
        let transfer =
            YtDlpTransfer::start_with_program(request, fixture.path().join("missing-yt-dlp"))
                .expect("the worker starts before the executable is resolved");

        let YtDlpEvent::Failed(YtDlpError::StartProcess(message)) =
            transfer.recv_timeout(Duration::from_secs(2)).unwrap()
        else {
            panic!("a missing executable should be reported as a start failure");
        };
        assert!(!message.is_empty());
    }

    #[test]
    fn non_zero_exit_preserves_code_and_stderr() {
        let fixture = tempdir().expect("fixture directory is created");
        let destination = tempdir().expect("destination directory is created");
        let executable = fixture.path().join("fake-yt-dlp");
        fs::write(
            &executable,
            "#!/bin/sh\nprintf 'provider rejected request\\n' >&2\nexit 7\n",
        )
        .expect("fake executable is written");
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();
        let request = YtDlpRequest::new(
            "https://soundcloud.com/artist/track".to_owned(),
            destination.path().to_path_buf(),
        )
        .unwrap();
        let transfer =
            YtDlpTransfer::start_with_program(request, executable).expect("fake transfer starts");

        assert_eq!(
            transfer.recv_timeout(Duration::from_secs(2)).unwrap(),
            YtDlpEvent::Failed(YtDlpError::ProcessFailed {
                code: Some(7),
                stderr: "provider rejected request".to_owned(),
            })
        );
    }

    #[test]
    fn successful_exit_without_a_reported_file_is_a_failure() {
        let fixture = tempdir().expect("fixture directory is created");
        let destination = tempdir().expect("destination directory is created");
        let executable = fixture.path().join("fake-yt-dlp");
        fs::write(&executable, "#!/bin/sh\nexit 0\n").expect("fake executable is written");
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();
        let request = YtDlpRequest::new(
            "https://soundcloud.com/artist/track".to_owned(),
            destination.path().to_path_buf(),
        )
        .unwrap();
        let transfer =
            YtDlpTransfer::start_with_program(request, executable).expect("fake transfer starts");

        assert_eq!(
            transfer.recv_timeout(Duration::from_secs(2)).unwrap(),
            YtDlpEvent::Failed(YtDlpError::MissingOutput)
        );
    }

    #[test]
    fn reported_file_outside_destination_is_rejected() {
        let fixture = tempdir().expect("fixture directory is created");
        let destination = tempdir().expect("destination directory is created");
        let executable = fixture.path().join("fake-yt-dlp");
        fs::write(
            &executable,
            r#"#!/bin/sh
output="$(dirname "$0")/escaped.opus"
printf 'fake audio' > "$output"
printf 'AMP808_FILE:%s\n' "$output"
"#,
        )
        .expect("fake executable is written");
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();
        let request = YtDlpRequest::new(
            "https://soundcloud.com/artist/track".to_owned(),
            destination.path().to_path_buf(),
        )
        .unwrap();
        let transfer =
            YtDlpTransfer::start_with_program(request, executable).expect("fake transfer starts");
        let escaped = fixture.path().join("escaped.opus");

        assert_eq!(
            transfer.recv_timeout(Duration::from_secs(2)).unwrap(),
            YtDlpEvent::Failed(YtDlpError::InvalidOutput(escaped.canonicalize().unwrap()))
        );
    }

    #[test]
    fn fake_process_emits_progress_and_a_validated_complete_file() {
        let fixture = tempdir().expect("fixture directory is created");
        let destination = tempdir().expect("destination directory is created");
        let executable = fixture.path().join("fake-yt-dlp");
        fs::write(
            &executable,
            r#"#!/bin/sh
destination=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--paths" ]; then
    shift
    destination="$1"
  fi
  shift
done
output="$destination/Cosmic Signal [fixture].opus"
printf 'fake audio' > "$output"
printf 'AMP808_PROGRESS:420:1000\n'
printf 'AMP808_FILE:%s\n' "$output"
"#,
        )
        .expect("fake executable is written");
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();
        let request = YtDlpRequest::new(
            "https://soundcloud.com/artist/track".to_owned(),
            destination.path().to_path_buf(),
        )
        .unwrap();

        let transfer =
            YtDlpTransfer::start_with_program(request, executable).expect("fake transfer starts");

        assert_eq!(
            transfer.recv_timeout(Duration::from_secs(2)).unwrap(),
            YtDlpEvent::Progress {
                transferred_bytes: 420,
                total_bytes: Some(1_000),
            }
        );
        let YtDlpEvent::Completed { path } = transfer.recv_timeout(Duration::from_secs(2)).unwrap()
        else {
            panic!("fake transfer should complete after progress");
        };
        assert!(path.is_file());
        assert!(path.starts_with(destination.path().canonicalize().unwrap()));
    }
}
