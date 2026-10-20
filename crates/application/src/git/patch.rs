//! Partial staging works from reviewed unified diffs, never frontend-created patches.
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PatchKind {
    /// File headers, commit metadata and other text outside hunks.
    Meta,
    Hunk,
    Context,
    Added,
    Removed,
}

/// One rendered diff line. `index` addresses the line in the reviewed text.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PatchLine {
    pub index: usize,
    pub text: String,
    pub changed: bool,
    pub kind: PatchKind,
    pub old: Option<usize>,
    pub new: Option<usize>,
}

fn hunk_start(header: &str, marker: char) -> Option<(usize, usize)> {
    let range = header
        .split_whitespace()
        .find_map(|part| part.strip_prefix(marker))?;
    let mut parts = range.split(',');
    let start = parts.next()?.parse().ok()?;
    let count = parts.next().map_or(Some(1), |c| c.parse().ok())?;
    Some((start, count))
}

/// Classify unified diff text, tracking hunk sizes so `--- ` content lines
/// inside a hunk are not mistaken for file headers.
pub(crate) fn lines(text: &str) -> Vec<PatchLine> {
    let (mut old, mut new) = (0, 0);
    let (mut old_left, mut new_left) = (0usize, 0usize);
    text.split_inclusive('\n')
        .enumerate()
        .map(|(index, line)| {
            let in_hunk = old_left > 0 || new_left > 0;
            let (kind, old_number, new_number) = if in_hunk && line.starts_with('+') {
                new_left -= 1;
                new += 1;
                (PatchKind::Added, None, Some(new - 1))
            } else if in_hunk && line.starts_with('-') {
                old_left -= 1;
                old += 1;
                (PatchKind::Removed, Some(old - 1), None)
            } else if in_hunk && (line.starts_with(' ') || line == "\n") {
                old_left = old_left.saturating_sub(1);
                new_left = new_left.saturating_sub(1);
                old += 1;
                new += 1;
                (PatchKind::Context, Some(old - 1), Some(new - 1))
            } else if let (true, Some((o, oc)), Some((n, nc))) = (
                line.starts_with("@@ "),
                hunk_start(line, '-'),
                hunk_start(line, '+'),
            ) {
                (old, new, old_left, new_left) = (o, n, oc, nc);
                (PatchKind::Hunk, None, None)
            } else {
                (PatchKind::Meta, None, None)
            };
            PatchLine {
                index,
                text: line.into(),
                changed: matches!(kind, PatchKind::Added | PatchKind::Removed),
                kind,
                old: old_number,
                new: new_number,
            }
        })
        .collect()
}

pub(crate) fn selected(
    diff: &crate::GitDiff,
    selected: &[usize],
) -> Result<String, crate::AppError> {
    let fail = || {
        crate::AppError::Git("Select changed lines from a current text diff. Use file staging for additions, deletions, renames or binary files.".into())
    };
    if diff.truncated
        || selected.is_empty()
        || diff.text.contains("/dev/null")
        || diff.text.matches("diff --git ").count() != 1
    {
        return Err(fail());
    }
    let all: Vec<_> = diff.text.split_inclusive('\n').collect();
    let before = all
        .iter()
        .find(|l| l.starts_with("--- "))
        .ok_or_else(fail)?;
    let after = all
        .iter()
        .find(|l| l.starts_with("+++ "))
        .ok_or_else(fail)?;
    let mut patch = if diff.staged {
        format!("--- {}+++ {}", &after[4..], &before[4..])
    } else {
        format!("{before}{after}")
    };
    let mut at = 0;
    let mut offset: i64 = 0;
    let mut changed = false;
    while at < all.len() {
        if !all[at].starts_with("@@ ") {
            at += 1;
            continue;
        }
        let range = all[at]
            .split_whitespace()
            .nth(if diff.staged { 2 } else { 1 })
            .ok_or_else(fail)?;
        let start: i64 = range[1..]
            .split(',')
            .next()
            .ok_or_else(fail)?
            .parse()
            .map_err(|_| fail())?;
        at += 1;
        let mut body = String::new();
        let mut old = 0;
        let mut new = 0;
        let mut used = false;
        while at < all.len() && !all[at].starts_with("@@ ") && !all[at].starts_with("diff --git ") {
            let line = all[at];
            let marker = line.as_bytes().first().copied().unwrap_or(b' ');
            let marker = if diff.staged {
                match marker {
                    b'+' => b'-',
                    b'-' => b'+',
                    other => other,
                }
            } else {
                marker
            };
            match marker {
                b' ' => {
                    body.push_str(line);
                    old += 1;
                    new += 1;
                }
                b'-' if selected.contains(&at) => {
                    body.push('-');
                    body.push_str(&line[1..]);
                    old += 1;
                    used = true;
                }
                b'-' => {
                    body.push(' ');
                    body.push_str(&line[1..]);
                    old += 1;
                    new += 1;
                }
                b'+' if selected.contains(&at) => {
                    body.push('+');
                    body.push_str(&line[1..]);
                    new += 1;
                    used = true;
                }
                b'+' => {}
                b'\\' => {
                    body.push_str(line);
                }
                _ => break,
            }
            at += 1;
        }
        if used {
            patch.push_str(&format!(
                "@@ -{start},{old} +{},{} @@\n{body}",
                start + offset,
                new
            ));
            offset += new - old;
            changed = true;
        }
    }
    if changed { Ok(patch) } else { Err(fail()) }
}
