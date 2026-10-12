//! Rendering. Every string that came from the network goes through
//! `crate::terminal_text` before it reaches the terminal.
use super::{
    App, FOLDERS, PAGE, Tone,
    theme::{self, Theme},
};
use alice_core::{Message, i18n};
use chrono::{DateTime, Datelike, Local};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Padding, Paragraph, Wrap},
};
use std::time::Duration;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Below this width the reader stacks under the list instead of beside it.
const NARROW: u16 = 90;
/// A background refresh only shows a spinner once it is noticeably slow, so
/// the 2s refresh cadence never flickers.
const LOADING_GRACE: Duration = Duration::from_millis(300);
/// Label column of the compose form.
const LABEL: u16 = 14;
const ROW_HEIGHT: u16 = 3;
const ROW_STRIDE: u16 = ROW_HEIGHT + 1;

#[derive(Clone, Copy)]
pub(super) struct Activity {
    /// How long the current inbox load has been in flight.
    pub loading: Option<Duration>,
    /// A mutation (send, save, sync, update) is in flight.
    pub busy: bool,
    /// Time since the TUI started; drives the spinner.
    pub clock: Duration,
}

impl Activity {
    fn spinner(&self) -> &'static str {
        theme::spinner(self.clock)
    }
    fn loading_visible(&self, loaded: bool) -> bool {
        self.loading
            .is_some_and(|elapsed| !loaded || elapsed >= LOADING_GRACE)
    }
}

pub(super) fn draw(frame: &mut Frame<'_>, app: &mut App, theme: &Theme, activity: Activity) {
    let area = frame.area().inner(Margin::new(1, 0));
    let browsing = app.editing.is_none();
    let [top, tabs, content, status, hints] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(u16::from(browsing)),
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);
    top_bar(frame, top, app, theme, activity);
    if browsing {
        folder_tabs(frame, tabs, app, theme);
        browse(frame, content, app, theme, activity);
    } else {
        compose(frame, content, app, theme);
    }
    status_line(frame, status, app, theme, activity);
    hint_line(frame, hints, app, theme);
    if app.help {
        help(frame, frame.area(), app, theme);
    }
}

fn top_bar(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme, activity: Activity) {
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let mut left = vec![
        Span::styled(
            "✳ alice",
            theme.fg(theme.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  /  ", theme.fg(theme.faint)),
    ];
    if app.editing.is_some() {
        left.push(Span::styled(app.editor_title.as_str(), bold));
    } else {
        left.push(Span::styled(folder_title(FOLDERS[app.folder_index]), bold));
        left.push(Span::raw("   "));
        // One span with one style: the range must reach the terminal as a
        // single contiguous run of text.
        left.push(Span::styled(range_text(app), theme.fg(theme.muted)));
        if app.unread > 0 {
            left.push(Span::styled("  ·  ", theme.fg(theme.faint)));
            left.push(Span::styled(
                alice_core::t!(crate::locale(), "tui.header.unread", count = app.unread),
                theme.fg(theme.accent),
            ));
        }
        if !app.query.is_empty() {
            left.push(Span::styled("  ·  ", theme.fg(theme.faint)));
            left.push(Span::styled(
                alice_core::t!(
                    crate::locale(),
                    "tui.header.matching",
                    query = truncate(&one_line(&crate::terminal_text(&app.query)), 24)
                ),
                theme.fg(theme.muted),
            ));
        }
        if activity.loading_visible(app.loaded) {
            left.push(Span::styled(
                format!("  {}", activity.spinner()),
                theme.fg(theme.accent),
            ));
        }
    }
    let left = Line::from(left);
    // Style the span, not the line: a line style would recolor the whole row.
    let right = Line::from(Span::styled(account_context(app), theme.fg(theme.muted)));
    let fits = left.width() + right.width() + 2 <= area.width as usize;
    frame.render_widget(left, area);
    if fits {
        frame.render_widget(right.right_aligned(), area);
    }
}

fn range_text(app: &App) -> String {
    if !app.loaded {
        alice_core::t!(crate::locale(), "tui.header.loading")
    } else if app.total == 0 {
        alice_core::t!(crate::locale(), "tui.header.no_messages")
    } else {
        alice_core::t!(
            crate::locale(),
            "tui.header.range",
            start = app.offset + 1,
            end = app.offset + app.messages.len(),
            total = app.total
        )
    }
}

fn account_context(app: &App) -> String {
    match app.accounts.as_slice() {
        [] if app.loaded => alice_core::t!(crate::locale(), "tui.header.no_accounts"),
        [] => String::new(),
        [account] => one_line(&crate::terminal_text(&account.email)),
        accounts => alice_core::t!(
            crate::locale(),
            "tui.header.accounts",
            count = accounts.len()
        ),
    }
}

fn folder_title(folder: &str) -> String {
    let key = match folder {
        "inbox" => "common.folder.inbox",
        "starred" => "common.folder.starred",
        "drafts" => "common.folder.drafts",
        "sent" => "common.folder.sent",
        "archive" => "common.folder.archive",
        "spam" => "common.folder.spam",
        _ => "common.folder.trash",
    };
    i18n::tr(crate::locale(), key, &[])
}

fn folder_tabs(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let labels: Vec<_> = FOLDERS
        .iter()
        .map(|folder| format!(" {} ", folder_title(folder)))
        .collect();
    let mut start = 0;
    let mut selected_width: usize = labels[..=app.folder_index]
        .iter()
        .map(|label| label.width() + 1)
        .sum();
    while selected_width > area.width as usize && start < app.folder_index {
        selected_width = selected_width.saturating_sub(labels[start].width() + 1);
        start += 1;
    }
    let mut spans = Vec::new();
    let mut room = area.width as usize;
    for (index, label) in labels.into_iter().enumerate().skip(start) {
        if room == 0 || (index > app.folder_index && label.width() > room) {
            break;
        }
        let label = if label.width() > room {
            truncate(&label, room)
        } else {
            label
        };
        room = room.saturating_sub(label.width() + 1);
        spans.push(if index == app.folder_index {
            Span::styled(label, theme.active_tab())
        } else {
            Span::styled(label, theme.fg(theme.muted))
        });
        spans.push(Span::raw(" "));
    }
    frame.render_widget(Line::from(spans), area);
}

fn pane(theme: &Theme) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.fg(theme.faint))
}

