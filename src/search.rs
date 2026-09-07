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

impl Search {
    /// Returns indexes into `hays`, best first. Empty query keeps the input order.
    pub fn rank<'a>(&mut self, query: &str, hays: impl Iterator<Item = &'a [String]>) -> Vec<usize> {
        if query.trim().is_empty() {
            return hays.enumerate().map(|(i, _)| i).collect();
        }
        let pat = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let mut scored: Vec<(u32, usize)> = hays
            .enumerate()
            .filter_map(|(i, fields)| {
                fields
                    .iter()
                    .filter_map(|f| pat.score(Utf32Str::new(f, &mut self.buf), &mut self.matcher))
                    .max()
                    .map(|s| (s, i))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        scored.into_iter().map(|(_, i)| i).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_and_filters() {
        let items = vec![vec!["New worktree".to_string()], vec!["Rename tab".to_string()], vec!["tile cache".to_string(), "w5".to_string()]];
        let mut s = Search::default();
        let r = s.rank("wt", items.iter().map(|v| v.as_slice()));
        assert_eq!(r, vec![0]);
        let r = s.rank("", items.iter().map(|v| v.as_slice()));
        assert_eq!(r, vec![0, 1, 2]);
    }
}
