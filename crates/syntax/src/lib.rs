//! Shared syntax token ranges for the native editors.

use regex::Regex;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TokenKind {
    Keyword,
    Number,
    String,
    Comment,
    Heading,
}

impl TokenKind {
    pub const fn code(self) -> u8 {
        match self {
            Self::Keyword => 1,
            Self::Number => 2,
            Self::String => 3,
            Self::Comment => 4,
            Self::Heading => 5,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Token {
    pub start_char: usize,
    pub end_char: usize,
    pub start_utf16: usize,
    pub end_utf16: usize,
    pub kind: TokenKind,
}

static CODE_RULES: LazyLock<Vec<(Regex, TokenKind)>> = LazyLock::new(|| {
    [
        (r"\b(?:fn|func|function|def|class|struct|enum|interface|protocol|trait|impl|import|from|use|let|var|const|mut|pub|private|public|static|return|if|else|for|while|loop|match|switch|case|break|continue|async|await|try|catch|throw|throws|new|self|Self|this|super|true|false|null|nil|None|Some|Ok|Err|type|extends|export|default|package|mod|in|as)\b", TokenKind::Keyword),
        (r"\b\d+(?:\.\d+)?\b", TokenKind::Number),
        (r#""(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`"#, TokenKind::String),
        (r"(?m)//[^\n]*|#[^\n]*|/\*[\s\S]*?\*/|<!--[\s\S]*?-->", TokenKind::Comment),
    ]
    .into_iter()
    .map(|(pattern, kind)| (Regex::new(pattern).expect("syntax rule"), kind))
    .collect()
});

static MARKDOWN_RULES: LazyLock<Vec<(Regex, TokenKind)>> = LazyLock::new(|| {
    [
        (r"(?m)^#{1,6}\s.*$", TokenKind::Heading),
        (r"(?m)^\s*>.*$", TokenKind::Comment),
        (r"`[^`\n]+`|\*\*[^*\n]+\*\*", TokenKind::String),
        (r"\[[^\]\n]+\]\([^\)\n]+\)", TokenKind::Keyword),
    ]
    .into_iter()
    .map(|(pattern, kind)| (Regex::new(pattern).expect("markdown rule"), kind))
    .collect()
});

pub fn highlight(file_name: &str, text: &str) -> Vec<Token> {
    let extension = std::path::Path::new(file_name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let rules = if matches!(extension.as_str(), "md" | "markdown") {
        &*MARKDOWN_RULES
    } else if matches!(
        extension.as_str(),
        "rs" | "swift"
            | "js"
            | "jsx"
            | "ts"
            | "tsx"
            | "py"
            | "go"
            | "java"
            | "c"
            | "cc"
            | "cpp"
            | "h"
            | "hpp"
            | "cs"
            | "kt"
            | "rb"
            | "sh"
            | "qml"
            | "json"
            | "yaml"
            | "yml"
            | "toml"
            | "css"
            | "html"
            | "xml"
    ) {
        &*CODE_RULES
    } else {
        return Vec::new();
    };

    let mut offsets = Vec::with_capacity(text.len() + 1);
    let mut utf16 = 0;
    for (byte, character) in text.char_indices() {
        offsets.push((byte, offsets.len(), utf16));
        utf16 += character.len_utf16();
    }
    offsets.push((text.len(), offsets.len(), utf16));
    let position = |byte: usize| {
        let index = offsets
            .binary_search_by_key(&byte, |entry| entry.0)
            .unwrap();
        (offsets[index].1, offsets[index].2)
    };
    let mut tokens = Vec::new();
    for (regex, kind) in rules {
        for found in regex.find_iter(text) {
            let (start_char, start_utf16) = position(found.start());
            let (end_char, end_utf16) = position(found.end());
            tokens.push(Token {
                start_char,
                end_char,
                start_utf16,
                end_utf16,
                kind: *kind,
            });
        }
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_account_for_unicode_and_document_type() {
        let tokens = highlight("main.rs", "🦀 let value = 42; // note");
        let keyword = tokens
            .iter()
            .find(|token| token.kind == TokenKind::Keyword)
            .unwrap();
        assert_eq!((keyword.start_char, keyword.start_utf16), (2, 3));
        assert!(tokens.iter().any(|token| token.kind == TokenKind::Number));
        assert!(tokens.iter().any(|token| token.kind == TokenKind::Comment));
        assert!(highlight("notes.txt", "let value = 42").is_empty());
        assert!(
            highlight("README.md", "# Title\n[link](page)")
                .iter()
                .any(|token| token.kind == TokenKind::Heading)
        );
    }
}
