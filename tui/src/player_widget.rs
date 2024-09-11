use std::f64;

use protocol::{playback::PlayerStatus, Duration};
use ratatui::{
    layout::{Constraint, Layout},
    prelude::BlockExt,
    style::Style,
    text::Line,
    widgets::{Block, Paragraph, Widget},
};

pub struct PlayerWiget<'a> {
    status: PlayerStatus,
    block: Option<Block<'a>>,
    style: Option<Style>,
}

impl<'a> Default for PlayerWiget<'a> {
    fn default() -> Self {
        Self::new(PlayerStatus::Stopped)
    }
}

impl<'a> PlayerWiget<'a> {
    pub fn new(status: PlayerStatus) -> Self {
        Self {
            status,
            block: None,
            style: None,
        }
    }
    pub fn block(self, block: Block<'a>) -> Self {
        Self {
            block: Some(block),
            ..self
        }
    }
    pub fn style(self, style: Style) -> Self {
        Self {
            style: Some(style),
            ..self
        }
    }
}

impl<'a> Widget for PlayerWiget<'a> {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        if let Some(style) = self.style {
            buf.set_style(area, style);
        }
        self.block.render(area, buf);
        let area = self.block.inner_if_some(area);
        if area.is_empty() {
            return;
        }
        let [text_area, gauge_area] =
            Layout::vertical(vec![Constraint::Min(1), Constraint::Fill(1)]).areas(area);
        let [title_area, duration_area] =
            Layout::horizontal(vec![Constraint::Fill(1), Constraint::Min(18)]).areas(text_area);
        if let PlayerStatus::Playing { song, position } = self.status {
            // Rendering text bar
            let title = Paragraph::new(format!("{} | {}", song.title(), song.artists().join(",")));
            let duration = Paragraph::new(format!(
                "{}/{}",
                duration_to_str(position),
                duration_to_str(song.duration())
            ))
            .right_aligned();
            title.render(title_area, buf);
            duration.render(duration_area, buf);

            // Rendering progress bar
            let song_duration: std::time::Duration = song.duration().into();
            let position: std::time::Duration = position.into();
            let ratio = position.as_secs_f64() / song_duration.as_secs_f64().max(1.0);
            let ratio = ratio.min(1.0);
            let fill_length = (gauge_area.width as f64 * ratio) as usize;
            let last_char = if ratio == 1.0 { '█' } else { '┤' };
            let first_char = if fill_length >= 1 { '█' } else { '├' };
            let mut filled_part = String::new();
            for _ in 1..fill_length {
                filled_part.push('█');
            }
            let mut empty_part = String::new();
            for _ in fill_length.max(1)..gauge_area.width.saturating_sub(1) as usize {
                empty_part.push('─');
            }
            let gauge = Line::from(format!("{first_char}{filled_part}{empty_part}{last_char}"));
            gauge.render(gauge_area, buf);
        } else {
            return;
        }
    }
}
fn duration_to_str(duration: Duration) -> String {
    let duration: std::time::Duration = duration.into();
    let hours = duration.as_secs() / 3600;
    let minutes = duration.as_secs() / 60;
    let seconds = duration.as_secs() % 60;
    if hours == 0 {
        format!("{:0width$}:{:0width$}", minutes, seconds, width = 2)
    } else {
        format!(
            "{:0width$}:{:0width$}:{:0width$}",
            hours,
            minutes,
            seconds,
            width = 2
        )
    }
}
