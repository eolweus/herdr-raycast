use crate::app::{App, Flow, Kind, Mode, View};
use crate::commands::InputKind;
use crate::herdr::AgentStatus;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    draw_header(f, app, rows[0]);
    draw_search(f, app, rows[1]);
    let body = rows[2];
    if app.preview && app.flow.is_none() {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(body);
        draw_list(f, app, cols[0]);
        draw_preview(f, app, cols[1]);
    } else {
        draw_list(f, app, body);
    }
    draw_footer(f, app, rows[3]);
}

fn status_style(s: AgentStatus) -> Style {
    match s {
        AgentStatus::Blocked => Style::default().fg(Color::Red).bold(),
        AgentStatus::Done => Style::default().fg(Color::Cyan).bold(),
        AgentStatus::Working => Style::default().fg(Color::Yellow),
        AgentStatus::Idle => Style::default().fg(Color::Green),
        AgentStatus::Unknown => Style::default().fg(Color::DarkGray),
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![Span::raw(" ")];
    for (i, v) in View::ALL.iter().enumerate() {
        let label = format!(" {} {} ", i + 1, v.title());
        let style = if *v == app.view {
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        spans.push(Span::styled(label, style));
        spans.push(Span::raw(" "));
    }
    let left = Line::from(spans);
    let dir = crate::context::shorten_home(&app.ctx.dir);
    let ws = app.ctx.workspace_label.clone().or_else(|| app.ctx.workspace_id.clone().map(|id| app.snapshot.workspace_label(&id))).unwrap_or_default();
    let right = if ws.is_empty() { dir } else { format!("{ws}  {dir}") };
    let lw = left.width() as u16;
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(lw), Constraint::Min(1)])
        .split(area);
    f.render_widget(Paragraph::new(left), cols[0]);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(truncate_left(&right, cols[1].width.saturating_sub(1) as usize), Style::default().fg(Color::DarkGray))))
            .right_aligned(),
        cols[1],
    );
}

