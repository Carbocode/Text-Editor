#pragma once

#include <QCursor>
#include <QGuiApplication>
#include <QObject>
#include <QQuickItem>
#include <QWindow>

class CursorProbe : public QObject {
    Q_OBJECT
public:
    using QObject::QObject;
    Q_INVOKABLE int shape(QWindow* window) const {
        const auto override = QGuiApplication::overrideCursor();
        return override ? override->shape() : window->cursor().shape();
    }
    Q_INVOKABLE QString describe(QWindow* window, QQuickItem* item) const {
        const auto override = QGuiApplication::overrideCursor();
        return QStringLiteral("window=%1 override=%2 item=%3 cursor=%4 visible=%5 enabled=%6")
            .arg(window->cursor().shape()).arg(override ? override->shape() : -1)
            .arg(QString::fromLatin1(item->metaObject()->className())).arg(item->cursor().shape())
            .arg(item->isVisible()).arg(item->isEnabled());
    }
};
