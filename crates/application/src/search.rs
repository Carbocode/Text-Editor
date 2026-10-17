//! Workspace content search. A query is debounced, then files are searched on
//! every core in priority order: open buffers, then shallow files before deep
//! ones. Matches stream to the UI as they are found, and a newer query or an
//! emptied field cancels the remaining work within one file per thread.
use crate::{
    AppEvent, DocumentEntry, SearchLocation, SearchOptions, SearchResult,
    diagnostics::{Context, Outcome},
    search_options::FilePatterns,
    worker::{Cancellation, DebouncedWorker},
};
use ignore::{Match, gitignore::Gitignore, gitignore::GitignoreBuilder};
use native_workspace_domain::{DocumentId, path};
use native_workspace_filesystem::WorkspaceFilesystem;
use regex::{Regex, RegexBuilder};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread,
    time::{Duration, Instant},
};

/// Streamed matches are published at most this often after the first batch.
const PUBLISH_INTERVAL: Duration = Duration::from_millis(100);
/// Frontends rebuild the whole result list on each publish, so larger lists
/// are published less often: 20,000 results wait a second between updates.
const PUBLISH_DELAY_PER_RESULT: Duration = Duration::from_micros(50);
/// Searching stops after this many matches; frontends say the list is partial.
pub(crate) const RESULT_LIMIT: usize = 20_000;

pub(crate) struct SearchJob {
    pub diagnostic: Context,
    pub root: PathBuf,
    pub documents: Vec<DocumentEntry>,
    pub query: String,
    pub options: SearchOptions,
    /// The query is new and its results start empty, so publish matches as
    /// they arrive. Otherwise current results stay until the search finishes.
    pub stream: bool,
}

struct SearchBatch {
    diagnostic: Context,
    generation: u64,
    results: Vec<SearchResult>,
    /// Replace every current result instead of appending.
    replace: bool,
    finished: bool,
    truncated: bool,
}

/// The search worker and its stream of result batches.
pub(crate) struct SearchService {
    worker: DebouncedWorker<SearchJob>,
    batches: Receiver<SearchBatch>,
}

impl SearchService {
    pub fn new(filesystem: Arc<dyn WorkspaceFilesystem>) -> Self {
        let (sender, batches) = mpsc::channel();
        let worker = DebouncedWorker::spawn(
            |superseded: SearchJob, mut newer: SearchJob| {
                superseded
                    .diagnostic
                    .event("search.superseded", "search-worker", 0, 0);
                // A refresh queued after typing must still stream.
                newer.stream |= superseded.stream;
                newer
            },
            move |job, cancellation| execute(&*filesystem, job, cancellation, &sender),
        );
        Self { worker, batches }
    }

    /// Searches `job` after the debounce, superseding earlier jobs.
    pub fn submit(&self, job: SearchJob) -> u64 {
        self.worker.submit(job)
    }

    /// Discards running and pending searches.
    pub fn cancel(&self) {
        self.worker.cancel();
    }

    pub fn generation(&self) -> u64 {
        self.worker.current()
    }

    /// Merges the current search's delivered batches into one event and
    /// returns the diagnostics context of the search that produced it.
    pub fn collect(&self, revision: u64) -> Option<(AppEvent, Context)> {
        let current = self.worker.current();
        let mut merged: Option<(AppEvent, Context)> = None;
        while let Ok(batch) = self.batches.try_recv() {
            if batch.generation != current {
                batch.diagnostic.event(
                    "search.stale_generation",
                    "main",
                    revision,
                    batch.generation,
                );
                continue;
            }
            batch.diagnostic.event(
                "search.delivered",
                "main",
                revision,
                batch.results.len() as u64,
            );
            match &mut merged {
                Some((
                    AppEvent::SearchResultsUpdated {
                        results,
                        replace,
                        finished,
                        truncated,
                    },
                    context,
                )) => {
                    if batch.replace {
                        *results = batch.results;
                        *replace = true;
                    } else {
                        results.extend(batch.results);
                    }
                    *finished |= batch.finished;
                    *truncated |= batch.truncated;
                    *context = batch.diagnostic;
                }
                _ => {
                    merged = Some((
                        AppEvent::SearchResultsUpdated {
                            results: batch.results,
                            replace: batch.replace,
                            finished: batch.finished,
                            truncated: batch.truncated,
                        },
                        batch.diagnostic,
                    ));
                }
            }
        }
        merged
    }
}