fn browse(frame: &mut Frame<'_>, area: Rect, app: &mut App, theme: &Theme, activity: Activity) {
    if area.width >= NARROW {
        let list_width = (area.width * 2 / 5).clamp(40, 60);
        let [list, reader_area] =
            Layout::horizontal([Constraint::Length(list_width), Constraint::Fill(1)]).areas(area);
        message_list(frame, list, app, theme, activity);
        reader(frame, reader_area, app, theme);
    } else if area.height >= 18 {
        let [list, reader_area] =
            Layout::vertical([Constraint::Percentage(45), Constraint::Fill(1)]).areas(area);
        message_list(frame, list, app, theme, activity);
        reader(frame, reader_area, app, theme);
    } else {
        message_list(frame, area, app, theme, activity);
    }
}

fn message_list(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &mut App,
    theme: &Theme,
    activity: Activity,
) {
    let count = app.messages.len();
    let mut block = pane(theme);
    if let Some(selected) = app.selected.filter(|i| *i < count) {
        block = block.title_bottom(
            Line::styled(
                format!(
                    " {} ",
                    alice_core::t!(
                        crate::locale(),
                        "tui.header.position",
                        position = app.offset + selected + 1,
                        total = app.total.max(count)
                    )
                ),
                theme.fg(theme.muted),
            )
            .right_aligned(),
        );
    }
    let mut rows = block.inner(area);
    frame.render_widget(block, area);
    if app.searching || !app.query.is_empty() {
        let [bar, _, rest] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .areas(rows);
        search_bar(frame, bar, app, theme);
        rows = rest;
    }
    if count == 0 {
        list_empty(frame, rows, app, theme, activity);
        return;
    }
    let visible = (rows.height.saturating_add(1) / ROW_STRIDE).max(1) as usize;
    let selected = app.selected.unwrap_or(0).min(count - 1);
    app.list_top = app.list_top.min(count.saturating_sub(visible));
    if selected < app.list_top {
        app.list_top = selected;
    } else if selected >= app.list_top + visible {
        app.list_top = selected + 1 - visible;
    }
    for (row, index) in (app.list_top..count).take(visible).enumerate() {
        let y = rows.y + row as u16 * ROW_STRIDE;
        let height = ROW_HEIGHT.min(rows.bottom().saturating_sub(y));
        if height == 0 {
            break;
        }
        message_row(
            frame,
            Rect::new(rows.x, y, rows.width, height),
            &app.messages[index],
            app.selected == Some(index),
            theme,
        );
    }
}

fn message_row(
    frame: &mut Frame<'_>,
    area: Rect,
    message: &Message,
    selected: bool,
    theme: &Theme,
) {
    if selected {
        let buffer = frame.buffer_mut();
        buffer.set_style(area, theme.selected());
        for y in area.top()..area.bottom() {
            if let Some(cell) = buffer.cell_mut((area.x, y)) {
                cell.set_symbol("▎").set_style(theme.fg(theme.accent));
            }
        }
    }
    let line = |row: u16| Rect::new(area.x + 2, area.y + row, area.width.saturating_sub(3), 1);
    let width = area.width.saturating_sub(3) as usize;
    if width < 6 {
        return;
    }
    let unread = !message.is_read;
    let date = short_date(&message.date, Local::now(), crate::locale());
    let star = if message.is_starred { "★ " } else { "" };
    let right_width = star.width() + date.width();
    let show_right = width >= right_width + 10;
    let sender = if message.from_name.trim().is_empty() {
        &message.from_email
    } else {
        &message.from_name
    };
    let sender_room = width.saturating_sub(2 + if show_right { right_width + 2 } else { 0 });
    let sender_style = if unread {
        theme.fg(theme.text).add_modifier(Modifier::BOLD)
    } else {
        theme.fg(theme.text)
    };
    frame.render_widget(
        Line::from(vec![
            Span::styled(if unread { "● " } else { "  " }, theme.fg(theme.accent)),
            Span::styled(
                truncate(&one_line(&crate::terminal_text(sender)), sender_room),
                sender_style,
            ),
        ]),
        line(0),
    );
    if show_right {
        frame.render_widget(
            Line::from(vec![
                Span::styled(star, theme.fg(theme.star)),
                Span::styled(
                    date,
                    theme.fg(if unread { theme.accent } else { theme.muted }),
                ),
            ])
            .right_aligned(),
            line(0),
        );
    }
    if area.height > 1 {
        let subject = one_line(&crate::terminal_text(&message.subject));
        frame.render_widget(
            if subject.is_empty() {
                Line::styled(
                    format!(
                        "  {}",
                        alice_core::t!(crate::locale(), "tui.message.no_subject")
                    ),
                    theme.fg(theme.muted),
                )
            } else {
                Line::styled(
                    format!("  {}", truncate(&subject, width - 2)),
                    theme.fg(theme.text),
                )
            },
            line(1),
        );
    }
    if area.height > 2 {
        frame.render_widget(
            Line::styled(
                format!(
                    "  {}",
                    truncate(
                        &one_line(&crate::terminal_text(&message.preview)),
                        width - 2
                    )
                ),
                theme.fg(theme.muted),
            ),
            line(2),
        );
    }
}

