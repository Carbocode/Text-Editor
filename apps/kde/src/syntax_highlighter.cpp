#include "syntax_highlighter.h"

#include <QSyntaxHighlighter>
#include <QGuiApplication>
#include <QPalette>
#include <QTextBlock>
#include <QTextCharFormat>
#include <QTextDocument>
#include <QtQml/qqml.h>

class CodeHighlighter final : public QSyntaxHighlighter {
public:
    explicit CodeHighlighter(QTextDocument* document) : QSyntaxHighlighter(document) {}

    void setSpans(const QVariantList& spans) {
        spans_.clear();
        for (const QVariant& value : spans) {
            const auto span = value.toMap();
            spans_.append({span.value("start").toInt(), span.value("end").toInt(), span.value("kind").toInt()});
        }
        rehighlight();
    }

    void setColors(const QList<QColor>& colors) {
        colors_ = colors;
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
            format.setForeground(span.kind >= 0 && span.kind < colors_.size()
                ? colors_[span.kind] : QGuiApplication::palette().color(QPalette::Text));
            setFormat(from - blockStart, to - from, format);
        }
    }

private:
    struct Span { int start; int end; int kind; };
    QList<Span> spans_;
    QList<QColor> colors_;
};

EditorSyntaxHighlighter::EditorSyntaxHighlighter(QObject* parent) : QObject(parent) {}

void EditorSyntaxHighlighter::setTextDocument(QQuickTextDocument* document) {
    if (document_ == document) return;
    delete highlighter_;
    document_ = document;
    highlighter_ = document ? new CodeHighlighter(document->textDocument()) : nullptr;
    if (highlighter_) {
        static_cast<CodeHighlighter*>(highlighter_)->setColors(colors_);
        static_cast<CodeHighlighter*>(highlighter_)->setSpans(spans_);
    }
}

void EditorSyntaxHighlighter::clearUndoHistory() {
    if (document_ && document_->textDocument()) document_->textDocument()->clearUndoRedoStacks();
}

void EditorSyntaxHighlighter::setSpans(const QVariantList& spans) {
    if (spans_ == spans) return;
    spans_ = spans;
    if (highlighter_) static_cast<CodeHighlighter*>(highlighter_)->setSpans(spans_);
}

void EditorSyntaxHighlighter::setColors(const QList<QColor>& colors) {
    if (colors_ == colors) return;
    colors_ = colors;
    if (highlighter_) static_cast<CodeHighlighter*>(highlighter_)->setColors(colors_);
}

extern "C" void register_editor_syntax_highlighter() {
    qmlRegisterType<EditorSyntaxHighlighter>("org.nativeworkspace.Syntax", 1, 0, "EditorSyntaxHighlighter");
}