fn execute(
    filesystem: &dyn WorkspaceFilesystem,
    job: SearchJob,
    cancellation: &Cancellation,
    batches: &Sender<SearchBatch>,
) {
    let generation = cancellation.generation();
    let span = job.diagnostic.span("search.execute", "search-worker");
    let mut found = Vec::new();
    let mut published = 0u64;
    let truncated = run_search(filesystem, &job, &|| cancellation.is_cancelled(), |batch| {
        published += batch.len() as u64;
        if job.stream {
            let _ = batches.send(SearchBatch {
                diagnostic: job.diagnostic.clone(),
                generation,
                results: batch,
                replace: false,
                finished: false,
                truncated: false,
            });
        } else {
            found.extend(batch);
        }
    });
    if cancellation.is_cancelled() {
        span.end(Outcome::Cancelled, 0, published);
        job.diagnostic
            .event("search.superseded", "search-worker", 0, 0);
        return;
    }
    span.end(Outcome::Ok, 0, published);
    job.diagnostic
        .event("search.published", "search-worker", 0, published);
    let _ = batches.send(SearchBatch {
        diagnostic: job.diagnostic,
        generation,
        results: found,
        replace: !job.stream,
        finished: true,
        truncated,
    });
}

/// Case-insensitive matching with Rust's lowercase semantics. A regex rules
/// out files and lines quickly; the exact matcher computes positions.
pub(crate) struct Matcher {
    needle: String,
    prefilter: Option<Regex>,
    exact: Option<Regex>,
    whole_word: bool,
    regex: bool,
}

impl Matcher {
    pub fn new(query: &str) -> Self {
        let needle = query.trim().to_lowercase();
        // For an ASCII needle, only ASCII letters, KELVIN SIGN (a case variant
        // of `k` in the regex too) and `İ` (lowercase "i̇") lowercase into
        // ASCII. Allowing `İ` wherever `i` occurs keeps the regex a superset.
        let prefilter = (!needle.is_empty()
            && needle.is_ascii()
            && !needle.chars().any(|ch| ch.is_ascii_control()))
        .then(|| {
            let pattern: String = needle
                .chars()
                .map(|ch| match ch {
                    'i' => "(?:i|\u{130})".to_owned(),
                    ch => regex::escape(ch.encode_utf8(&mut [0; 4])),
                })
                .collect();
            RegexBuilder::new(&pattern)
                .case_insensitive(true)
                .build()
                .ok()
        })
        .flatten();
        Self {
            needle,
            prefilter,
            exact: None,
            whole_word: false,
            regex: false,
        }
    }

    pub(crate) fn configured(query: &str, options: &SearchOptions) -> Result<Self, String> {
        let mut matcher = Self::new(query);
        matcher.whole_word = options.whole_word;
        matcher.regex = options.regex;
        if options.regex || options.match_case {
            let pattern = if options.regex {
                query.trim().to_owned()
            } else {
                regex::escape(query.trim())
            };
            matcher.exact = Some(
                RegexBuilder::new(&pattern)
                    .case_insensitive(!options.match_case)
                    .multi_line(true)
                    .build()
                    .map_err(|error| format!("Invalid regular expression: {error}"))?,
            );
        }
        Ok(matcher)
    }

    /// Replacement uses precisely the same Unicode ranges as search.
    pub(crate) fn replacement(
        &self,
        content: &str,
        start: usize,
        end: usize,
        template: &str,
    ) -> String {
        if self.regex
            && let Some(expression) = &self.exact
            && let Some(captures) = expression.captures_at(content, start)
            && captures
                .get(0)
                .is_some_and(|found| found.start() == start && found.end() == end)
        {
            let mut expanded = String::new();
            captures.expand(template, &mut expanded);
            return expanded;
        }
        template.to_owned()
    }

    pub fn needle(&self) -> &str {
        &self.needle
    }

    fn may_match(&self, text: &str) -> bool {
        self.prefilter
            .as_ref()
            .is_none_or(|prefilter| prefilter.is_match(text))
    }