fn search_bar(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let area = Rect::new(area.x + 2, area.y, area.width.saturating_sub(4), 1);
    let prefix = "/ ";
    let room = (area.width as usize).saturating_sub(prefix.width() + 1);
    let query = tail(&one_line(&crate::terminal_text(&app.query)), room);
    let mut spans = vec![Span::styled(
        prefix,
        theme.fg(theme.accent).add_modifier(Modifier::BOLD),
    )];
    if query.is_empty() {
        // The daemon's query language; trimmed to whatever fits.
        let placeholder = alice_core::t!(crate::locale(), "tui.search.placeholder");
        spans.push(Span::styled(
            truncate(&placeholder, room),
            theme.fg(theme.muted),
        ));
    } else {
        spans.push(Span::styled(query.clone(), theme.fg(theme.text)));
    }
    if !app.searching {
        let hint = alice_core::t!(crate::locale(), "tui.search.hint");
        if prefix.width() + query.width() + hint.width() <= area.width as usize {
            spans.push(Span::styled(hint, theme.fg(theme.faint)));
        }
    }
    frame.render_widget(Line::from(spans), area);
    if app.searching && area.width > 0 {
        frame.set_cursor_position((
            area.x + ((prefix.width() + query.width()) as u16).min(area.width - 1),
            area.y,
        ));
    }
}

fn list_empty(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme, activity: Activity) {
    let folder = FOLDERS[app.folder_index];
    let (title, hint) = if !app.loaded {
        (
            alice_core::t!(
                crate::locale(),
                "tui.empty.loading",
                spinner = activity.spinner(),
                folder = folder_title(folder)
            ),
            String::new(),
        )
    } else if app.accounts.is_empty() {
        (
            alice_core::t!(crate::locale(), "tui.empty.no_accounts"),
            alice_core::t!(crate::locale(), "tui.empty.connect"),
        )
    } else if !app.query.is_empty() {
        (
            alice_core::t!(
                crate::locale(),
                "tui.empty.no_matches",
                query = truncate(&one_line(&crate::terminal_text(&app.query)), 30)
            ),
            alice_core::t!(crate::locale(), "tui.empty.search_hint"),
        )
    } else {
        let (title, hint) = match folder {
            "inbox" => ("tui.empty.inbox", "tui.empty.inbox_hint"),
            "starred" => ("tui.empty.starred", "tui.empty.starred_hint"),
            "drafts" => ("tui.empty.drafts", "tui.empty.drafts_hint"),
            "sent" => ("tui.empty.sent", "tui.empty.sent_hint"),
            "archive" => ("tui.empty.archive", "tui.empty.archive_hint"),
            "spam" => ("tui.empty.spam", "tui.empty.spam_hint"),
            _ => ("tui.empty.trash", "tui.empty.trash_hint"),
        };
        (
            i18n::tr(crate::locale(), title, &[]),
            i18n::tr(crate::locale(), hint, &[]),
        )
    };
    let lines = vec![
        Line::styled(title, theme.fg(theme.text).add_modifier(Modifier::BOLD)),
        Line::default(),
        Line::styled(hint, theme.fg(theme.muted)),
    ];
    let area = area.inner(Margin::new(2, 0));
    let top = area.height.saturating_sub(lines.len() as u16) / 3;
    frame.render_widget(
        Paragraph::new(lines).centered().wrap(Wrap { trim: true }),
        Rect::new(area.x, area.y + top, area.width, area.height - top),
    );
}

