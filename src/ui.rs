use crate::app::{App, Flow, Kind, Mode, Seg, Tone, View};
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
    let lw = left.width() as u16;
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(lw), Constraint::Min(1)])
        .split(area);
    f.render_widget(Paragraph::new(left), cols[0]);
    let avail = cols[1].width.saturating_sub(1) as usize;
    let mut right = vec![];
    if !ws.is_empty() && ws.width() + 3 + 8 < avail {
        right.push(Span::styled(ws.clone(), tone_style(Tone::Workspace).bold()));
        right.push(Span::raw("  "));
    }
    let used: usize = right.iter().map(|s| s.width()).sum();
    right.push(Span::styled(truncate_left(&dir, avail.saturating_sub(used)), tone_style(Tone::Path).bold()));
    right.push(Span::raw(" "));
    f.render_widget(Paragraph::new(Line::from(right)).right_aligned(), cols[1]);
}

fn tone_style(t: Tone) -> Style {
    match t {
        Tone::Plain => Style::default(),
        Tone::Dim | Tone::Rule => Style::default().fg(Color::DarkGray),
        Tone::Title => Style::default().bold(),
        Tone::Heading => Style::default().fg(Color::Blue).bold(),
        Tone::Strong => Style::default().bold(),
        Tone::Em => Style::default().italic(),
        Tone::Code => Style::default().fg(Color::Yellow),
        Tone::Path => Style::default().fg(Color::Cyan),
        Tone::Agent => Style::default().fg(Color::Magenta),
        Tone::Workspace => Style::default().fg(Color::Blue),
        Tone::Status(s) => status_style(s),
    }
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
    depth: u8,
    current: bool,
    title: String,
    meta: Vec<Seg>,
    dim: bool,
    workspace: bool,
}

fn dim_meta(text: impl Into<String>) -> Vec<Seg> {
    let t: String = text.into();
    if t.is_empty() { vec![] } else { vec![Seg(t, Tone::Dim)] }
}

fn plain(depth: u8, title: String, meta: Vec<Seg>) -> Row {
    Row { glyph: None, depth, current: false, title, meta, dim: false, workspace: false }
}