    /// Every occurrence in `content`, including overlapping ones.
    pub fn content_results(&self, id: &DocumentId, content: &str, results: &mut Vec<SearchResult>) {
        let start = results.len();
        self.unfiltered_results(id, content, results);
        if self.whole_word {
            let chars: Vec<_> = content.chars().collect();
            let mut index = 0;
            results.retain(|result| {
                let existing = index < start;
                index += 1;
                existing
                    || (result
                        .location
                        .start_char
                        .checked_sub(1)
                        .and_then(|offset| chars.get(offset as usize))
                        .is_none_or(|ch| !word_char(*ch))
                        && chars
                            .get(result.location.end_char as usize)
                            .is_none_or(|ch| !word_char(*ch)))
            });
        }
    }

    fn unfiltered_results(&self, id: &DocumentId, content: &str, results: &mut Vec<SearchResult>) {
        if let Some(expression) = &self.exact {
            if self.needle.is_empty() {
                return;
            }
            let mut positions = vec![(0usize, 0usize, 0usize, 1u64, 1u64)];
            let (mut scalar, mut utf16, mut line, mut column) = (0usize, 0usize, 1u64, 1u64);
            for (byte, ch) in content.char_indices() {
                scalar += 1;
                utf16 += ch.len_utf16();
                if ch == '\n' {
                    line += 1;
                    column = 1;
                } else {
                    column += 1;
                }
                positions.push((byte + ch.len_utf8(), scalar, utf16, line, column));
            }
            let line_starts: Vec<_> = std::iter::once(0)
                .chain(content.match_indices('\n').map(|(byte, _)| byte + 1))
                .collect();
            let mut preview: Option<(usize, LinePreview<'_>)> = None;
            let mut regular = expression.find_iter(content);
            let mut from = 0;
            let matches = std::iter::from_fn(|| {
                if self.regex {
                    return regular.next();
                }
                let found = expression.find_at(content, from)?;
                from = found.start() + content[found.start()..].chars().next()?.len_utf8();
                Some(found)
            });
            for found in matches {
                let start = positions[positions
                    .binary_search_by_key(&found.start(), |p| p.0)
                    .unwrap()];
                let end = positions[positions
                    .binary_search_by_key(&found.end(), |p| p.0)
                    .unwrap()];
                let line_index = line_starts.partition_point(|&byte| byte <= found.start()) - 1;
                let line_start = line_starts[line_index];
                let line_end = line_starts
                    .get(line_index + 1)
                    .map_or(content.len(), |byte| byte - 1);
                if preview
                    .as_ref()
                    .is_none_or(|(index, _)| *index != line_index)
                {
                    preview = Some((line_index, LinePreview::new(&content[line_start..line_end])));
                }
                results.push(SearchResult {
                    id: id.clone(),
                    snippet: preview.as_ref().unwrap().1.snippet(start.4 as usize - 1),
                    location: SearchLocation {
                        line: start.3,
                        column: start.4,
                        start_char: start.1 as u64,
                        end_char: end.1 as u64,
                        start_utf16: start.2 as u64,
                        end_utf16: end.2 as u64,
                    },
                });
            }
            return;
        }
        let needle = self.needle.as_str();
        if needle.is_empty() || !self.may_match(content) {
            return;
        }
        let mut line_start_char = 0usize;
        let mut line_start_utf16 = 0usize;
        let mut push = |line_index: usize,
                        preview: &LinePreview,
                        column: usize,
                        start: (usize, usize),
                        end: (usize, usize)| {
            results.push(SearchResult {
                id: id.clone(),
                snippet: preview.snippet(column - 1),
                location: SearchLocation {
                    line: (line_index + 1) as u64,
                    column: column as u64,
                    start_char: start.0 as u64,
                    end_char: end.0 as u64,
                    start_utf16: start.1 as u64,
                    end_utf16: end.1 as u64,
                },
            });
        };
        for (line_index, raw_line) in content.split_inclusive('\n').enumerate() {
            let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
            let ascii = raw_line.is_ascii();
            if let Some(prefilter) = &self.prefilter
                && ascii
            {
                // Byte, character and UTF-16 offsets coincide on ASCII lines.
                let mut from = 0;
                let mut preview = None;
                while let Some(found) = prefilter.find_at(line, from) {
                    let (start, end) = (found.start(), found.end());
                    push(
                        line_index,
                        preview.get_or_insert_with(|| LinePreview::new(line)),
                        start + 1,
                        (line_start_char + start, line_start_utf16 + start),
                        (line_start_char + end, line_start_utf16 + end),
                    );
                    from = start + 1;
                }
            } else if self.may_match(line) {
                let mut folded = String::new();
                let mut positions = Vec::new();
                let mut char_offset = line_start_char;
                let mut utf16_offset = line_start_utf16;
                for ch in line.chars() {
                    let next_char = char_offset + 1;
                    let next_utf16 = utf16_offset + ch.len_utf16();
                    for lower in ch.to_lowercase() {
                        positions.push((
                            folded.len(),
                            char_offset,
                            next_char,
                            utf16_offset,
                            next_utf16,
                        ));
                        folded.push(lower);
                    }
                    char_offset = next_char;
                    utf16_offset = next_utf16;
                }
                let mut search_from = 0;
                let mut preview = None;
                while search_from < folded.len() {
                    let Some(relative) = folded[search_from..].find(needle) else {
                        break;
                    };
                    let start_byte = search_from + relative;
                    let start_index = positions
                        .binary_search_by_key(&start_byte, |position| position.0)
                        .expect("match starts at a character boundary");
                    let end_byte = start_byte + needle.len();
                    let end_index = positions
                        .binary_search_by_key(&end_byte, |position| position.0)
                        .unwrap_or_else(|index| index);
                    let (_, start_char, _, start_utf16, _) = positions[start_index];
                    let (_, _, end_char, _, end_utf16) = positions[end_index - 1];
                    push(
                        line_index,
                        preview.get_or_insert_with(|| LinePreview::new(line)),
                        start_char - line_start_char + 1,
                        (start_char, start_utf16),
                        (end_char, end_utf16),
                    );
                    search_from =
                        start_byte + folded[start_byte..].chars().next().unwrap().len_utf8();
                }
            }
            if ascii {
                line_start_char += raw_line.len();
                line_start_utf16 += raw_line.len();
            } else {
                line_start_char += raw_line.chars().count();
                line_start_utf16 += raw_line.encode_utf16().count();
            }
        }
    }
}

fn word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// Snippets around the matches of one line. Built once per matching line, so
/// a long minified line with many matches costs linear rather than quadratic time.
struct LinePreview<'a> {
    trimmed: &'a str,
    leading: usize,
    /// Byte offset of each character of `trimmed`; empty when it is ASCII.
    offsets: Vec<usize>,
}

