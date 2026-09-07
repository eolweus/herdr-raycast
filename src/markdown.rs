//! Small markdown subset for preview text: headings, bullets, fenced and
//! inline code, bold and italic, wrapped paragraphs.
use crate::app::{Seg, Tone};
use unicode_width::UnicodeWidthStr;

pub fn render(text: &str, width: usize) -> Vec<Vec<Seg>> {
    let width = width.max(10);
    let mut out: Vec<Vec<Seg>> = vec![];
    let mut para: Vec<String> = vec![];
    let mut fence = false;
    let flush = |para: &mut Vec<String>, out: &mut Vec<Vec<Seg>>| {
        if !para.is_empty() {
            out.extend(wrap(inline(&para.join(" ")), width, ""));
            para.clear();
        }
    };
    for raw in text.lines() {
        let line = raw.trim_end();
        if line.trim_start().starts_with("```") {
            flush(&mut para, &mut out);
            fence = !fence;
            continue;
        }
        if fence {
            out.push(vec![Seg(format!("  {line}"), Tone::Code)]);
            continue;
        }
        let t = line.trim_start();
        if t.is_empty() {
            flush(&mut para, &mut out);
            out.push(vec![]);
        } else if let Some(h) = heading(t) {
            flush(&mut para, &mut out);
            out.push(vec![Seg(h.to_string(), Tone::Heading)]);
        } else if let Some(rest) = bullet(t) {
            flush(&mut para, &mut out);
            let mut segs = vec![Seg("• ".into(), Tone::Dim)];
            segs.extend(inline(rest));
            out.extend(wrap(segs, width, "  "));
        } else if let Some((n, rest)) = numbered(t) {
            flush(&mut para, &mut out);
            let mut segs = vec![Seg(format!("{n}. "), Tone::Dim)];
            segs.extend(inline(rest));
            out.extend(wrap(segs, width, "   "));
        } else {
            para.push(t.to_string());
        }
    }
    flush(&mut para, &mut out);
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out
}

fn heading(t: &str) -> Option<&str> {
    let hashes = t.len() - t.trim_start_matches('#').len();
    (1..=6).contains(&hashes).then(|| t[hashes..].trim()).filter(|h| !h.is_empty() && t.as_bytes()[hashes] == b' ')
}

fn bullet(t: &str) -> Option<&str> {
    ["- ", "* ", "+ "].iter().find_map(|b| t.strip_prefix(b))
}

fn numbered(t: &str) -> Option<(&str, &str)> {
    let digits = t.len() - t.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 {
        return None;
    }
    let rest = t[digits..].strip_prefix(". ").or_else(|| t[digits..].strip_prefix(") "))?;
    Some((&t[..digits], rest))
}

/// Inline spans: `code`, **bold**, *em* / _em_.
fn inline(text: &str) -> Vec<Seg> {
    let mut segs = vec![];
    let mut buf = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let push = |segs: &mut Vec<Seg>, buf: &mut String, tone| {
        if !buf.is_empty() {
            segs.push(Seg(std::mem::take(buf), tone));
        }
    };
    while i < chars.len() {
        let c = chars[i];
        let take = |open: &str, close: &str| -> Option<(String, usize)> {
            let o: Vec<char> = open.chars().collect();
            if chars[i..].starts_with(&o) {
                let start = i + o.len();
                let cl: Vec<char> = close.chars().collect();
                let mut j = start;
                while j + cl.len() <= chars.len() {
                    if chars[j..].starts_with(&cl) && j > start {
                        return Some((chars[start..j].iter().collect(), j + cl.len()));
                    }
                    j += 1;
                }
            }
            None
        };
        let word_boundary = i == 0 || !chars[i - 1].is_alphanumeric();
        if c == '`' {
            if let Some((inner, next)) = take("`", "`") {
                push(&mut segs, &mut buf, Tone::Plain);
                segs.push(Seg(inner, Tone::Code));
                i = next;
                continue;
            }
        } else if c == '*' || c == '_' {
            let d = c.to_string();
            let dd = d.repeat(2);
            if let Some((inner, next)) = take(&dd, &dd) {
                push(&mut segs, &mut buf, Tone::Plain);
                segs.extend(inline(&inner).into_iter().map(|s| Seg(s.0, if s.1 == Tone::Plain { Tone::Strong } else { s.1 })));
                i = next;
                continue;
            }
            if word_boundary {
                if let Some((inner, next)) = take(&d, &d) {
                    if !inner.starts_with(' ') && !inner.ends_with(' ') && chars.get(next).is_none_or(|n| !n.is_alphanumeric()) {
                        push(&mut segs, &mut buf, Tone::Plain);
                        segs.extend(inline(&inner).into_iter().map(|s| Seg(s.0, if s.1 == Tone::Plain { Tone::Em } else { s.1 })));
                        i = next;
                        continue;
                    }
                }
            }
        }
        buf.push(c);
        i += 1;
    }
    push(&mut segs, &mut buf, Tone::Plain);
    segs
}

