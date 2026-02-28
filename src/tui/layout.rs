/// TUI layout — Miller column layout with metadata panel.
///
/// Three panes: parent dir | current file list | preview (thumbnail + metadata).
/// Footer shows contextual keybindings.
use super::App;
use crate::types::{
    format_bitrate, format_duration, format_size, MediaKind,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, List, ListItem, Paragraph, Wrap,
};

/// Main render function — lays out all panes.
pub fn render(frame: &mut Frame, app: &App) {
    let size = frame.area();

    // Top-level: main area + footer
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),    // main area
            Constraint::Length(2), // metadata bar + keybindings
        ])
        .split(size);

    let main_area = outer[0];
    let footer_area = outer[1];

    // Main area: miller columns
    if app.show_metadata {
        let main_split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(5),    // miller columns
                Constraint::Length(2), // metadata bar
            ])
            .split(main_area);

        render_miller_columns(frame, app, main_split[0]);
        render_metadata_bar(frame, app, main_split[1]);
    } else {
        render_miller_columns(frame, app, main_area);
    }

    render_footer(frame, app, footer_area);

    // Overlays
    if app.show_help {
        render_help_overlay(frame, size);
    }

    if app.filter_active {
        render_filter_input(frame, size);
    }
}

fn render_miller_columns(frame: &mut Frame, app: &App, area: Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20), // parent
            Constraint::Percentage(45), // current
            Constraint::Percentage(35), // preview
        ])
        .split(area);

    render_parent_pane(frame, app, columns[0]);
    render_file_list(frame, app, columns[1]);
    render_preview_pane(frame, app, columns[2]);
}

fn render_parent_pane(frame: &mut Frame, app: &App, area: Rect) {
    let title = app
        .current_dir
        .parent()
        .and_then(|p| p.file_name())
        .map_or("Parent", |n| {
            // Leak is fine here — these are short-lived frame renders
            // and ratatui borrows from the frame anyway
            Box::leak(n.to_string_lossy().into_owned().into_boxed_str())
        });

    let items: Vec<ListItem> = if let Some(parent) = app.current_dir.parent() {
        std::fs::read_dir(parent)
            .ok()
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| e.path().is_dir())
                    .map(|e| {
                        let name = e.file_name().to_string_lossy().into_owned();
                        let is_current = e.path() == app.current_dir;
                        let style = if is_current {
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::Blue)
                        };
                        ListItem::new(format!("{name}/")).style(style)
                    })
                    .collect()
            })
            .unwrap_or_default()
    } else {
        vec![]
    };

    let list = List::new(items).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(list, area);
}