impl<'a> LinePreview<'a> {
    fn new(line: &'a str) -> Self {
        let leading = line[..line.len() - line.trim_start().len()].chars().count();
        let trimmed = line.trim();
        let offsets = if trimmed.is_ascii() {
            Vec::new()
        } else {
            trimmed.char_indices().map(|(offset, _)| offset).collect()
        };
        Self {
            trimmed,
            leading,
            offsets,
        }
    }

    fn byte(&self, character: usize) -> usize {
        if self.offsets.is_empty() {
            character.min(self.trimmed.len())
        } else {
            self.offsets
                .get(character)
                .copied()
                .unwrap_or(self.trimmed.len())
        }
    }

    /// Up to 160 characters starting 40 before the match, in line characters.
    fn snippet(&self, match_start: usize) -> String {
        let count = if self.offsets.is_empty() {
            self.trimmed.len()
        } else {
            self.offsets.len()
        };
        let start = match_start
            .saturating_sub(self.leading)
            .saturating_sub(40)
            .min(count);
        let end = (start + 160).min(count);
        format!(
            "{}{}{}",
            if start > 0 { "…" } else { "" },
            &self.trimmed[self.byte(start)..self.byte(end)],
            if end < count { "…" } else { "" }
        )
    }
}

/// `.gitignore` rules found in the workspace listing. Like ripgrep and VS
/// Code, a search skips ignored files and `.git`; the tree still lists them.
struct IgnoreRules {
    by_directory: HashMap<String, Gitignore>,
}