fn reader(frame: &mut Frame<'_>, area: Rect, app: &mut App, theme: &Theme) {
    let block = pane(theme).padding(Padding::new(2, 2, 1, 0));
    let inner = block.inner(area);
    let Some(message) = app.selected.and_then(|i| app.messages.get(i)) else {
        frame.render_widget(block, area);
        if app.loaded {
            let text = if app.messages.is_empty() {
                alice_core::t!(crate::locale(), "tui.message.empty")
            } else {
                alice_core::t!(crate::locale(), "tui.message.select")
            };
            let top = inner.height.saturating_sub(1) / 3;
            frame.render_widget(
                Paragraph::new(Line::styled(text, theme.fg(theme.muted)))
                    .centered()
                    .wrap(Wrap { trim: true }),
                Rect::new(inner.x, inner.y + top, inner.width, inner.height - top),
            );
        }
        return;
    };
    if inner.width < 4 || inner.height < 3 {
        frame.render_widget(block, area);
        return;
    }
    let width = inner.width as usize;
    let compact = inner.height < 14;
    let subject = one_line(&crate::terminal_text(&message.subject));
    let subject = Paragraph::new(if subject.is_empty() {
        Line::styled(
            alice_core::t!(crate::locale(), "tui.message.no_subject"),
            theme.fg(theme.muted),
        )
    } else {
        Line::styled(
            if compact {
                truncate(&subject, width)
            } else {
                subject
            },
            theme.fg(theme.text).add_modifier(Modifier::BOLD),
        )
    })
    .wrap(Wrap { trim: true });
    let subject_height = (subject.line_count(inner.width).clamp(1, 3)) as u16;
    let mut meta = Vec::new();
    let name = one_line(&crate::terminal_text(&message.from_name));
    let email = one_line(&crate::terminal_text(&message.from_email));
    if compact {
        let date = short_date(&message.date, Local::now(), crate::locale());
        let sender = if name.is_empty() { &email } else { &name };
        meta.push(Line::from(vec![
            Span::styled(
                truncate(sender, width.saturating_sub(date.width() + 3)),
                theme.fg(theme.text),
            ),
            Span::styled(format!(" · {date}"), theme.fg(theme.muted)),
        ]));
        if let Some(invite) = &message.invite {
            meta.push(Line::styled(
                truncate(
                    &one_line(&format!(
                        "{} · {} · {}",
                        crate::event_title(&invite.event),
                        crate::event_when(&invite.event),
                        crate::invite_state(invite)
                    )),
                    width,
                ),
                theme.fg(theme.accent),
            ));
        }
    } else {
        let room = width.saturating_sub(LABEL as usize);
        let from = if name.is_empty() {
            vec![Span::styled(truncate(&email, room), theme.fg(theme.text))]
        } else if name.width() + email.width() + 3 <= room {
            vec![
                Span::styled(name, theme.fg(theme.text)),
                Span::styled(format!(" <{email}>"), theme.fg(theme.muted)),
            ]
        } else {
            vec![Span::styled(truncate(&name, room), theme.fg(theme.text))]
        };
        meta.push(meta_line(
            alice_core::t!(crate::locale(), "tui.label.from"),
            from,
            theme,
        ));
        let to = one_line(&crate::terminal_text(&message.to.join(", ")));
        if !to.is_empty() {
            meta.push(meta_line(
                alice_core::t!(crate::locale(), "tui.label.to"),
                vec![Span::styled(truncate(&to, room), theme.fg(theme.text))],
                theme,
            ));
        }
        meta.push(meta_line(
            alice_core::t!(crate::locale(), "tui.label.date"),
            vec![Span::styled(long_date(&message.date), theme.fg(theme.text))],
            theme,
        ));
        if !message.attachments.is_empty() {
            let names = message
                .attachments
                .iter()
                .map(|file| one_line(&crate::terminal_text(&file.name)))
                .collect::<Vec<_>>()
                .join(", ");
            meta.push(meta_line(
                alice_core::t!(crate::locale(), "tui.label.files"),
                vec![Span::styled(
                    truncate(&format!("{} · {names}", message.attachments.len()), room),
                    theme.fg(theme.text),
                )],
                theme,
            ));
        }
        if let Some(invite) = &message.invite {
            invite_lines(&mut meta, message, invite, room, theme);
        }
    }
    let gap = u16::from(!compact);
    let meta_height = meta.len() as u16;
    let [subject_area, _, meta_area, _, rule_area, _, body_area] = Layout::vertical([
        Constraint::Length(subject_height),
        Constraint::Length(gap),
        Constraint::Length(meta_height),
        Constraint::Length(gap),
        Constraint::Length(1),
        Constraint::Length(gap),
        Constraint::Fill(1),
    ])
    .areas(inner);
    let body = Paragraph::new(crate::terminal_text(&message.body).replace('\t', "    "))
        .style(theme.fg(theme.text))
        .wrap(Wrap { trim: false });
    let lines = body.line_count(body_area.width);
    let overflow = lines.saturating_sub(body_area.height as usize);
    app.scroll = app.scroll.min(overflow.min(u16::MAX as usize) as u16);
    let block = if overflow > 0 {
        let seen = (app.scroll as usize + body_area.height as usize).min(lines);
        block.title_bottom(
            Line::styled(
                format!(
                    " {} ",
                    alice_core::t!(
                        crate::locale(),
                        "tui.message.scroll",
                        percent = seen * 100 / lines.max(1)
                    )
                ),
                theme.fg(theme.muted),
            )
            .right_aligned(),
        )
    } else {
        block
    };
    frame.render_widget(block, area);
    frame.render_widget(subject, subject_area);
    frame.render_widget(Paragraph::new(meta), meta_area);
    frame.render_widget(
        Line::styled("─".repeat(width), theme.fg(theme.faint)),
        rule_area,
    );
    frame.render_widget(body.scroll((app.scroll, 0)), body_area);
}

fn meta_line<'a>(label: String, mut value: Vec<Span<'a>>, theme: &Theme) -> Line<'a> {
    value.insert(
        0,
        Span::styled(padded_label(&label, LABEL as usize), theme.fg(theme.muted)),
    );
    Line::from(value)
}

fn padded_label(label: &str, width: usize) -> String {
    let label = truncate(label, width.saturating_sub(1));
    let padding = width.saturating_sub(label.width());
    format!("{label}{}", " ".repeat(padding))
}