fn render_file_list(frame: &mut Frame, app: &App, area: Rect) {
    let title = format!(
        " {} — {} files ",
        app.current_dir
            .file_name()
            .map_or(".", |n| Box::leak(n.to_string_lossy().into_owned().into_boxed_str())),
        app.visible_count()
    );

    // Calculate visible window
    let inner_height = area.height.saturating_sub(2) as usize;
    let scroll = if app.selected >= inner_height {
        app.selected - inner_height + 1
    } else {
        0
    };

    let items: Vec<ListItem> = app
        .filtered_indices
        .iter()
        .enumerate()
        .skip(scroll)
        .take(inner_height)
        .map(|(vis_idx, &real_idx)| {
            let entry = &app.entries[real_idx];
            let is_selected = vis_idx == app.selected;
            let is_marked = app.marked.contains(&real_idx);

            let marker = if is_marked { "* " } else { "  " };
            let kind_icon = match entry.media.kind {
                MediaKind::Video | MediaKind::Av => "V",
                MediaKind::Audio => "A",
            };

            // Build compact info line
            let resolution = entry
                .media
                .video
                .as_ref()
                .map_or_else(String::new, super::super::types::VideoInfo::resolution_label);
            let duration = entry
                .media
                .duration_ms
                .map_or_else(String::new, format_duration);
            let size = format_size(entry.fs.size_bytes);

            let line = format!(
                "{marker}{kind_icon} {:<30} {:>6} {:>7} {:>8}",
                truncate(&entry.file_name, 30),
                resolution,
                duration,
                size,
            );

            let style = if is_selected {
                Style::default()
                    .bg(Color::DarkGray)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else if is_marked {
                Style::default().fg(Color::Green)
            } else {
                Style::default()
            };

            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    frame.render_widget(list, area);
}

#[expect(clippy::too_many_lines)]
fn render_preview_pane(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Preview ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let Some(entry) = app.selected_entry() else {
        let empty = Paragraph::new("No file selected").block(block);
        frame.render_widget(empty, area);
        return;
    };

    // Build preview text with metadata
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(vec![
        Span::styled("File: ", Style::default().fg(Color::DarkGray)),
        Span::styled(&entry.file_name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
    ]));

    lines.push(Line::from(vec![
        Span::styled("Kind: ", Style::default().fg(Color::DarkGray)),
        Span::styled(entry.media.kind.to_string(), Style::default().fg(Color::Yellow)),
    ]));

    if let Some(ref video) = entry.media.video {
        lines.push(Line::from(""));
        lines.push(Line::styled(
            "── Video ──",
            Style::default().fg(Color::Cyan),
        ));
        lines.push(Line::from(vec![
            Span::styled("Resolution: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{}×{}", video.width, video.height)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Codec: ", Style::default().fg(Color::DarkGray)),
            Span::raw(&video.codec.name),
            Span::raw(
                video
                    .codec
                    .profile
                    .as_ref()
                    .map_or(String::new(), |p| format!(" ({p})")),
            ),
        ]));
        if let Some(fps) = &video.fps {
            lines.push(Line::from(vec![
                Span::styled("FPS: ", Style::default().fg(Color::DarkGray)),
                Span::raw(fps.to_string()),
            ]));
        }
        if let Some(bitrate) = video.bitrate_bps {
            lines.push(Line::from(vec![
                Span::styled("Bitrate: ", Style::default().fg(Color::DarkGray)),
                Span::raw(format_bitrate(bitrate)),
            ]));
        }
    }

    if let Some(ref audio) = entry.media.audio {
        lines.push(Line::from(""));
        lines.push(Line::styled(
            "── Audio ──",
            Style::default().fg(Color::Magenta),
        ));
        lines.push(Line::from(vec![
            Span::styled("Codec: ", Style::default().fg(Color::DarkGray)),
            Span::raw(&audio.codec.name),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Channels: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{}", audio.channels)),
            Span::raw(
                audio
                    .channel_layout
                    .as_ref()
                    .map_or(String::new(), |l| format!(" ({l})")),
            ),
        ]));
        if let Some(sr) = audio.sample_rate_hz {
            lines.push(Line::from(vec![
                Span::styled("Sample Rate: ", Style::default().fg(Color::DarkGray)),
                Span::raw(format!("{sr} Hz")),
            ]));
        }
        if let Some(bitrate) = audio.bitrate_bps {
            lines.push(Line::from(vec![
                Span::styled("Bitrate: ", Style::default().fg(Color::DarkGray)),
                Span::raw(format_bitrate(bitrate)),
            ]));
        }
    }

    if let Some(dur) = entry.media.duration_ms {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Duration: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format_duration(dur),
                Style::default().fg(Color::Green),
            ),
        ]));
    }

    lines.push(Line::from(vec![
        Span::styled("Size: ", Style::default().fg(Color::DarkGray)),
        Span::raw(format_size(entry.fs.size_bytes)),
    ]));

    if let Some(br) = entry.media.overall_bitrate_bps {
        lines.push(Line::from(vec![
            Span::styled("Overall: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format_bitrate(br)),
        ]));
    }

    // Tags
    let tags = &entry.media.tags;
    let has_tags = tags.title.is_some()
        || tags.artist.is_some()
        || tags.album.is_some();
    if has_tags {
        lines.push(Line::from(""));
        lines.push(Line::styled(
            "── Tags ──",
            Style::default().fg(Color::Yellow),
        ));
        if let Some(ref t) = tags.title {
            lines.push(Line::from(vec![
                Span::styled("Title: ", Style::default().fg(Color::DarkGray)),
                Span::raw(t),
            ]));
        }
        if let Some(ref a) = tags.artist {
            lines.push(Line::from(vec![
                Span::styled("Artist: ", Style::default().fg(Color::DarkGray)),
                Span::raw(a),
            ]));
        }
        if let Some(ref a) = tags.album {
            lines.push(Line::from(vec![
                Span::styled("Album: ", Style::default().fg(Color::DarkGray)),
                Span::raw(a),
            ]));
        }
    }

    let preview = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
    frame.render_widget(preview, area);
}

