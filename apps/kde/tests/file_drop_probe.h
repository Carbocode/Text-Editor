#pragma once

#include <QCoreApplication>
#include <QDragEnterEvent>
#include <QDragLeaveEvent>
#include <QDragMoveEvent>
#include <QDropEvent>
#include <QMimeData>
#include <QObject>
#include <QStringList>
#include <QUrl>
#include <QWindow>
#include <memory>

// Sends native URI-list events through QQuickWindow, including DropArea hit testing.
class FileDropProbe : public QObject {
    Q_OBJECT
    std::unique_ptr<QMimeData> data;
public:
    using QObject::QObject;
    Q_INVOKABLE bool enter(QWindow* window, double x, double y, const QStringList& paths) {
        data = std::make_unique<QMimeData>();
        QList<QUrl> urls;
        for (const auto& path : paths) urls.append(QUrl::fromLocalFile(path));
        data->setUrls(urls);
        QDragEnterEvent event(QPoint(x, y), Qt::CopyAction | Qt::MoveAction,
            data.get(), Qt::LeftButton, Qt::NoModifier);
        QCoreApplication::sendEvent(window, &event);
        return event.isAccepted();
    }
    Q_INVOKABLE bool move(QWindow* window, double x, double y) {
        QDragMoveEvent event(QPoint(x, y), Qt::CopyAction | Qt::MoveAction,
            data.get(), Qt::LeftButton, Qt::NoModifier);
        QCoreApplication::sendEvent(window, &event);
        return event.isAccepted();
    }
    Q_INVOKABLE bool drop(QWindow* window, double x, double y) {
        QDropEvent event(QPointF(x, y), Qt::CopyAction | Qt::MoveAction,
            data.get(), Qt::LeftButton, Qt::NoModifier);
        QCoreApplication::sendEvent(window, &event);
        data.reset();
        return event.isAccepted();
    }
    Q_INVOKABLE void leave(QWindow* window) {
        QDragLeaveEvent event;
        QCoreApplication::sendEvent(window, &event);
        data.reset();
    }
};
