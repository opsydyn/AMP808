use crossterm::event::{KeyCode, KeyEvent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiCommand {
    Load { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandInputResult {
    Continue,
    Submit(String),
    Cancel,
}

pub fn parse_command(input: &str) -> Result<UiCommand, String> {
    let mut s = input.trim();
    if let Some(rest) = s.strip_prefix(':') {
        s = rest.trim_start();
    }

    if s.is_empty() {
        return Err("empty command".to_string());
    }

    let (name, args) = if let Some((name, args)) = s.split_once(char::is_whitespace) {
        (name, args.trim())
    } else {
        (s, "")
    };

    match name.to_ascii_lowercase().as_str() {
        "load" => {
            if args.is_empty() {
                Err("usage: :load <path-to-playlist.m3u>".to_string())
            } else {
                Ok(UiCommand::Load {
                    path: args.to_string(),
                })
            }
        }
        _ => Err(format!("unknown command: {name}")),
    }
}

pub fn handle_command_input_key(input: &mut String, key: KeyEvent) -> CommandInputResult {
    match key.code {
        KeyCode::Esc => {
            input.clear();
            CommandInputResult::Cancel
        }
        KeyCode::Enter => {
            let out = input.clone();
            input.clear();
            CommandInputResult::Submit(out)
        }
        KeyCode::Backspace => {
            input.pop();
            CommandInputResult::Continue
        }
        KeyCode::Char(c) => {
            input.push(c);
            CommandInputResult::Continue
        }
        _ => CommandInputResult::Continue,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    #[test]
    fn parse_load_command_with_path() {
        let cmd = parse_command(":load ./playlist.m3u").unwrap();
        assert_eq!(
            cmd,
            UiCommand::Load {
                path: "./playlist.m3u".to_string(),
            }
        );
    }

    #[test]
    fn command_input_submit_and_clear_buffer() {
        let mut input = "load ./playlist.m3u".to_string();
        let result = handle_command_input_key(
            &mut input,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );

        assert_eq!(
            result,
            CommandInputResult::Submit("load ./playlist.m3u".to_string())
        );
        assert!(input.is_empty());
    }

    #[test]
    fn parse_load_command_requires_path() {
        let err = parse_command(":load").unwrap_err();
        assert!(err.contains("usage"));
    }

    #[test]
    fn parse_rejects_unknown_command() {
        let err = parse_command(":rm -rf /").unwrap_err();
        assert!(err.contains("unknown command"));
    }

    #[test]
    fn command_input_escape_cancels_and_clears() {
        let mut input = "load ./playlist.m3u".to_string();
        let result =
            handle_command_input_key(&mut input, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(result, CommandInputResult::Cancel);
        assert!(input.is_empty());
    }
}
