use std::io;

use amp808_core::web_audio::{HostedAudioIssue, WebAudioSource};
use ratzilla::ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use ratzilla::{WebGl2Backend, WebRenderer};

fn main() -> io::Result<()> {
    let backend = WebGl2Backend::new()?;
    let terminal = Terminal::new(backend)?;
    let source = WebAudioSource::hosted_url("https://example.com/audio.mp3");
    let cors_message = HostedAudioIssue::CorsRequired.user_message();

    terminal.draw_web(move |frame| {
        let area = frame.area();
        let block = Block::default()
            .title("AMP808 WEB 808")
            .title_style(
                Style::default()
                    .fg(Color::Rgb(0xff, 0x7a, 0x45))
                    .add_modifier(Modifier::BOLD),
            )
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(0xf6, 0xa6, 0x23)));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Length(2),
                Constraint::Min(1),
            ])
            .split(inner);

        let title = Paragraph::new(Text::from(Line::from(vec![
            Span::styled("Source: ", Style::default().fg(Color::Gray)),
            Span::styled(source.label(), Style::default().fg(Color::White)),
        ])))
        .alignment(Alignment::Center);
        frame.render_widget(title, rows[0]);

        let cors = Paragraph::new(Text::from(Line::from(Span::styled(
            cors_message,
            Style::default().fg(Color::Red),
        ))))
        .alignment(Alignment::Center);
        frame.render_widget(cors, rows[1]);

        let body = Paragraph::new(Text::from(Line::from(Span::styled(
            "WebGL2 Ratzilla shell ready for browser audio wiring",
            Style::default().fg(Color::Rgb(0xc9, 0xc9, 0xc9)),
        ))))
        .alignment(Alignment::Center);
        frame.render_widget(body, rows[2]);
    });

    Ok(())
}
