#pragma once

#include <QObject>
#include <QPointer>
#include <QQuickTextDocument>
#include <QStringList>

class QSyntaxHighlighter;

class EditorSyntaxHighlighter : public QObject {
    Q_OBJECT
    Q_PROPERTY(QQuickTextDocument* textDocument READ textDocument WRITE setTextDocument)
    Q_PROPERTY(QStringList spans READ spans WRITE setSpans)

public:
    explicit EditorSyntaxHighlighter(QObject* parent = nullptr);
    QQuickTextDocument* textDocument() const { return document_; }
    void setTextDocument(QQuickTextDocument* document);
    QStringList spans() const { return spans_; }
    void setSpans(const QStringList& spans);

private:
    QPointer<QQuickTextDocument> document_;
    QSyntaxHighlighter* highlighter_ = nullptr;
    QStringList spans_;
};

extern "C" void register_editor_syntax_highlighter();
