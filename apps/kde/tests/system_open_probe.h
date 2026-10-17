#pragma once

#include <QCoreApplication>
#include <QFileOpenEvent>
#include <QObject>
#include <QUrl>

class SystemOpenProbe : public QObject {
    Q_OBJECT
public:
    using QObject::QObject;
    Q_INVOKABLE void deliver(const QUrl& url) {
        QFileOpenEvent event(url);
        QCoreApplication::sendEvent(QCoreApplication::instance(), &event);
    }
};
