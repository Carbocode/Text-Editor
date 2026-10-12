#pragma once

#include <QFont>
#include <QObject>

class EditorFonts : public QObject {
    Q_OBJECT
    Q_PROPERTY(QFont fixedFont READ fixedFont CONSTANT)

public:
    explicit EditorFonts(QObject* parent = nullptr);
    QFont fixedFont() const { return fixedFont_; }

private:
    QFont fixedFont_;
};

extern "C" void register_editor_fonts();