impl IgnoreRules {
    fn load(root: &Path, documents: &[DocumentEntry]) -> Self {
        let mut builders: HashMap<String, GitignoreBuilder> = HashMap::new();
        for entry in documents {
            if entry.id().is_external() {
                continue;
            }
            let path = entry.id().as_str();
            let (directory, name) = (path::parent(path), path::name(path));
            if name == ".gitignore" && !in_git_directory(path) {
                builders
                    .entry(directory.to_owned())
                    .or_insert_with(|| GitignoreBuilder::new(root.join(directory)))
                    .add(root.join(path));
            }
        }
        let exclude = root.join(".git/info/exclude");
        if exclude.is_file() {
            builders
                .entry(String::new())
                .or_insert_with(|| GitignoreBuilder::new(root))
                .add(exclude);
        }
        Self {
            by_directory: builders
                .into_iter()
                .filter_map(|(directory, builder)| Some((directory, builder.build().ok()?)))
                .collect(),
        }
    }

    fn ignored(&self, path: &str) -> bool {
        if in_git_directory(path) {
            return true;
        }
        // The nearest `.gitignore` decides, as in Git.
        let mut directory = path::parent(path);
        loop {
            if let Some(rules) = self.by_directory.get(directory) {
                let relative = if directory.is_empty() {
                    path
                } else {
                    &path[directory.len() + 1..]
                };
                match rules.matched_path_or_any_parents(relative, false) {
                    Match::Ignore(_) => return true,
                    Match::Whitelist(_) => return false,
                    Match::None => {}
                }
            }
            if directory.is_empty() {
                return false;
            }
            directory = path::parent(directory);
        }
    }
}

fn in_git_directory(path: &str) -> bool {
    path.starts_with(".git/") || path.contains("/.git/")
}

/// Indices of the documents to search, most relevant first: open buffers,
/// then by depth so root files come before nested ones, then by path.
/// Ignored files are skipped unless open.
#[cfg(test)]
fn search_order(root: &Path, documents: &[DocumentEntry]) -> Vec<usize> {
    search_order_with_options(root, documents, &SearchOptions::default())
}

fn search_order_with_options(
    root: &Path,
    documents: &[DocumentEntry],
    options: &SearchOptions,
) -> Vec<usize> {
    let rules = IgnoreRules::load(root, documents);
    let Ok(patterns) = FilePatterns::new(options) else {
        return Vec::new();
    };
    let mut order: Vec<usize> = (0..documents.len())
        .filter(|&index| {
            let entry = &documents[index];
            (!entry.id().is_external() || options.only_open_editors)
                && (!options.only_open_editors || entry.is_loaded())
                && patterns.matches(entry.id().as_str())
                && (!options.use_ignore_files
                    || entry.is_loaded()
                    || !rules.ignored(entry.id().as_str()))
        })
        .collect();
    order.sort_by_key(|&index| {
        let entry = &documents[index];
        (!entry.is_loaded(), path::depth(entry.id().as_str()))
    });
    order
}

