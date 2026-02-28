/// TUI layout — Miller column layout with metadata panel.
///
/// Three panes: parent dir | current file list | preview (thumbnail + metadata).
/// Footer shows contextual keybindings.
use super::App;
use crate::types::{MediaKind, format_bitrate, format_duration, format_size};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};

/// Main render function — lays out all panes.
pub fn render(frame: &mut Frame, app: &mut App) {
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

    // Main area: miller columns + optional bars
    let has_playback = app.mpv.state() != crate::playback::PlaybackState::Stopped;
    let bar_height = u16::from(app.show_metadata) + u16::from(has_playback);

    if bar_height > 0 {
        let main_split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(5), Constraint::Length(bar_height)])
            .split(main_area);

        render_miller_columns(frame, app, main_split[0]);

        if bar_height == 2 {
            // Both metadata and playback bars
            let bar_split = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Length(1)])
                .split(main_split[1]);
            if app.show_metadata {
                render_metadata_bar(frame, app, bar_split[0]);
                render_playback_bar(frame, app, bar_split[1]);
            } else {
                // Only playback (shouldn't happen since bar_height logic)
                render_playback_bar(frame, app, main_split[1]);
            }
        } else if app.show_metadata {
            render_metadata_bar(frame, app, main_split[1]);
        } else {
            render_playback_bar(frame, app, main_split[1]);
        }
    } else {
        render_miller_columns(frame, app, main_area);
    }

    render_footer(frame, app, footer_area);

    // Overlays
    if app.show_help {
        render_help_overlay(frame, size);
    }

    // Filter input is rendered inside render_miller_columns
    // (overlaid on the middle pane bottom row)

    if let Some(ref text) = app.move_input {
        render_move_input(frame, size, text);
    }
}

fn render_miller_columns(frame: &mut Frame, app: &mut App, area: Rect) {
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

    if app.filter_active {
        render_filter_input(frame, app, columns[1]);
    }
}