enum Display {
    Spacer,
    Header(&'static str),
    Row(usize),
}

fn rows(app: &App) -> (Vec<Row>, Vec<Display>, usize) {
    let (rows, cursor): (Vec<Row>, usize) = match &app.flow {
        Some(Flow::Menu { actions, cursor, query }) => {
            let hays: Vec<Vec<String>> = actions.iter().map(|(c, _)| vec![c.title.clone()]).collect();
            let mut s = crate::search::Search::default();
            let idx = s.rank(query, hays.iter().map(|v| v.as_slice()));
            (idx.into_iter().map(|i| plain(0, actions[i].0.title.clone(), dim_meta(actions[i].0.class.label()))).collect(), *cursor)
        }
        Some(Flow::Input { draft, step, options, query, cursor, .. }) => {
            let input = &draft.command.inputs[*step];
            if matches!(input.kind, InputKind::Text) {
                let mut r = vec![plain(0, "type a value and press enter".into(), vec![])];
                for (k, v) in &draft.answers {
                    if !k.ends_with("_label") {
                        r.push(plain(1, format!("{k} = {v}"), vec![]));
                    }
                }
                (r, usize::MAX)
            } else {
                let hays: Vec<Vec<String>> = options.iter().map(|(v, l)| vec![l.clone(), v.clone()]).collect();
                let mut s = crate::search::Search::default();
                let idx = s.rank(query, hays.iter().map(|v| v.as_slice()));
                (idx.into_iter().map(|i| plain(0, options[i].1.clone(), if options[i].0 == options[i].1 { vec![] } else { dim_meta(options[i].0.clone()) })).collect(), *cursor)
            }
        }
        Some(Flow::Confirm { draft }) => {
            let mut r = vec![plain(0, draft.command.title.clone(), vec![])];
            if let Some(item) = &draft.item {
                r.push(plain(1, format!("on {}", item.id()), vec![]));
            }
            (r, usize::MAX)
        }
        None => (
            app.filtered
                .iter()
                .enumerate()
                .map(|(n, &i)| {
                    let it = &app.items[i];
                    let glyph = match it.kind {
                        Kind::Command(_) => Some(("›".to_string(), Style::default().fg(Color::DarkGray))),
                        Kind::Workspace if app.view != View::Workspaces => Some(("▪".to_string(), Style::default().fg(Color::Blue))),
                        _ => it.status.map(|s| (s.glyph().to_string(), status_style(s))),
                    };
                    Row {
                        glyph,
                        depth: it.depth,
                        current: it.current,
                        title: it.title.clone(),
                        meta: it.meta.clone(),
                        dim: app.context.get(n).copied().unwrap_or(false),
                        workspace: matches!(it.kind, Kind::Workspace),
                    }
                })
                .collect(),
            app.cursor,
        ),
    };
    let mut display = vec![];
    if app.flow.is_none() {
        match app.view {
            View::All => {
                let mut last = usize::MAX;
                for (n, &i) in app.filtered.iter().enumerate() {
                    let (sec, name) = if n < app.top { (usize::MAX - 1, "Top") } else { app.items[i].kind.section() };
                    if sec != last {
                        if last != usize::MAX {
                            display.push(Display::Spacer);
                        }
                        display.push(Display::Header(name));
                        last = sec;
                    }
                    display.push(Display::Row(n));
                }
            }
            View::Workspaces => {
                for (n, row) in rows.iter().enumerate() {
                    if row.depth == 0 && n > 0 {
                        display.push(Display::Spacer);
                    }
                    display.push(Display::Row(n));
                }
            }
            _ => display.extend((0..rows.len()).map(Display::Row)),
        }
    } else {
        display.extend((0..rows.len()).map(Display::Row));
    }
    (rows, display, cursor)
}

fn has_following_sibling(depths: &[u8], idx: usize, depth: u8) -> bool {
    depths[idx + 1..].iter().take_while(|d| **d >= depth).any(|d| *d == depth)
}

fn tree_prefix(depths: &[u8], idx: usize) -> String {
    let d = depths[idx];
    if d == 0 {
        return "▾".into();
    }
    let mut p = String::new();
    for level in 1..d {
        p.push_str(if has_following_sibling(depths, idx, level) { "│  " } else { "   " });
    }
    p.push_str(if has_following_sibling(depths, idx, d) { "├──" } else { "└──" });
    p
}

fn meta_spans(meta: &[Seg], max: usize, dim: bool) -> (Vec<Span<'static>>, usize) {
    if meta.is_empty() || max < 4 {
        return (vec![], 0);
    }
    let mut parts: Vec<(String, Tone)> = meta.iter().map(|s| (s.0.clone(), s.1)).collect();
    let total = |parts: &[(String, Tone)]| parts.iter().map(|p| p.0.width()).sum::<usize>() + 3 * parts.len().saturating_sub(1);
    // Shrink paths from the left first, then drop segments from the front.
    if total(&parts) > max {
        if let Some(pos) = parts.iter().position(|p| p.1 == Tone::Path) {
            let over = total(&parts) - max;
            let w = parts[pos].0.width();
            if w > over + 4 {
                parts[pos].0 = truncate_left(&parts[pos].0, w - over);
            }
        }
    }
    while parts.len() > 1 && total(&parts) > max {
        parts.remove(0);
    }
    if total(&parts) > max {
        parts[0].0 = truncate(&parts[0].0, max);
    }
    let mut spans = vec![];
    for (n, (text, tone)) in parts.iter().enumerate() {
        if n > 0 {
            spans.push(Span::styled(" · ", Style::default().fg(Color::DarkGray)));
        }
        let style = if dim { Style::default().fg(Color::DarkGray) } else { tone_style(*tone) };
        spans.push(Span::styled(text.clone(), style));
    }
    let w = total(&parts);
    (spans, w)
}

fn draw_list(f: &mut Frame, app: &App, area: Rect) {
    let (rows, display, cursor) = rows(app);
    let height = area.height as usize;
    if height == 0 {
        return;
    }
    let tree = app.flow.is_none() && app.view == View::Workspaces;
    let depths: Vec<u8> = rows.iter().map(|r| r.depth).collect();
    let cursor_line = display.iter().position(|d| matches!(d, Display::Row(n) if *n == cursor)).unwrap_or(0);
    let first = if cursor == usize::MAX || cursor_line < height { 0 } else { cursor_line + 1 - height };
    let width = area.width as usize;
    let mut lines = vec![];
    for d in display.iter().skip(first).take(height) {
        let i = match d {
            Display::Spacer => {
                lines.push(Line::default());
                continue;
            }
            Display::Header(name) => {
                let label = format!(" {name} ");
                let rule = "─".repeat(width.saturating_sub(label.width() + 1));
                lines.push(Line::from(vec![Span::styled(label, Style::default().fg(Color::DarkGray).bold()), Span::styled(rule, Style::default().fg(Color::DarkGray))]));
                continue;
            }
            Display::Row(i) => *i,
        };
        let row = &rows[i];
        let selected = i == cursor;
        let mut spans = vec![Span::raw(if selected { "▌" } else { " " })];
        let mut used = 1;
        if tree {
            let mark = if row.current { "◆ " } else { "  " };
            spans.push(Span::styled(mark, Style::default().fg(Color::Blue)));
            let prefix = format!("{} ", tree_prefix(&depths, i));
            used += 2 + prefix.width();
            spans.push(Span::styled(prefix, Style::default().fg(Color::DarkGray)));
        } else {
            let indent = "  ".repeat(row.depth as usize);
            used += indent.len();
            spans.push(Span::raw(indent));
        }
        if let Some((g, st)) = &row.glyph {
            spans.push(Span::styled(format!("{g} "), if row.dim { Style::default().fg(Color::DarkGray) } else { *st }));
            used += g.width() + 1;
        } else {
            spans.push(Span::raw("  "));
            used += 2;
        }
        let meta_max = width / 2;
        let avail_title = width.saturating_sub(used + if row.meta.is_empty() { 0 } else { 3 });
        let title = truncate(&row.title, avail_title);
        let tw = title.width();
        let title_style = if row.dim {
            Style::default().fg(Color::DarkGray)
        } else if row.workspace && tree {
            Style::default().fg(Color::Blue).bold()
        } else if selected || row.current {
            Style::default().bold()
        } else {
            Style::default()
        };
        spans.push(Span::styled(title, title_style));
        used += tw;
        let (meta, mw) = meta_spans(&row.meta, meta_max.min(width.saturating_sub(used + 2)), row.dim);
        if mw > 0 {
            let pad = width.saturating_sub(used + mw + 1);
            spans.push(Span::raw(" ".repeat(pad)));
            spans.extend(meta);
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
    let block = Block::default().borders(Borders::LEFT).border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    let lines = app.preview_lines(inner.width as usize);
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
