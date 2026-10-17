#pragma once
#include <QObject>
#include <QQuickItem>
#include <QCoreApplication>
#include <QInputMethodEvent>
#include <QKeyEvent>

class TerminalProbe : public QObject {
    Q_OBJECT
public:
    using QObject::QObject;
    Q_INVOKABLE void type(QObject* target, const QString& text) {
        QInputMethodEvent commit;
        commit.setCommitString(text);
        QCoreApplication::sendEvent(target,&commit);
        QKeyEvent enter(QEvent::KeyPress,Qt::Key_Return,Qt::NoModifier,QStringLiteral("\r"));
        QCoreApplication::sendEvent(target,&enter);
    }
};