fn render_parent_pane(frame: &mut Frame, app: &App, area: Rect) {
    let title = app
        .current_dir
        .parent()
        .and_then(|p| p.file_name())
        .map_or_else(
            || "Parent".to_string(),
            |n| n.to_string_lossy().into_owned(),
        );

    let items: Vec<ListItem> = app
        .sibling_dirs
        .iter()
        .map(|dir| {
            let name = dir
                .file_name()
                .map_or_else(|| ".".to_string(), |n| n.to_string_lossy().into_owned());
            let is_current = *dir == app.current_dir;
            let style = if is_current {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Blue)
            };
            ListItem::new(format!("{name}/")).style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(list, area);
}

fn render_file_list(frame: &mut Frame, app: &App, area: Rect) {
    let dir_name = app
        .current_dir
        .file_name()
        .map_or_else(|| ".".to_string(), |n| n.to_string_lossy().into_owned());

    let scanning_indicator = if app.dir_scanning { " Scanning..." } else { "" };
    let title = format!(
        " {dir_name} — {} items{scanning_indicator} ",
        app.visible_count()
    );

    let total_items = app.visible_count();
    let dir_count = app.dir_items.len();

    // Calculate visible window
    let inner_height = area.height.saturating_sub(2) as usize;
    let scroll = if app.selected >= inner_height {
        app.selected - inner_height + 1
    } else {
        0
    };

    let items: Vec<ListItem> = (0..total_items)
        .skip(scroll)
        .take(inner_height)
        .map(|vis_idx| {
            let is_selected = vis_idx == app.selected;

            if vis_idx < dir_count {
                // Directory item
                let dir = &app.dir_items[vis_idx];
                let name = dir
                    .file_name()
                    .map_or_else(|| ".".to_string(), |n| n.to_string_lossy().into_owned());
                let line = format!("  D {name}/");
                let style = if is_selected {
                    Style::default()
                        .bg(Color::DarkGray)
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Blue)
                };
                ListItem::new(line).style(style)
            } else {
                // Media entry item
                let media_idx = vis_idx - dir_count;
                let real_idx = app.filtered_indices[media_idx];
                let entry = &app.entries[real_idx];
                let is_marked = app.marked.contains(&real_idx);

                let marker = if is_marked { "* " } else { "  " };
                let kind_icon = match entry.media.kind {
                    MediaKind::Video | MediaKind::Av => "V",
                    MediaKind::Audio => "A",
                };

                let resolution = entry.media.video.as_ref().map_or_else(
                    String::new,
                    super::super::types::VideoInfo::resolution_label,
                );
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
            }
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

fn render_preview_pane(frame: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::default()
        .title(" Preview ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    // Directory selected — show directory name
    if let Some(dir) = app.selected_dir() {
        let dir_name = dir
            .file_name()
            .map_or_else(|| ".".to_string(), |n| n.to_string_lossy().into_owned());
        let lines = vec![
            Line::styled(
                dir_name,
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::from(""),
            Line::styled("Press Enter to open", Style::default().fg(Color::DarkGray)),
        ];
        let preview = Paragraph::new(lines).block(block);
        frame.render_widget(preview, area);
        return;
    }

    let Some(entry) = app.selected_entry().cloned() else {
        let msg = if app.dir_scanning {
            "Scanning..."
        } else {
            "No file selected"
        };
        let empty = Paragraph::new(msg).block(block);
        frame.render_widget(empty, area);
        return;
    };

    // Split preview pane: thumbnail on top, metadata below
    let has_video = entry.media.video.is_some();
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if has_video {
        let split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(40), // thumbnail
                Constraint::Percentage(60), // metadata text
            ])
            .split(inner);

        super::preview::render_thumbnail(frame, app, split[0]);
        render_metadata_text(frame, &entry, split[1]);
    } else {
        render_metadata_text(frame, &entry, inner);
    }
}

#[expect(clippy::too_many_lines)]
fn render_metadata_text(frame: &mut Frame, entry: &crate::types::MediaEntry, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(vec![
        Span::styled("File: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            &entry.file_name,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    lines.push(Line::from(vec![
        Span::styled("Kind: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            entry.media.kind.to_string(),
            Style::default().fg(Color::Yellow),
        ),
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
            Span::styled(format_duration(dur), Style::default().fg(Color::Green)),
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
    let has_tags = tags.title.is_some() || tags.artist.is_some() || tags.album.is_some();
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

    let preview = Paragraph::new(lines).wrap(Wrap { trim: true });
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
        && current == entry.path
    {
        spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
        let state_icon = match app.mpv.state() {
            crate::playback::PlaybackState::Playing => "▶ Playing",
            crate::playback::PlaybackState::Paused => "⏸ Paused",
            crate::playback::PlaybackState::Stopped => "",
        };
        spans.push(Span::styled(state_icon, Style::default().fg(Color::Green)));
    }

    let bar = Paragraph::new(Line::from(spans)).style(Style::default().bg(Color::Rgb(30, 30, 30)));
    frame.render_widget(bar, area);
}

fn render_playback_bar(frame: &mut Frame, app: &App, area: Rect) {
    let state_icon = match app.mpv.state() {
        crate::playback::PlaybackState::Playing => "▶",
        crate::playback::PlaybackState::Paused => "⏸",
        crate::playback::PlaybackState::Stopped => return,
    };

    let file_name = app.playback_file_name.as_deref().unwrap_or("Unknown");

    let pos = app.playback_position.unwrap_or(0.0);
    let dur = app.playback_duration.unwrap_or(0.0);

    let pos_str = format_seconds(pos);
    let dur_str = format_seconds(dur);

    // Calculate progress bar width: area - icon(2) - name - time - padding
    let time_part = format!("  {pos_str} / {dur_str}");
    let prefix = format!("{state_icon} {file_name}  ");
    let prefix_width = prefix.chars().count();
    let time_width = time_part.len();
    let bar_width = (area.width as usize).saturating_sub(prefix_width + time_width + 1);

    let progress = format_progress_bar(pos, dur, bar_width);

    let spans = vec![
        Span::styled(format!("{state_icon} "), Style::default().fg(Color::Green)),
        Span::styled(truncate(file_name, 30), Style::default().fg(Color::White)),
        Span::raw("  "),
        Span::styled(progress, Style::default().fg(Color::Cyan)),
        Span::styled(time_part, Style::default().fg(Color::DarkGray)),
    ];

    let bar = Paragraph::new(Line::from(spans)).style(Style::default().bg(Color::Rgb(25, 25, 40)));
    frame.render_widget(bar, area);
}

/// Format seconds as MM:SS or H:MM:SS.
fn format_seconds(secs: f64) -> String {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "playback seconds fit in u64"
    )]
    let total = secs as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Render an ASCII progress bar: `[=====>--------]`.
#[must_use]
pub fn format_progress_bar(position: f64, duration: f64, width: usize) -> String {
    if width < 3 || duration <= 0.0 {
        return format!("[{}]", "-".repeat(width.saturating_sub(2)));
    }

    let inner = width - 2; // exclude [ and ]
    let ratio = (position / duration).clamp(0.0, 1.0);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "progress bar character count fits in usize"
    )]
    let filled = (ratio * inner as f64) as usize;
    let remaining = inner.saturating_sub(filled + 1);

    let mut bar = String::with_capacity(width);
    bar.push('[');
    for _ in 0..filled {
        bar.push('=');
    }
    if filled < inner {
        bar.push('>');
        for _ in 0..remaining {
            bar.push('-');
        }
    }
    bar.push(']');
    bar
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
        let kind_label = app.kind_filter.label();
        Line::styled(
            format!(
                "{}/{} files │ Sort: {} │ [{}]",
                app.selected + 1,
                app.visible_count(),
                app.sort_key.label(),
                kind_label,
            ),
            Style::default().fg(Color::DarkGray),
        )
    };
    frame.render_widget(Paragraph::new(status), footer_layout[0]);

    // Keybinding bar
    let keys = if app.triage.is_some() {
        "[y] keep  [n] delete  [m] move  [u] undo  [q] quit triage"
    } else {
        "[j/k] nav  [Enter] open  [p] play  [/] filter  [1/2/3] kind  [s] sort  [t] triage  [?] help"
    };
    let keybindings = Paragraph::new(Line::styled(keys, Style::default().fg(Color::DarkGray)));
    frame.render_widget(keybindings, footer_layout[1]);
}

fn render_help_overlay(frame: &mut Frame, area: Rect) {
    let help_area = centered_rect(60, 70, area);
    frame.render_widget(Clear, help_area);

    let help_text = vec![
        Line::styled(
            "mls — Media LS Help",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
        Line::styled("Navigation", Style::default().add_modifier(Modifier::BOLD)),
        Line::from("  j/k, ↑/↓    Move up/down"),
        Line::from("  h/l, ←/→     Parent/enter directory"),
        Line::from("  g/G          First/last"),
        Line::from("  Ctrl-d/u     Page down/up"),
        Line::from("  Enter        Open file / enter directory"),
        Line::from(""),
        Line::styled("Actions", Style::default().add_modifier(Modifier::BOLD)),
        Line::from("  /            Fuzzy filter (prefix = for structured)"),
        Line::from("  1/2/3        Filter: All/Video/Audio"),
        Line::from("  s/S          Cycle sort / reverse"),
        Line::from("  i            Toggle metadata panel"),
        Line::from("  Space        Mark/unmark file"),
        Line::from(""),
        Line::styled("Playback", Style::default().add_modifier(Modifier::BOLD)),
        Line::from("  p            Play / pause / switch file"),
        Line::from("  P            Stop playback"),
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

fn render_filter_input(frame: &mut Frame, app: &App, pane_area: Rect) {
    // Render at the bottom of the middle pane (inside border)
    let input_area = Rect {
        x: pane_area.x + 1,
        y: pane_area.y + pane_area.height.saturating_sub(2),
        width: pane_area.width.saturating_sub(2),
        height: 1,
    };

    frame.render_widget(Clear, input_area);

    let mode_indicator = match app.filter_mode {
        super::FilterMode::Fuzzy => Span::styled("[~] ", Style::default().fg(Color::Cyan)),
        super::FilterMode::Structured => {
            // Show red if current text doesn't parse
            let color = if app.filter_text.is_empty()
                || crate::filter::Filter::parse(&app.filter_text).is_ok()
            {
                Color::Green
            } else {
                Color::Red
            };
            Span::styled("[=] ", Style::default().fg(color))
        }
    };

    let match_count = format!("({})", app.filtered_indices.len());

    // Calculate how much space the match count + padding takes
    #[expect(
        clippy::cast_possible_truncation,
        reason = "match count display width fits u16"
    )]
    let count_width = match_count.len() as u16 + 1;
    let text_width = input_area.width.saturating_sub(count_width + 5); // 5 = mode(4) + slash(1)

    let display_text = if app.filter_text.chars().count() > text_width as usize {
        // Truncate from the left to show the cursor end
        let skip = app.filter_text.chars().count() - text_width as usize;
        app.filter_text.chars().skip(skip).collect::<String>()
    } else {
        app.filter_text.clone()
    };

    let mut spans = vec![
        Span::styled("/ ", Style::default().fg(Color::Yellow)),
        mode_indicator,
        Span::raw(display_text),
        Span::styled("_", Style::default().fg(Color::White)),
    ];

    // Right-align match count
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let padding = input_area.width as usize - used.min(input_area.width as usize);
    if padding > match_count.len() + 1 {
        let pad = " ".repeat(padding - match_count.len());
        spans.push(Span::raw(pad));
        spans.push(Span::styled(
            match_count,
            Style::default().fg(Color::DarkGray),
        ));
    }

    let input =
        Paragraph::new(Line::from(spans)).style(Style::default().bg(Color::Rgb(40, 40, 40)));
    frame.render_widget(input, input_area);
}

fn render_move_input(frame: &mut Frame, area: Rect, text: &str) {
    let input_area = Rect {
        x: area.x + 1,
        y: area.height.saturating_sub(4),
        width: area.width.saturating_sub(2),
        height: 1,
    };

    frame.render_widget(Clear, input_area);
    let input = Paragraph::new(Line::from(vec![
        Span::styled("Move to: ", Style::default().fg(Color::Yellow)),
        Span::raw(text),
        Span::styled("_", Style::default().fg(Color::White)),
    ]))
    .style(Style::default().bg(Color::Rgb(40, 40, 40)));
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
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let end = s
            .char_indices()
            .nth(max_len - 1)
            .map_or(s.len(), |(i, _)| i);
        format!("{}…", &s[..end])
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_ascii_unchanged() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_exact_length_unchanged() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn truncate_long_ascii() {
        let result = truncate("hello world", 5);
        assert_eq!(result, "hell…");
    }

    #[test]
    fn truncate_japanese_filename() {
        // 5 Japanese chars, truncate to 3 → 2 chars + ellipsis
        let result = truncate("映画作品集", 3);
        assert_eq!(result, "映画…");
    }

    #[test]
    fn truncate_emoji_filename() {
        let result = truncate("🎬🎥🎞️📽️🎦", 3);
        // 2 emojis + ellipsis
        let chars: Vec<char> = result.chars().collect();
        assert_eq!(chars[0], '🎬');
        assert_eq!(chars[1], '🎥');
        assert_eq!(*chars.last().unwrap(), '…');
    }

    #[test]
    fn truncate_mixed_ascii_and_multibyte() {
        // "abc映画" is 5 chars, truncate to 4 → "abc…"
        let result = truncate("abc映画", 4);
        assert_eq!(result, "abc…");
    }

    #[test]
    fn truncate_single_char_limit() {
        // max_len=1 means 0 content chars + ellipsis
        let result = truncate("abcdef", 1);
        assert_eq!(result, "…");
    }

    #[test]
    fn truncate_empty_string() {
        assert_eq!(truncate("", 5), "");
    }

    #[test]
    fn truncate_japanese_short_unchanged() {
        assert_eq!(truncate("映画", 5), "映画");
    }

    // --- Progress bar ---

    #[test]
    fn progress_bar_zero_position() {
        let bar = format_progress_bar(0.0, 100.0, 12);
        assert_eq!(bar, "[>---------]");
    }

    #[test]
    fn progress_bar_half() {
        let bar = format_progress_bar(50.0, 100.0, 12);
        assert_eq!(bar, "[=====>----]");
    }

    #[test]
    fn progress_bar_full() {
        let bar = format_progress_bar(100.0, 100.0, 12);
        assert_eq!(bar, "[==========]");
    }

    #[test]
    fn progress_bar_zero_duration() {
        let bar = format_progress_bar(0.0, 0.0, 12);
        assert_eq!(bar, "[----------]");
    }

    #[test]
    fn progress_bar_narrow() {
        let bar = format_progress_bar(50.0, 100.0, 4);
        assert_eq!(bar, "[=>]");
    }

    #[test]
    fn progress_bar_too_narrow() {
        let bar = format_progress_bar(50.0, 100.0, 2);
        assert_eq!(bar, "[]");
    }

    // --- format_seconds ---

    #[test]
    fn format_seconds_zero() {
        assert_eq!(format_seconds(0.0), "0:00");
    }

    #[test]
    fn format_seconds_minutes() {
        assert_eq!(format_seconds(83.0), "1:23");
    }

    #[test]
    fn format_seconds_hours() {
        assert_eq!(format_seconds(3723.0), "1:02:03");
    }
}
