#pragma once

#include <QFileOpenEvent>
#include <QGuiApplication>
#include <QFileInfo>
#include <QObject>
#include <QStringList>
#include <QUrl>

// Native delivery only. Rust decides whether each path is a file or workspace.
class SystemOpenRequests : public QObject {
    Q_OBJECT
    QStringList pending;
public:
    explicit SystemOpenRequests(QObject* parent) : QObject(parent) {
        bool positional = false;
        const auto arguments = QGuiApplication::arguments();
        for (int i = 1; i < arguments.size(); ++i) {
            const auto& argument = arguments[i];
            if (!positional && argument == QStringLiteral("--")) { positional = true; continue; }
            if (!positional && argument.startsWith(QLatin1Char('-'))) continue;
            const auto url = QUrl(argument);
            pending.append(url.scheme().isEmpty()
                ? QUrl::fromLocalFile(QFileInfo(argument).absoluteFilePath()).toString()
                : argument);
        }
    }
    Q_INVOKABLE QStringList takePending() {
        QStringList result;
        result.swap(pending);
        return result;
    }
    Q_INVOKABLE bool hasPending() const { return !pending.isEmpty(); }
    bool eventFilter(QObject*, QEvent* event) override {
        if (event->type() != QEvent::FileOpen) return false;
        pending.append(static_cast<QFileOpenEvent*>(event)->url().toString());
        emit received();
        return true;
    }
signals:
    void received();
};
