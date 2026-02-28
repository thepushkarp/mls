/// Thumbnail preview rendering via ratatui-image.
///
/// Renders video thumbnails in the preview pane using halfblocks
/// (or a better protocol if the terminal supports it).
use super::{App, ThumbState};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui_image::StatefulImage;

/// Render the thumbnail area in the preview pane.
pub fn render_thumbnail(frame: &mut Frame, app: &mut App, area: Rect) {
    match app.thumb_state {
        ThumbState::Loading => {
            let msg = Paragraph::new(Line::styled(
                "Loading preview...",
                Style::default().fg(Color::DarkGray),
            ));
            frame.render_widget(msg, area);
        }
        ThumbState::Ready(ref mut proto) => {
            let image = StatefulImage::default();
            frame.render_stateful_widget(image, area, proto.as_mut());
        }
        ThumbState::Failed | ThumbState::Empty => {
            let msg = Paragraph::new(Line::styled(
                "No preview",
                Style::default().fg(Color::DarkGray),
            ));
            frame.render_widget(msg, area);
        }
    }
}
