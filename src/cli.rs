use anyhow::{Result, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Local,
    MusicApp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliArgs {
    pub backend: BackendKind,
    pub inputs: Vec<String>,
}

pub fn parse_args<I, S>(args: I) -> Result<CliArgs>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut backend = BackendKind::Local;
    let mut inputs = Vec::new();
    let mut iter = args.into_iter().map(Into::into);

    while let Some(arg) = iter.next() {
        if arg == "--backend" {
            let Some(value) = iter.next() else {
                bail!("--backend requires one of: local, music-app");
            };
            backend = parse_backend(&value)?;
            continue;
        }

        inputs.push(arg);
    }

    if backend == BackendKind::MusicApp && !inputs.is_empty() {
        bail!("music-app backend does not accept media paths or URLs");
    }

    Ok(CliArgs { backend, inputs })
}

fn parse_backend(value: &str) -> Result<BackendKind> {
    match value {
        "local" => Ok(BackendKind::Local),
        "music-app" => Ok(BackendKind::MusicApp),
        other => bail!("unknown backend '{other}' (expected: local, music-app)"),
    }
}

#[cfg(test)]
mod tests {
    use super::{BackendKind, parse_args};

    #[test]
    fn defaults_to_local_backend() {
        let cli = parse_args(["Alive.mp3"]).unwrap();

        assert_eq!(cli.backend, BackendKind::Local);
        assert_eq!(cli.inputs, vec!["Alive.mp3"]);
    }

    #[test]
    fn accepts_explicit_music_app_backend_without_inputs() {
        let cli = parse_args(["--backend", "music-app"]).unwrap();

        assert_eq!(cli.backend, BackendKind::MusicApp);
        assert!(cli.inputs.is_empty());
    }

    #[test]
    fn rejects_missing_backend_value() {
        let err = parse_args(["--backend"]).unwrap_err();

        assert!(
            err.to_string()
                .contains("--backend requires one of: local, music-app")
        );
    }

    #[test]
    fn rejects_unknown_backend_value() {
        let err = parse_args(["--backend", "spotify"]).unwrap_err();

        assert!(err.to_string().contains("unknown backend 'spotify'"));
    }

    #[test]
    fn rejects_media_inputs_in_music_app_mode() {
        let err = parse_args(["--backend", "music-app", "Alive.mp3"]).unwrap_err();

        assert!(
            err.to_string()
                .contains("music-app backend does not accept media paths or URLs")
        );
    }
}