fn draw_search(f: &mut Frame, app: &App, area: Rect) {
    let (prompt, text, cursor_visible) = match &app.flow {
        Some(Flow::Menu { query, .. }) => ("actions".to_string(), query.clone(), true),
        Some(Flow::Input { draft, step, text, query, .. }) => {
            let input = &draft.command.inputs[*step];
            let is_text = matches!(input.kind, InputKind::Text);
            let label = if input.prompt.is_empty() { input.name.clone() } else { input.prompt.clone() };
            (format!("{} · {label}", draft.command.title), if is_text { text.clone() } else { query.clone() }, true)
        }
        Some(Flow::Confirm { draft }) => (format!("{}?", draft.command.title), "press y to confirm, any other key cancels".to_string(), false),
        None => (
            match app.mode {
                Mode::Insert => "search".to_string(),
                Mode::Normal => "NORMAL".to_string(),
            },
            app.query.clone(),
            app.mode == Mode::Insert,
        ),
    };
    let prompt_style = if app.mode == Mode::Normal && app.flow.is_none() {
        Style::default().fg(Color::Magenta).bold()
    } else {
        Style::default().fg(Color::Blue).bold()
    };
    let mut spans = vec![Span::raw(" "), Span::styled(format!("{prompt} ›"), prompt_style), Span::raw(" "), Span::raw(text.clone())];
    if cursor_visible {
        spans.push(Span::styled(" ", Style::default().add_modifier(Modifier::REVERSED)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

struct Row {
    glyph: Option<(String, Style)>,
    indent: usize,
    title: String,
    meta: String,
}

fn rows(app: &App) -> (Vec<Row>, usize) {
    match &app.flow {
        Some(Flow::Menu { actions, cursor, query }) => {
            let hays: Vec<Vec<String>> = actions.iter().map(|(c, _)| vec![c.title.clone()]).collect();
            let mut s = crate::search::Search::default();
            let idx = s.rank(query, hays.iter().map(|v| v.as_slice()));
            (idx.into_iter().map(|i| Row { glyph: None, indent: 0, title: actions[i].0.title.clone(), meta: actions[i].0.class.label().into() }).collect(), *cursor)
        }
        Some(Flow::Input { draft, step, options, query, cursor, .. }) => {
            let input = &draft.command.inputs[*step];
            if matches!(input.kind, InputKind::Text) {
                let mut r = vec![Row { glyph: None, indent: 0, title: "type a value and press enter".into(), meta: String::new() }];
                for (k, v) in &draft.answers {
                    if !k.ends_with("_label") {
                        r.push(Row { glyph: None, indent: 1, title: format!("{k} = {v}"), meta: String::new() });
                    }
                }
                return (r, usize::MAX);
            }
            let hays: Vec<Vec<String>> = options.iter().map(|(v, l)| vec![l.clone(), v.clone()]).collect();
            let mut s = crate::search::Search::default();
            let idx = s.rank(query, hays.iter().map(|v| v.as_slice()));
            (idx.into_iter().map(|i| Row { glyph: None, indent: 0, title: options[i].1.clone(), meta: options[i].0.clone() }).collect(), *cursor)
        }
        Some(Flow::Confirm { draft }) => {
            let mut r = vec![Row { glyph: None, indent: 0, title: draft.command.title.clone(), meta: String::new() }];
            if let Some(item) = &draft.item {
                r.push(Row { glyph: None, indent: 1, title: format!("on {}", item.id()), meta: String::new() });
            }
            (r, usize::MAX)
        }
        None => (
            app.filtered
                .iter()
                .map(|&i| {
                    let it = &app.items[i];
                    let glyph = match it.kind {
                        Kind::Command(_) => Some(("›".to_string(), Style::default().fg(Color::DarkGray))),
                        _ => it.status.map(|s| (s.glyph().to_string(), status_style(s))),
                    };
                    Row { glyph, indent: it.depth as usize, title: it.title.clone(), meta: it.meta.clone() }
                })
                .collect(),
            app.cursor,
        ),
    }
}

fn draw_list(f: &mut Frame, app: &App, area: Rect) {
    let (rows, cursor) = rows(app);
    let height = area.height as usize;
    if height == 0 {
        return;
    }
    let first = if cursor == usize::MAX || cursor < height { 0 } else { cursor + 1 - height };
    let width = area.width as usize;
    let mut lines = vec![];
    for (i, row) in rows.iter().enumerate().skip(first).take(height) {
        let selected = i == cursor;
        let mut spans = vec![Span::raw(if selected { "▌" } else { " " })];
        let mut used = 1;
        if let Some((g, st)) = &row.glyph {
            spans.push(Span::styled(format!("{g} "), *st));
            used += g.width() + 1;
        } else {
            spans.push(Span::raw("  "));
            used += 2;
        }
        let indent = "  ".repeat(row.indent);
        spans.push(Span::raw(indent.clone()));
        used += indent.len();
        let meta_w = row.meta.width();
        let avail_title = width.saturating_sub(used + if meta_w > 0 { meta_w.min(width / 2) + 2 } else { 0 });
        let title = truncate(&row.title, avail_title);
        let tw = title.width();
        spans.push(Span::styled(title, if selected { Style::default().bold() } else { Style::default() }));
        used += tw;
        if meta_w > 0 {
            let meta = truncate(&row.meta, width.saturating_sub(used + 2).min(width / 2));
            let pad = width.saturating_sub(used + meta.width() + 1);
            spans.push(Span::raw(" ".repeat(pad)));
            spans.push(Span::styled(meta, Style::default().fg(Color::DarkGray)));
        }
        let mut line = Line::from(spans);
        if selected {
            line = line.style(Style::default().add_modifier(Modifier::REVERSED));
        }
        lines.push(line);
    }
    if rows.is_empty() {
        lines.push(Line::from(Span::styled("  nothing matches", Style::default().fg(Color::DarkGray))));
    }
    f.render_widget(Paragraph::new(lines), area);
}

fn draw_preview(f: &mut Frame, app: &App, area: Rect) {
    let lines = app.preview_lines();
    let block = Block::default().borders(Borders::LEFT).border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let h = inner.height as usize;
    let skip = lines.len().saturating_sub(h).min(if lines.len() > h { lines.len() - h } else { 0 });
    let head: Vec<Line> = if lines.len() > h {
        let keep_head = 5.min(lines.len());
        let mut v: Vec<Line> = lines.iter().take(keep_head).map(|l| Line::from(l.as_str())).collect();
        v.push(Line::from(Span::styled("…", Style::default().fg(Color::DarkGray))));
        v.extend(lines.iter().skip((skip + keep_head + 1).min(lines.len())).map(|l| Line::from(l.as_str())));
        v
    } else {
        lines.iter().map(|l| Line::from(l.as_str())).collect()
    };
    f.render_widget(Paragraph::new(head), inner);
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let text = if let Some(s) = &app.status {
        s.clone()
    } else {
        match (&app.flow, app.mode) {
            (Some(Flow::Menu { .. }), _) => "⏎ run  esc back  type to filter".into(),
            (Some(Flow::Input { .. }), _) => "⏎ next  esc cancel".into(),
            (Some(Flow::Confirm { .. }), _) => "y confirm  any key cancels".into(),
            (None, Mode::Insert) => "esc normal  tab views  ⏎ open  ^o actions  ^c quit".into(),
            (None, Mode::Normal) => "i search  1-4 views  ⏎ open  ^o actions  r rename  x close  N new ws  a agent  p preview  q quit".into(),
        }
    };
    let style = if app.status.as_deref().is_some_and(|s| s.starts_with("error")) {
        Style::default().fg(Color::Red)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    f.render_widget(Paragraph::new(Line::from(Span::styled(format!(" {text}"), style))), area);
}

fn truncate(s: &str, max: usize) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    for c in s.chars() {
        if out.width() + 1 >= max {
            break;
        }
        out.push(c);
    }
    out.push('…');
    out
}

fn truncate_left(s: &str, max: usize) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    for c in chars.iter().rev() {
        if out.width() + 1 >= max {
            break;
        }
        out.insert(0, *c);
    }
    format!("…{out}")
}