/// The invitation block in the reader: what, when, where, who invited you and your answer.
fn invite_lines(
    meta: &mut Vec<Line<'static>>,
    message: &Message,
    invite: &alice_core::Invite,
    room: usize,
    theme: &Theme,
) {
    let text = |value: String| {
        vec![Span::styled(
            truncate(&one_line(&value), room),
            theme.fg(theme.text),
        )]
    };
    meta.push(meta_line(
        alice_core::t!(crate::locale(), "tui.label.event"),
        vec![Span::styled(
            truncate(&one_line(&crate::event_title(&invite.event)), room),
            theme.fg(theme.accent).add_modifier(Modifier::BOLD),
        )],
        theme,
    ));
    meta.push(meta_line(
        alice_core::t!(crate::locale(), "tui.label.when"),
        text(crate::event_when(&invite.event)),
        theme,
    ));
    if let Some(location) = invite
        .event
        .location
        .as_deref()
        .filter(|l| !l.trim().is_empty())
    {
        meta.push(meta_line(
            alice_core::t!(crate::locale(), "tui.label.where"),
            text(crate::terminal_text(location)),
            theme,
        ));
    }
    if let Some(organizer) = invite
        .organizer
        .as_ref()
        .filter(|o| !o.email.eq_ignore_ascii_case(&message.from_email))
    {
        meta.push(meta_line(
            alice_core::t!(crate::locale(), "tui.label.host"),
            text(crate::person(organizer)),
            theme,
        ));
    }
    let state = crate::invite_state(invite);
    let color = match invite.response {
        _ if invite.cancelled => theme.error,
        None => theme.star,
        Some(alice_core::Rsvp::Accepted) => theme.accent,
        Some(_) => theme.muted,
    };
    let hint = if invite.response.is_some() {
        alice_core::t!(crate::locale(), "tui.invite.change")
    } else {
        alice_core::t!(crate::locale(), "tui.invite.answer")
    };
    let show_hint = crate::answerable(invite) && state.width() + hint.width() <= room;
    let mut answer = vec![Span::styled(truncate(state, room), theme.fg(color))];
    if show_hint {
        answer.push(Span::styled(hint, theme.fg(theme.faint)));
    }
    meta.push(meta_line(
        alice_core::t!(crate::locale(), "tui.label.rsvp"),
        answer,
        theme,
    ));
}

fn compose(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let Some(draft) = &app.editing else {
        return;
    };
    let block = pane(theme)
        .title(Line::styled(
            format!(" {} ", app.editor_title),
            theme.fg(theme.accent).add_modifier(Modifier::BOLD),
        ))
        .padding(Padding::new(2, 2, 1, 0));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width <= LABEL || inner.height < 6 {
        return;
    }
    let row = |y: u16| Rect::new(inner.x, inner.y + y, inner.width, 1);
    let value = |y: u16| Rect::new(inner.x + LABEL, inner.y + y, inner.width - LABEL, 1);
    let label = |text: String, focused: bool| {
        Span::styled(
            padded_label(&text, LABEL as usize),
            if focused {
                theme.fg(theme.accent).add_modifier(Modifier::BOLD)
            } else {
                theme.fg(theme.muted)
            },
        )
    };
    let from = app
        .accounts
        .iter()
        .find(|a| a.id == draft.account_id)
        .map(|a| a.email.as_str())
        .unwrap_or(&draft.account_id);
    let mut from_line = vec![
        label(alice_core::t!(crate::locale(), "tui.label.from"), false),
        Span::styled(one_line(&crate::terminal_text(from)), theme.fg(theme.text)),
    ];
    if !app.editor_locked && app.accounts.len() > 1 {
        from_line.push(Span::styled(
            alice_core::t!(crate::locale(), "tui.editor.switch_account"),
            theme.fg(theme.faint),
        ));
    }
    frame.render_widget(Line::from(from_line), row(0));
    for (index, y, name) in [(0, 1, "tui.label.to"), (1, 2, "tui.label.subject")] {
        frame.render_widget(
            label(i18n::tr(crate::locale(), name, &[]), app.field == index),
            row(y),
        );
        if app.field == index {
            mark_focus(frame, row(y), theme);
        }
    }
    let focused_body = app.field == 2;
    let rule_style = theme.fg(if focused_body {
        theme.accent
    } else {
        theme.faint
    });
    frame.render_widget(
        Line::from(vec![
            label(
                alice_core::t!(crate::locale(), "tui.label.message"),
                focused_body,
            ),
            Span::styled(
                "─".repeat(inner.width.saturating_sub(LABEL) as usize),
                rule_style,
            ),
        ]),
        row(3),
    );
    if focused_body {
        mark_focus(frame, row(3), theme);
    }
    let body = Rect::new(
        inner.x,
        inner.y + 5,
        inner.width,
        inner.height.saturating_sub(5),
    );
    for (index, area, text, placeholder) in [
        (
            0,
            value(1),
            app.editor_to.as_str(),
            alice_core::t!(crate::locale(), "tui.editor.to_placeholder"),
        ),
        (
            1,
            value(2),
            draft.subject.as_str(),
            alice_core::t!(crate::locale(), "tui.editor.subject_placeholder"),
        ),
        (
            2,
            body,
            draft.body.as_str(),
            alice_core::t!(crate::locale(), "tui.editor.body_placeholder"),
        ),
    ] {
        text_field(
            frame,
            area,
            text,
            app.cursors[index],
            app.field == index,
            &placeholder,
            theme,
        );
    }
    if app.field == 0 && !app.suggestions.is_empty() {
        suggestion_list(frame, value(1), app, theme);
    }
}

