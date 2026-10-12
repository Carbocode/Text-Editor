#include "editor_fonts.h"

#include <QFontDatabase>
#include <QtQml/qqml.h>

EditorFonts::EditorFonts(QObject* parent)
    : QObject(parent), fixedFont_(QFontDatabase::systemFont(QFontDatabase::FixedFont)) {
    fixedFont_.setStyleHint(QFont::Monospace);
    fixedFont_.setFixedPitch(true);
    fixedFont_.setKerning(false);
}

extern "C" void register_editor_fonts() {
    qmlRegisterSingletonType<EditorFonts>("org.nativeworkspace.Editor", 1, 0, "EditorFonts",
        [](QQmlEngine*, QJSEngine*) -> QObject* { return new EditorFonts; });
}
