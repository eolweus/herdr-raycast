use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

pub struct Search {
    matcher: Matcher,
    buf: Vec<char>,
}

impl Default for Search {
    fn default() -> Self {
        Self { matcher: Matcher::new(Config::DEFAULT), buf: Vec::new() }
    }
}

// Exact > whole word > prefix > substring > fuzzy; the first field (title) beats the rest.
fn tier(field: &str, query: &str, title: bool) -> u8 {
    let f = field.to_lowercase();
    let t = if f == query {
        5
    } else if f.split(|c: char| !c.is_alphanumeric()).any(|w| w == query) {
        4
    } else if f.starts_with(query) {
        3
    } else if f.contains(query) {
        2
    } else {
        1
    };
    if title { t + 5 } else { t }
}

impl Search {
    /// Returns indexes into `hays`, best first. Empty query keeps the input order.
    pub fn rank<'a>(&mut self, query: &str, hays: impl Iterator<Item = &'a [String]>) -> Vec<usize> {
        self.rank_used(query, hays, &[])
    }

    /// Like `rank`, but within a match tier more-used items come first.
    pub fn rank_used<'a>(&mut self, query: &str, hays: impl Iterator<Item = &'a [String]>, usage: &[u32]) -> Vec<usize> {
        if query.trim().is_empty() {
            return hays.enumerate().map(|(i, _)| i).collect();
        }
        let q = query.trim().to_lowercase();
        let pat = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let mut scored: Vec<((u8, u32, u32), usize)> = hays
            .enumerate()
            .filter_map(|(i, fields)| {
                fields
                    .iter()
                    .enumerate()
                    .filter_map(|(n, f)| pat.score(Utf32Str::new(f, &mut self.buf), &mut self.matcher).map(|s| (tier(f, &q, n == 0), s)))
                    .max()
                    .map(|(t, s)| ((t, usage.get(i).copied().unwrap_or(0), s), i))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        scored.into_iter().map(|(_, i)| i).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hays(v: &[&[&str]]) -> Vec<Vec<String>> {
        v.iter().map(|f| f.iter().map(|s| s.to_string()).collect()).collect()
    }

    #[test]
    fn ranks_and_filters() {
        let items = hays(&[&["New worktree"], &["Rename tab"], &["tile cache", "w5"]]);
        let mut s = Search::default();
        let r = s.rank("wt", items.iter().map(|v| v.as_slice()));
        assert_eq!(r, vec![0]);
        let r = s.rank("", items.iter().map(|v| v.as_slice()));
        assert_eq!(r, vec![0, 1, 2]);
    }

    #[test]
    fn exact_then_title_then_fuzzy() {
        let items = hays(&[&["Tilecache scope", "tile cache"], &["shell", "tile cache"], &["tile cache"], &["tile"], &["retire atlas", "titled elsewhere"]]);
        let mut s = Search::default();
        let r = s.rank("tile", items.iter().map(|v| v.as_slice()));
        assert_eq!(r, vec![3, 2, 0, 1, 4]);
    }

    #[test]
    fn usage_breaks_ties_within_tier() {
        let items = hays(&[&["Rename tab"], &["Rename agent"]]);
        let mut s = Search::default();
        let r = s.rank_used("ren", items.iter().map(|v| v.as_slice()), &[0, 5]);
        assert_eq!(r, vec![1, 0]);
    }
}
