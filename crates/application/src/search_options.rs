//! Shared Search sidebar settings and file-pattern validation.
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchOptions {
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
    pub include: String,
    pub exclude: String,
    pub use_ignore_files: bool,
    pub only_open_editors: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            match_case: false,
            whole_word: false,
            regex: false,
            include: String::new(),
            exclude: String::new(),
            use_ignore_files: true,
            only_open_editors: false,
        }
    }
}

impl SearchOptions {
    pub(crate) fn error(&self, query: &str) -> Option<String> {
        crate::search::Matcher::configured(query, self)
            .map(|_| ())
            .and_then(|()| FilePatterns::new(self).map(|_| ()))
            .err()
    }
}

pub(crate) struct FilePatterns {
    include: GlobSet,
    exclude: GlobSet,
}

impl FilePatterns {
    pub(crate) fn new(options: &SearchOptions) -> Result<Self, String> {
        Ok(Self {
            include: patterns(&options.include)?,
            exclude: patterns(&options.exclude)?,
        })
    }

    pub(crate) fn matches(&self, path: &str) -> bool {
        (self.include.is_empty() || self.include.is_match(path)) && !self.exclude.is_match(path)
    }
}

fn patterns(input: &str) -> Result<GlobSet, String> {
    let mut builder = GlobSetBuilder::new();
    // Commas inside brace alternatives belong to the glob, not the list.
    let mut depth = 0usize;
    let mut start = 0;
    let mut parts = Vec::new();
    for (index, ch) in input.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&input[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&input[start..]);
    for part in parts {
        let part = part.trim().trim_end_matches('/');
        if part.is_empty() {
            continue;
        }
        let rooted = part.starts_with("./");
        let part = part.strip_prefix("./").unwrap_or(part);
        let pattern = if rooted || part.starts_with("**/") {
            part.to_owned()
        } else {
            format!("**/{part}")
        };
        for pattern in [pattern.clone(), format!("{pattern}/**")] {
            builder.add(
                GlobBuilder::new(&pattern)
                    .literal_separator(true)
                    .build()
                    .map_err(|error| format!("Invalid file pattern: {error}"))?,
            );
        }
    }
    builder
        .build()
        .map_err(|error| format!("Invalid file pattern: {error}"))
}
