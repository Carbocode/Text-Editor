#include "syntax_highlighter.h"

#include <QSyntaxHighlighter>
#include <QTextBlock>
#include <QTextCharFormat>
#include <QTextDocument>
#include <QtQml/qqml.h>

class CodeHighlighter final : public QSyntaxHighlighter {
public:
    explicit CodeHighlighter(QTextDocument* document) : QSyntaxHighlighter(document) {}

    void setSpans(const QStringList& spans) {
        spans_.clear();
        for (const QString& encoded : spans) {
            const auto parts = encoded.split(':');
            if (parts.size() != 3) continue;
            spans_.append({parts[0].toInt(), parts[1].toInt(), parts[2].toInt()});
        }
        rehighlight();
    }

protected:
    void highlightBlock(const QString&) override {
        const int blockStart = currentBlock().position();
        const int blockEnd = blockStart + currentBlock().length();
        for (const auto& span : spans_) {
            const int from = qMax(span.start, blockStart);
            const int to = qMin(span.end, blockEnd);
            if (from >= to) continue;
            QTextCharFormat format;
            switch (span.kind) {
            case 1: format.setForeground(QColor("#9141ac")); break;
            case 2: format.setForeground(QColor("#c64600")); break;
            case 3: format.setForeground(QColor("#2e9f55")); break;
            case 4: format.setForeground(QColor("#808080")); break;
            default: format.setForeground(QColor("#3584e4")); break;
            }
            setFormat(from - blockStart, to - from, format);
        }
    }

private:
    struct Span { int start; int end; int kind; };
    QList<Span> spans_;
};

EditorSyntaxHighlighter::EditorSyntaxHighlighter(QObject* parent) : QObject(parent) {}

void EditorSyntaxHighlighter::setTextDocument(QQuickTextDocument* document) {
    if (document_ == document) return;
    delete highlighter_;
    document_ = document;
    highlighter_ = document ? new CodeHighlighter(document->textDocument()) : nullptr;
    if (highlighter_) static_cast<CodeHighlighter*>(highlighter_)->setSpans(spans_);
}

void EditorSyntaxHighlighter::setSpans(const QStringList& spans) {
    if (spans_ == spans) return;
    spans_ = spans;
    if (highlighter_) static_cast<CodeHighlighter*>(highlighter_)->setSpans(spans_);
}

extern "C" void register_editor_syntax_highlighter() {
    qmlRegisterType<EditorSyntaxHighlighter>("org.nativeworkspace.Syntax", 1, 0, "EditorSyntaxHighlighter");
}