/// Contacts matching the recipient being typed, in a popup under the To field.
fn suggestion_list(frame: &mut Frame<'_>, anchor: Rect, app: &App, theme: &Theme) {
    let bounds = frame.area();
    let x = anchor.x.saturating_sub(1);
    let y = anchor.y + 1;
    let width = (anchor.width.min(64) + 2).min(bounds.right().saturating_sub(x));
    let height = (app.suggestions.len() as u16 + 2).min(bounds.bottom().saturating_sub(y));
    if width < 12 || height < 3 {
        return;
    }
    let area = Rect::new(x, y, width, height);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.fg(theme.accent));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    let room = (inner.width as usize).saturating_sub(2);
    for (row, contact) in app
        .suggestions
        .iter()
        .enumerate()
        .take(inner.height as usize)
    {
        let line = Rect::new(inner.x, inner.y + row as u16, inner.width, 1);
        let selected = row == app.suggestion;
        if selected {
            frame.buffer_mut().set_style(line, theme.selected());
        }
        let name = one_line(&crate::terminal_text(&contact.name));
        let email = one_line(&crate::terminal_text(&contact.email));
        let mut spans = vec![Span::styled(
            if selected { "› " } else { "  " },
            theme.fg(theme.accent),
        )];
        if name.is_empty() {
            spans.push(Span::styled(truncate(&email, room), theme.fg(theme.text)));
        } else {
            let name = truncate(&name, room);
            let rest = room.saturating_sub(name.width() + 2);
            spans.push(Span::styled(name, theme.fg(theme.text)));
            if rest >= 6 {
                spans.push(Span::styled(
                    format!("  {}", truncate(&email, rest)),
                    theme.fg(theme.muted),
                ));
            }
        }
        frame.render_widget(Line::from(spans), line);
    }
}

/// A violet bar in the left padding beside the focused compose row.
fn mark_focus(frame: &mut Frame<'_>, row: Rect, theme: &Theme) {
    if let Some(cell) = frame
        .buffer_mut()
        .cell_mut((row.x.saturating_sub(2), row.y))
    {
        cell.set_symbol("▎").set_style(theme.fg(theme.accent));
    }
}

/// Renders an editable value, scrolled so the cursor stays visible, and
/// places the terminal cursor when focused.
fn text_field(
    frame: &mut Frame<'_>,
    area: Rect,
    value: &str,
    cursor: usize,
    focused: bool,
    placeholder: &str,
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let before = &value[..cursor.min(value.len())];
    let row = before.chars().filter(|c| *c == '\n').count();
    let column = Line::from(crate::terminal_text(
        before.rsplit('\n').next().unwrap_or(""),
    ))
    .width();
    let y_scroll = row
        .saturating_sub(area.height.saturating_sub(1) as usize)
        .min(u16::MAX as usize) as u16;
    let x_scroll = column
        .saturating_sub(area.width.saturating_sub(1) as usize)
        .min(u16::MAX as usize) as u16;
    if value.is_empty() {
        frame.render_widget(
            Paragraph::new(placeholder)
                .style(theme.fg(theme.faint))
                .wrap(Wrap { trim: false }),
            area,
        );
    } else {
        frame.render_widget(
            Paragraph::new(crate::terminal_text(value))
                .style(theme.fg(theme.text))
                .scroll((y_scroll, x_scroll)),
            area,
        );
    }
    if focused {
        frame.set_cursor_position((
            area.x + (column.saturating_sub(x_scroll as usize) as u16).min(area.width - 1),
            area.y + (row.saturating_sub(y_scroll as usize) as u16).min(area.height - 1),
        ));
    }
}

fn status_line(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme, activity: Activity) {
    let mut spans = Vec::new();
    if app.confirm {
        spans.push(Span::styled(
            truncate(
                &alice_core::t!(crate::locale(), "tui.prompt.send"),
                area.width as usize,
            ),
            theme.fg(theme.accent).add_modifier(Modifier::BOLD),
        ));
    } else if app.rsvp {
        let title = app
            .selected
            .and_then(|i| app.messages.get(i))
            .and_then(|m| m.invite.as_ref())
            .map(|invite| crate::event_title(&invite.event))
            .unwrap_or_default();
        spans.push(Span::styled(
            truncate(
                &one_line(&alice_core::t!(
                    crate::locale(),
                    "tui.prompt.answer",
                    title = title
                )),
                area.width as usize,
            ),
            theme.fg(theme.accent).add_modifier(Modifier::BOLD),
        ));
    } else {
        if activity.busy {
            spans.push(Span::styled(
                format!("{} ", activity.spinner()),
                theme.fg(theme.accent),
            ));
        }
        if !app.status.text.is_empty() {
            let age = app.status.since.elapsed();
            let color = match app.status.tone {
                Tone::Info => theme.fade(theme.flash, theme.status, age),
                Tone::Error => theme.fade(theme.error_flash, theme.error, age),
            };
            let room = (area.width as usize).saturating_sub(Line::from(spans.clone()).width());
            spans.push(Span::styled(
                truncate(&one_line(&crate::terminal_text(&app.status.text)), room),
                theme.fg(color),
            ));
        }
    }
    frame.render_widget(Line::from(spans), area);
}

