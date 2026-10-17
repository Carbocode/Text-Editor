#include "desktop_integration.h"
#include "system_open.h"
#include "application_lifecycle.h"
#include "../tests/cursor_probe.h"
#include "../tests/file_drop_probe.h"
#include "../tests/system_open_probe.h"

#include <KColorSchemeManager>
#include <QEvent>
#include <QGuiApplication>
#include <QIcon>
#include <QIdentityProxyModel>
#include <QLibraryInfo>
#include <QQuickItem>
#include <QtQml/qqml.h>

namespace {
class ControlCursorFeedback : public QObject {
public:
    using QObject::QObject;
    bool eventFilter(QObject* object, QEvent* event) override {
        if (event->type() == QEvent::HoverEnter && object->inherits("QQuickAbstractButton")) {
            if (auto item = qobject_cast<QQuickItem*>(object);
                item && item->isEnabled() && item->cursor().shape() == Qt::ArrowCursor) {
                item->setCursor(Qt::PointingHandCursor);
            }
        }
        return false;
    }
};
}

extern "C" void configure_kde_desktop() {
    auto lifecycle = new ApplicationLifecycle(qGuiApp);
    qGuiApp->installEventFilter(lifecycle);
    qmlRegisterSingletonInstance("org.nativeworkspace.Desktop", 1, 0, "ApplicationLifecycle", lifecycle);
    auto opens = new SystemOpenRequests(qGuiApp);
    qGuiApp->installEventFilter(opens);
    qmlRegisterSingletonInstance("org.nativeworkspace.Desktop", 1, 0, "SystemOpenRequests", opens);
    // Give Qt/Kirigami buttons and item delegates a native pointing cursor.
    // Leave text, resize and active drag cursors to their toolkit controls.
    qGuiApp->installEventFilter(new ControlCursorFeedback(qGuiApp));
    // The manager follows QStyleHints::colorSchemeChanged and applies the
    // standard Breeze/Breeze Dark scheme on non-KDE platforms as well.
    KColorSchemeManager::instance()->setAutosaveChanges(false);
    // KDE installations supply their own theme; standalone Qt SDKs keep KDE
    // dependencies beside Qt and need that icon directory added explicitly.
    auto paths = QIcon::themeSearchPaths();
    const auto prefix = QLibraryInfo::path(QLibraryInfo::PrefixPath);
    paths.append(prefix + QStringLiteral("/share/icons"));
    paths.append(prefix + QStringLiteral("/icons"));
    QIcon::setThemeSearchPaths(paths);
    QIcon::setFallbackThemeName(QStringLiteral("breeze"));
    if (QIcon::themeName().isEmpty()) QIcon::setThemeName(QStringLiteral("breeze"));
    // TreeModel's QML helpers accept role names. The built-in tree editor uses
    // QAbstractItemModel's numeric roles, exposed unchanged by this Qt proxy.
    qmlRegisterType<QIdentityProxyModel>("org.nativeworkspace.Editor", 1, 0, "NativeTreeModel");
    if (QGuiApplication::arguments().join(QChar(' ')).contains(QStringLiteral("--smoke-system-open="))) {
        qmlRegisterSingletonType<SystemOpenProbe>("org.nativeworkspace.Test", 1, 0, "SystemOpenProbe",
            [](QQmlEngine*, QJSEngine*) -> QObject* { return new SystemOpenProbe; });
    }
    if (QGuiApplication::arguments().join(QChar(' ')).contains(QStringLiteral("--smoke-external="))) {
        qmlRegisterSingletonType<FileDropProbe>("org.nativeworkspace.Test", 1, 0, "FileDropProbe",
            [](QQmlEngine*, QJSEngine*) -> QObject* { return new FileDropProbe; });
    }
    if (QGuiApplication::arguments().join(QChar(' ')).contains(QStringLiteral("--smoke-cursors="))) {
        qmlRegisterSingletonType<CursorProbe>("org.nativeworkspace.Test", 1, 0, "CursorProbe",
            [](QQmlEngine*, QJSEngine*) -> QObject* { return new CursorProbe; });
    }
}