fn render_metadata_bar(frame: &mut Frame, app: &App, area: Rect) {
    let Some(entry) = app.selected_entry() else {
        frame.render_widget(Paragraph::new(""), area);
        return;
    };

    let mut spans = Vec::new();

    // Video codec + resolution
    if let Some(ref v) = entry.media.video {
        spans.push(Span::styled(
            v.codec.name.to_uppercase(),
            Style::default().fg(Color::Cyan),
        ));
        if let Some(ref p) = v.codec.profile {
            spans.push(Span::raw(format!(" {p}")));
        }
        spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
        spans.push(Span::raw(format!("{}×{}", v.width, v.height)));
        spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
        if let Some(fps) = &v.fps {
            spans.push(Span::raw(format!("{fps}fps")));
            spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
        }
    }

    // Audio codec
    if let Some(ref a) = entry.media.audio {
        spans.push(Span::styled(
            a.codec.name.to_uppercase(),
            Style::default().fg(Color::Magenta),
        ));
        if let Some(ref layout) = a.channel_layout {
            spans.push(Span::raw(format!(" {layout}")));
        }
        spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
        if let Some(sr) = a.sample_rate_hz {
            spans.push(Span::raw(format!("{}kHz", sr / 1000)));
            spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
        }
    }

    // Overall bitrate
    if let Some(br) = entry.media.overall_bitrate_bps {
        spans.push(Span::raw(format_bitrate(br)));
    }

    // Playback indicator
    if let Some(current) = app.mpv.current_file()
        && current == entry.path {
            spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
            let state_icon = match app.mpv.state() {
                crate::playback::PlaybackState::Playing => "▶ Playing",
                crate::playback::PlaybackState::Paused => "⏸ Paused",
                crate::playback::PlaybackState::Stopped => "",
            };
            spans.push(Span::styled(
                state_icon,
                Style::default().fg(Color::Green),
            ));
        }

    let bar = Paragraph::new(Line::from(spans))
        .style(Style::default().bg(Color::Rgb(30, 30, 30)));
    frame.render_widget(bar, area);
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let footer_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(area);

    // Status line
    let status = if let Some(ref msg) = app.status_message {
        Line::styled(msg.as_str(), Style::default().fg(Color::Yellow))
    } else if !app.errors.is_empty() {
        Line::styled(
            format!("{} probe errors", app.errors.len()),
            Style::default().fg(Color::Red),
        )
    } else {
        Line::styled(
            format!(
                "{}/{} files │ Sort: {}",
                app.selected + 1,
                app.visible_count(),
                app.sort_key.label()
            ),
            Style::default().fg(Color::DarkGray),
        )
    };
    frame.render_widget(Paragraph::new(status), footer_layout[0]);

    // Keybinding bar
    let keys = if app.triage.is_some() {
        "[y] keep  [n] delete  [m] move  [u] undo  [q] quit triage"
    } else {
        "[j/k] nav  [Enter] open  [Space] mark  [p] play  [s] sort  [/] filter  [t] triage  [?] help"
    };
    let keybindings = Paragraph::new(Line::styled(
        keys,
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(keybindings, footer_layout[1]);
}

fn render_help_overlay(frame: &mut Frame, area: Rect) {
    let help_area = centered_rect(60, 70, area);
    frame.render_widget(Clear, help_area);

    let help_text = vec![
        Line::styled("mls — Media LS Help", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Line::from(""),
        Line::styled("Navigation", Style::default().add_modifier(Modifier::BOLD)),
        Line::from("  j/k, ↑/↓    Move up/down"),
        Line::from("  g/G          First/last"),
        Line::from("  Ctrl-d/u     Page down/up"),
        Line::from("  Enter        Open with default app"),
        Line::from(""),
        Line::styled("Actions", Style::default().add_modifier(Modifier::BOLD)),
        Line::from("  /            Fuzzy filter"),
        Line::from("  s/S          Cycle sort / reverse"),
        Line::from("  i            Toggle metadata panel"),
        Line::from("  Space        Mark/unmark file"),
        Line::from(""),
        Line::styled("Playback", Style::default().add_modifier(Modifier::BOLD)),
        Line::from("  p            Play/pause"),
        Line::from("  [/]          Seek -10s/+10s"),
        Line::from(""),
        Line::styled("Triage", Style::default().add_modifier(Modifier::BOLD)),
        Line::from("  t            Enter triage mode"),
        Line::from("  y/n/m        Keep/delete/move (in triage)"),
        Line::from("  u            Undo (in triage)"),
        Line::from(""),
        Line::from("  q/Ctrl-c     Quit"),
        Line::from("  ?            Toggle this help"),
    ];

    let help = Paragraph::new(help_text)
        .block(
            Block::default()
                .title(" Help ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(help, help_area);
}

fn render_filter_input(frame: &mut Frame, area: Rect) {
    let input_area = Rect {
        x: area.x + 1,
        y: area.height.saturating_sub(3),
        width: area.width.saturating_sub(2),
        height: 1,
    };

    let input = Paragraph::new(Line::from(vec![
        Span::styled("/ ", Style::default().fg(Color::Yellow)),
        Span::raw("(filter input shown in status)"),
    ]));
    frame.render_widget(input, input_area);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len - 1])
    }
}