fn hints(app: &App) -> Vec<(&'static str, &'static str)> {
    if app.confirm {
        return vec![("y", "tui.hint.send"), ("", "tui.hint.cancel")];
    }
    if app.rsvp {
        return vec![
            ("y", "tui.hint.accept"),
            ("m", "tui.hint.maybe"),
            ("n", "tui.hint.decline"),
            ("", "tui.hint.cancel"),
        ];
    }
    if app.editing.is_some() {
        let mut hints = Vec::new();
        if app.field == 0 && !app.suggestions.is_empty() {
            hints.extend([("↓↑", "tui.hint.choose"), ("↵", "tui.hint.add_recipient")]);
        }
        hints.extend([
            ("Tab", "tui.hint.next_field"),
            ("Ctrl+S", "tui.hint.save"),
            ("Esc", "tui.hint.save_close"),
        ]);
        if !app.editor_locked && app.accounts.len() > 1 {
            hints.push(("F2", "tui.hint.switch_from"));
        }
        hints.push(("← →", "tui.hint.move_cursor"));
        return hints;
    }
    if app.searching {
        return vec![
            ("↵", "tui.hint.done"),
            ("Esc", "tui.hint.done"),
            ("⌫", "tui.hint.delete"),
        ];
    }
    let drafts = FOLDERS[app.folder_index] == "drafts";
    let mut hints = vec![("j/k", "tui.hint.move")];
    if drafts {
        hints.push(("!", "tui.hint.send"));
        hints.push(("r", "tui.hint.edit"));
    } else {
        hints.push(("↵", "tui.hint.mark_read"));
        hints.push(("r", "tui.hint.reply"));
    }
    if app
        .selected
        .and_then(|i| app.messages.get(i))
        .and_then(|m| m.invite.as_ref())
        .is_some_and(crate::answerable)
    {
        hints.push(("i", "tui.hint.answer_invite"));
    }
    hints.extend([
        ("c", "tui.hint.compose"),
        ("/", "tui.hint.search"),
        ("g", "tui.hint.folder"),
        ("s", "tui.hint.star"),
        ("a", "tui.hint.archive"),
        ("d", "tui.hint.trash"),
    ]);
    if let Some(message) = app.selected.and_then(|i| app.messages.get(i))
        && !matches!(message.folder.as_str(), "drafts" | "sent")
    {
        hints.push((
            "x",
            if message.folder == "spam" {
                "tui.hint.not_spam"
            } else {
                "tui.hint.spam"
            },
        ));
    }
    if app.offset + PAGE < app.total {
        hints.push(("]", "tui.hint.next_page"));
    }
    if app.offset > 0 {
        hints.push(("[", "tui.hint.prev_page"));
    }
    hints.extend([
        ("u", "tui.hint.unread"),
        ("S", "tui.hint.sync"),
        ("q", "tui.hint.quit"),
    ]);
    hints
}

fn hint_line(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let chip = |key: &str, label: &str| {
        let key = if key.is_empty() {
            std::borrow::Cow::Owned(alice_core::t!(crate::locale(), "tui.hint.any_key"))
        } else {
            std::borrow::Cow::Borrowed(key)
        };
        [
            Span::styled(format!(" {key} "), theme.chip()),
            Span::styled(
                format!(" {}", i18n::tr(crate::locale(), label, &[])),
                theme.fg(theme.muted),
            ),
        ]
    };
    let help = app.editing.is_none() && !app.searching && !app.confirm && !app.rsvp;
    let reserve = if help {
        Line::from(chip("?", "tui.hint.all_keys").to_vec()).width()
    } else {
        0
    };
    let mut spans = Vec::new();
    let mut used = 0;
    for (key, label) in hints(app) {
        let pair = chip(key, label);
        let width = Line::from(pair.to_vec()).width() + 2;
        if used + width + reserve > area.width as usize {
            break;
        }
        spans.extend(pair);
        spans.push(Span::raw("  "));
        used += width;
    }
    if help && used + reserve <= area.width as usize {
        spans.extend(chip("?", "tui.hint.all_keys"));
    }
    frame.render_widget(Line::from(spans), area);
}

const HELP: [(&str, &[(&str, &str)]); 4] = [
    (
        "tui.help.browse",
        &[
            ("j k  ↓ ↑", "tui.help.move"),
            ("PgUp PgDn", "tui.help.scroll"),
            ("[  ]", "tui.help.page"),
            ("g", "tui.help.folder"),
            ("/", "tui.help.search"),
            ("S", "tui.help.sync"),
        ],
    ),
    (
        "tui.help.message",
        &[
            ("↵", "tui.help.mark_read"),
            ("u", "tui.help.mark_unread"),
            ("s", "tui.help.star"),
            ("a", "tui.help.archive"),
            ("d", "tui.help.trash"),
            ("x", "tui.help.spam"),
            ("!", "tui.help.send"),
            ("i", "tui.help.invite"),
        ],
    ),
    (
        "tui.help.write",
        &[
            ("c", "tui.help.new"),
            ("r", "tui.help.reply"),
            ("Tab", "tui.help.field"),
            ("↓↑ ↵", "tui.help.recipient"),
            ("←→ Home End", "tui.help.cursor"),
            ("F2", "tui.help.account"),
            ("Ctrl+S  Esc", "tui.help.save"),
        ],
    ),
    (
        "tui.help.general",
        &[("?", "tui.help.toggle"), ("q  Ctrl+C", "tui.help.quit")],
    ),
];

