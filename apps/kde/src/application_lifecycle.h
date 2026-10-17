#pragma once

#include <QEvent>
#include <QGuiApplication>
#include <QObject>

// Native quit delivery and confirmation presentation stay in the adapter.
class ApplicationLifecycle : public QObject {
    Q_OBJECT
    bool approved = false;
    bool watching = false;
public:
    using QObject::QObject;
    Q_INVOKABLE void watchQuitRequests() { watching = true; }
    Q_INVOKABLE void finishQuit(int code = 0) {
        approved = true;
        QCoreApplication::exit(code);
    }
    bool eventFilter(QObject*, QEvent* event) override {
        if (event->type() != QEvent::Quit || approved || !watching) return false;
        emit quitRequested();
        return true;
    }
signals:
    void quitRequested();
};