/// Searches `documents` and hands matches to `publish` in batches: the first
/// as soon as it exists, then at most every `PUBLISH_INTERVAL`, and the rest
/// at the end. Returns whether `RESULT_LIMIT` cut the search short.
fn run_search(
    filesystem: &dyn WorkspaceFilesystem,
    job: &SearchJob,
    cancelled: &(dyn Fn() -> bool + Sync),
    mut publish: impl FnMut(Vec<SearchResult>),
) -> bool {
    let SearchJob {
        root,
        documents,
        query,
        options,
        ..
    } = job;
    let Ok(matcher) = Matcher::configured(query, options) else {
        return false;
    };
    if matcher.needle().is_empty() {
        return false;
    }
    let order = search_order_with_options(root, documents, options);
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let workers = thread::available_parallelism().map_or(4, |count| count.get().min(8));
    let (sender, receiver) = mpsc::channel::<Vec<SearchResult>>();
    let mut truncated = false;
    thread::scope(|scope| {
        for _ in 0..workers {
            let sender = sender.clone();
            let (order, next, stop, matcher) = (&order, &next, &stop, &matcher);
            scope.spawn(move || {
                let mut verified = HashSet::new();
                loop {
                    if stop.load(Ordering::Relaxed) || cancelled() {
                        return;
                    }
                    let Some(&index) = order.get(next.fetch_add(1, Ordering::Relaxed)) else {
                        return;
                    };
                    let entry = &documents[index];
                    let disk;
                    let content = match entry.content() {
                        Some(content) => &**content,
                        None => {
                            disk = filesystem.read_document_for_search(
                                root,
                                entry.id(),
                                &mut verified,
                            );
                            match &disk {
                                Some(content) => content.as_str(),
                                None => continue,
                            }
                        }
                    };
                    let mut found = Vec::new();
                    matcher.content_results(entry.id(), content, &mut found);
                    if !found.is_empty() && sender.send(found).is_err() {
                        return;
                    }
                }
            });
        }
        drop(sender);
        let mut pending = Vec::new();
        let mut count = 0;
        let mut published_at: Option<Instant> = None;
        loop {
            let finished = match receiver.recv_timeout(PUBLISH_INTERVAL) {
                Ok(mut found) => {
                    if count + found.len() >= RESULT_LIMIT {
                        found.truncate(RESULT_LIMIT - count);
                        truncated = true;
                        stop.store(true, Ordering::Relaxed);
                    }
                    count += found.len();
                    pending.append(&mut found);
                    truncated
                }
                Err(RecvTimeoutError::Timeout) => false,
                Err(RecvTimeoutError::Disconnected) => true,
            };
            if finished || cancelled() {
                break;
            }
            let interval = PUBLISH_INTERVAL.max(PUBLISH_DELAY_PER_RESULT * count as u32);
            if !pending.is_empty() && published_at.is_none_or(|at| at.elapsed() >= interval) {
                publish(std::mem::take(&mut pending));
                published_at = Some(Instant::now());
            }
        }
        if !pending.is_empty() && !cancelled() {
            publish(pending);
        }
    });
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn entry(path: &str, content: Option<&str>) -> DocumentEntry {
        let mut entry = DocumentEntry::new(DocumentId::parse(path).unwrap());
        if let Some(content) = content {
            entry.load(content.into());
        }
        entry
    }

    fn matches(query: &str, content: &str) -> Vec<(u64, u64, u64)> {
        let mut results = Vec::new();
        Matcher::new(query).content_results(
            &DocumentId::parse("a.txt").unwrap(),
            content,
            &mut results,
        );
        results
            .iter()
            .map(|result| {
                let location = &result.location;
                (location.line, location.start_char, location.start_utf16)
            })
            .collect()
    }

    #[test]
    fn matching_keeps_literal_overlapping_and_unicode_lowercase_semantics() {
        assert_eq!(matches("a.b", "axb a.b"), [(1, 4, 4)]);
        assert_eq!(matches("aa", "aaa"), [(1, 0, 0), (1, 1, 1)]);
        // KELVIN SIGN lowercases to `k`; `İ` lowercases to "i̇".
        assert_eq!(matches("k", "\u{212A}"), [(1, 0, 0)]);
        assert_eq!(matches("i", "x\u{130}"), [(1, 1, 1)]);
        assert!(matches("ix", "\u{130}x").is_empty());
        // Offsets after non-ASCII lines count characters and UTF-16 units.
        assert_eq!(matches("café", "💡\nCAFÉ"), [(2, 2, 3)]);
        assert_eq!(matches("needle", "💡 x\nNEEDLE"), [(2, 4, 5)]);
    }

    #[test]
    fn order_starts_with_open_buffers_and_shallow_files_and_skips_ignored() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), "target/\n*.log\n").unwrap();
        fs::create_dir_all(dir.path().join("web")).unwrap();
        fs::write(dir.path().join("web/.gitignore"), "dist\n!keep.log\n").unwrap();
        let documents = [
            entry(".git/config", None),
            entry(".gitignore", None),
            entry("a/b/deep.rs", None),
            entry("a/mid.rs", None),
            entry("debug.log", None),
            entry("root.rs", None),
            entry("target/out.rs", None),
            entry("target/open.rs", Some("open")),
            entry("web/.gitignore", None),
            entry("web/dist/bundle.js", None),
            entry("web/keep.log", None),
        ];
        let order: Vec<&str> = search_order(dir.path(), &documents)
            .into_iter()
            .map(|index| documents[index].id().as_str())
            .collect();
        assert_eq!(
            order,
            [
                "target/open.rs",
                ".gitignore",
                "root.rs",
                "a/mid.rs",
                "web/.gitignore",
                "web/keep.log",
                "a/b/deep.rs",
            ]
        );
    }
}