fn help(frame: &mut Frame<'_>, area: Rect, app: &mut App, theme: &Theme) {
    let section = |(title, keys): (&'static str, &[(&'static str, &'static str)])| {
        let mut lines = vec![Line::styled(
            i18n::tr(crate::locale(), title, &[]),
            theme.fg(theme.accent).add_modifier(Modifier::BOLD),
        )];
        for (key, description) in keys {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {key:<13}"),
                    theme.fg(theme.text).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    i18n::tr(crate::locale(), description, &[]),
                    theme.fg(theme.muted),
                ),
            ]));
        }
        lines.push(Line::default());
        lines
    };
    let two_columns = area.width >= 90;
    let columns: Vec<Paragraph<'_>> = if two_columns {
        vec![
            Paragraph::new(
                [HELP[0], HELP[3]]
                    .into_iter()
                    .flat_map(section)
                    .collect::<Vec<_>>(),
            )
            .wrap(Wrap { trim: true }),
            Paragraph::new(
                [HELP[1], HELP[2]]
                    .into_iter()
                    .flat_map(section)
                    .collect::<Vec<_>>(),
            )
            .wrap(Wrap { trim: true }),
        ]
    } else {
        vec![
            Paragraph::new(HELP.into_iter().flat_map(section).collect::<Vec<_>>())
                .wrap(Wrap { trim: true }),
        ]
    };
    let width = if two_columns { 114 } else { 64 }.min(area.width.saturating_sub(2));
    let column_width = width.saturating_sub(6) / columns.len() as u16;
    let content_height = columns
        .iter()
        .map(|paragraph| paragraph.line_count(column_width.max(1)))
        .max()
        .unwrap_or(0)
        .min(u16::MAX as usize) as u16;
    let height = content_height
        .saturating_add(3)
        .min(area.height.saturating_sub(2));
    let popup = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.fg(theme.accent))
        .title(Line::styled(
            format!(" {} ", alice_core::t!(crate::locale(), "tui.help.title")),
            theme.fg(theme.text).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(
            Line::styled(
                format!(" {} ", alice_core::t!(crate::locale(), "tui.help.close")),
                theme.fg(theme.muted),
            )
            .right_aligned(),
        )
        .padding(Padding::new(2, 2, 1, 0));
    let inner = block.inner(popup);
    app.help_scroll = app
        .help_scroll
        .min(content_height.saturating_sub(inner.height));
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    let areas = Layout::horizontal(vec![Constraint::Fill(1); columns.len()]).split(inner);
    for (paragraph, column) in columns.into_iter().zip(areas.iter()) {
        frame.render_widget(paragraph.scroll((app.help_scroll, 0)), *column);
    }
}

/// `9:42` for today, `Oct 3` this year, `Oct 3 2024` otherwise; anything
/// that is not RFC 3339 is shown as-is.
fn short_date(raw: &str, now: DateTime<Local>, locale: i18n::Locale) -> String {
    match DateTime::parse_from_rfc3339(raw) {
        Ok(date) => {
            let date = date.with_timezone(&Local);
            if date.date_naive() == now.date_naive() {
                i18n::date(locale, &date, &alice_core::t!(locale, "tui.date.today"))
            } else if date.year() == now.year() {
                i18n::date(locale, &date, &alice_core::t!(locale, "tui.date.this_year"))
            } else {
                i18n::date(
                    locale,
                    &date,
                    &alice_core::t!(locale, "tui.date.other_year"),
                )
            }
        }
        Err(_) => truncate(&one_line(&crate::terminal_text(raw)), 12),
    }
}

fn long_date(raw: &str) -> String {
    match DateTime::parse_from_rfc3339(raw) {
        Ok(date) => i18n::date(
            crate::locale(),
            &date.with_timezone(&Local),
            &alice_core::t!(crate::locale(), "tui.date.long"),
        ),
        Err(_) => one_line(&crate::terminal_text(raw)),
    }
}

/// Collapses newlines and runs of whitespace so a value fits one row.
fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Cuts `value` to at most `max` terminal columns, ending in `…` when cut.
fn truncate(value: &str, max: usize) -> String {
    if value.width() <= max {
        return value.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in value.chars() {
        let width = c.width().unwrap_or(0);
        if used + width + 1 > max {
            break;
        }
        out.push(c);
        used += width;
    }
    out.push('…');
    out
}

/// Keeps the end of `value` within `max` columns, so the text being typed
/// stays visible.
fn tail(value: &str, max: usize) -> String {
    if value.width() <= max {
        return value.to_string();
    }
    let mut kept = Vec::new();
    let mut used = 1;
    for c in value.chars().rev() {
        let width = c.width().unwrap_or(0);
        if used + width > max {
            break;
        }
        kept.push(c);
        used += width;
    }
    std::iter::once('…').chain(kept.into_iter().rev()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_by_display_width() {
        assert_eq!(truncate("hello", 5), "hello");
        assert_eq!(truncate("hello world", 6), "hello…");
        assert_eq!(truncate("你好世界", 5), "你好…");
        assert_eq!(truncate("你好世界", 5).width(), 5);
        assert_eq!(tail("search term", 5), "…term");
    }

    #[test]
    fn dates_are_relative() {
        let now = DateTime::parse_from_rfc3339("2026-10-11T12:00:00Z")
            .unwrap()
            .with_timezone(&Local);
        let today = short_date("2026-10-11T12:00:00Z", now, i18n::Locale::EnUs);
        assert!(today.contains(':'), "{today}");
        assert_eq!(
            short_date("2026-10-03T12:00:00Z", now, i18n::Locale::EnUs),
            "Oct 3"
        );
        assert_eq!(
            short_date("2024-10-03T12:00:00Z", now, i18n::Locale::EnUs),
            "Oct 3 2024"
        );
        assert_eq!(
            short_date("yesterday", now, i18n::Locale::EnUs),
            "yesterday"
        );
    }

    #[test]
    fn previews_fit_one_row() {
        assert_eq!(one_line("a\n\tb  c"), "a b c");
    }
}
