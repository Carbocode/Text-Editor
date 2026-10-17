#pragma once

#include <QObject>
#include <QColor>
#include <QPointer>
#include <QQuickTextDocument>
#include <QVariantList>

class QSyntaxHighlighter;

class EditorSyntaxHighlighter : public QObject {
    Q_OBJECT
    Q_PROPERTY(QQuickTextDocument* textDocument READ textDocument WRITE setTextDocument)
    Q_PROPERTY(QVariantList spans READ spans WRITE setSpans)
    Q_PROPERTY(QList<QColor> colors READ colors WRITE setColors)

public:
    explicit EditorSyntaxHighlighter(QObject* parent = nullptr);
    QQuickTextDocument* textDocument() const { return document_; }
    void setTextDocument(QQuickTextDocument* document);
    QVariantList spans() const { return spans_; }
    void setSpans(const QVariantList& spans);
    QList<QColor> colors() const { return colors_; }
    void setColors(const QList<QColor>& colors);
    Q_INVOKABLE void clearUndoHistory();

private:
    QPointer<QQuickTextDocument> document_;
    QSyntaxHighlighter* highlighter_ = nullptr;
    QVariantList spans_;
    QList<QColor> colors_;
};

extern "C" void register_editor_syntax_highlighter();
