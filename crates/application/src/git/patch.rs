//! Partial staging works from reviewed unified diffs, never frontend-created patches.
use serde::Serialize;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PatchLine {
    pub index: usize,
    pub text: String,
    pub changed: bool,
}
pub(crate) fn lines(text: &str) -> Vec<PatchLine> {
    let mut in_hunk = false;
    text.split_inclusive('\n')
        .enumerate()
        .filter_map(|(index, line)| {
            if line.starts_with("@@ ") {
                in_hunk = true;
            }
            in_hunk.then(|| PatchLine {
                index,
                text: line.into(),
                changed: line.starts_with('+') || line.starts_with('-'),
            })
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