/// Word-wrap styled segments; continuation lines get `indent`.
pub fn wrap(segs: Vec<Seg>, width: usize, indent: &str) -> Vec<Vec<Seg>> {
    let mut lines: Vec<Vec<Seg>> = vec![vec![]];
    let mut col = 0;
    for Seg(text, tone) in segs {
        let mut first = true;
        for piece in text.split(' ') {
            let space = !first;
            first = false;
            if piece.is_empty() {
                if space {
                    push_seg(lines.last_mut().unwrap(), " ", tone);
                    col += 1;
                }
                continue;
            }
            let w = piece.width();
            if col > 0 && col + usize::from(space) + w > width {
                let mut line = vec![];
                if !indent.is_empty() {
                    line.push(Seg(indent.to_string(), Tone::Plain));
                }
                lines.push(line);
                col = indent.width();
            } else if space {
                push_seg(lines.last_mut().unwrap(), " ", tone);
                col += 1;
            }
            push_seg(lines.last_mut().unwrap(), piece, tone);
            col += w;
        }
    }
    for line in &mut lines {
        if let Some(last) = line.last_mut() {
            let trimmed = last.0.trim_end().to_string();
            last.0 = trimmed;
        }
        line.retain(|s| !s.0.is_empty());
    }
    lines
}

fn push_seg(line: &mut Vec<Seg>, text: &str, tone: Tone) {
    match line.last_mut() {
        Some(last) if last.1 == tone => last.0.push_str(text),
        _ => line.push(Seg(text.to_string(), tone)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(lines: &[Vec<Seg>]) -> Vec<String> {
        lines.iter().map(|l| l.iter().map(|s| s.0.as_str()).collect()).collect()
    }

    #[test]
    fn inline_styles() {
        let segs = inline("run `herdr tab close` **now** or *later*, ok");
        let tones: Vec<Tone> = segs.iter().map(|s| s.1).collect();
        assert_eq!(tones, [Tone::Plain, Tone::Code, Tone::Plain, Tone::Strong, Tone::Plain, Tone::Em, Tone::Plain]);
        assert_eq!(segs[5].0, "later");
        assert_eq!(segs[6].0, ", ok");
        assert_eq!(inline("snake_case_name")[0].0, "snake_case_name");
        assert_eq!(inline("2 * 3 * 4")[0].0, "2 * 3 * 4");
    }

    #[test]
    fn blocks_and_wrap() {
        let md = "# Title\n\nFirst line\nsame paragraph here.\n\n- item one is long enough\n- `two`\n1. first\n```\ncode  \n```";
        let lines = render(md, 14);
        assert_eq!(
            flat(&lines),
            ["Title", "", "First line", "same paragraph", "here.", "", "• item one is", "  long enough", "• two", "1. first", "  code"]
        );
        assert_eq!(lines[0][0].1, Tone::Heading);
        assert_eq!(lines[8][1].1, Tone::Code);
        assert_eq!(lines[10][0].1, Tone::Code);
    }

    #[test]
    fn wrap_keeps_tone_across_lines() {
        let lines = wrap(vec![Seg("aaa ".into(), Tone::Plain), Seg("bbb ccc".into(), Tone::Strong)], 8, "");
        assert_eq!(flat(&lines), ["aaa bbb", "ccc"]);
        assert_eq!(lines[1][0].1, Tone::Strong);
    }
}
