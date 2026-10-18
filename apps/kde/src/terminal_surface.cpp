#include "terminal_surface.h"
#include <QPainter>
#include <QFontDatabase>
#include <QJsonDocument>
#include <QJsonArray>
#include <QKeyEvent>
#include <QInputMethodEvent>
#include <QMouseEvent>
#include <QWheelEvent>
#include <QGuiApplication>
#include <QClipboard>
#include <QtQml/qqml.h>
#include <algorithm>
#include <cmath>

namespace {
// The core's bundled face and grid; set once before QML loads.
struct TerminalFont { QString family; qreal size, cellWidth, cellHeight, baseline; };
TerminalFont terminalFont;
qreal snap(qreal value) { return std::round(value); }
}

extern "C" void configure_terminal_font(const char* family, double size, double cellWidth, double cellHeight,
    double baseline, const unsigned char* const* faces, const size_t* lengths, size_t count) {
    for (size_t i=0; i<count; ++i)
        QFontDatabase::addApplicationFontFromData(QByteArray::fromRawData(reinterpret_cast<const char*>(faces[i]), qsizetype(lengths[i])));
    terminalFont = {QString::fromUtf8(family), size, cellWidth, cellHeight, baseline};
}

TerminalSurface::TerminalSurface(QQuickItem* parent) : QQuickPaintedItem(parent) {
    setAcceptedMouseButtons(Qt::LeftButton);
    setFlag(ItemAcceptsInputMethod);
    setActiveFocusOnTab(true);
    // Qt merges in system fonts for glyphs the bundled face lacks.
    font_ = QFont(terminalFont.family);
    font_.setStyleHint(QFont::Monospace);
    font_.setPixelSize(qRound(terminalFont.size));
    font_.setKerning(false);
    cellWidth_ = terminalFont.cellWidth;
    cellHeight_ = terminalFont.cellHeight;
}
void TerminalSurface::setSnapshot(const QString& value) {
    if (value == snapshot_) return;
    snapshot_ = value;
    const auto state = QJsonDocument::fromJson(value.toUtf8()).object();
    const auto active = state.value("active").toInteger();
    const bool switched = active != active_;
    active_ = active;
    frame_ = state.value("frame").toObject();
    if (switched) { anchor_ = end_ = -1; resizeGrid(); forceActiveFocus(); }
    emit snapshotChanged(); update();
}
void TerminalSurface::resizeGrid() {
    if (active_ != 0) emit resized(std::clamp(int((width()-12)/cellWidth_),2,500), std::clamp(int((height()-8)/cellHeight_),1,200));
}
void TerminalSurface::geometryChange(const QRectF& now, const QRectF& before) {
    QQuickPaintedItem::geometryChange(now,before); resizeGrid();
}
void TerminalSurface::paint(QPainter* painter) {
    painter->fillRect(boundingRect(),QColor::fromRgb(frame_.value("background").toInt(0x191b1f)));
    const int cols = frame_.value("cols").toInt();
    if (cols < 1) return;
    const auto cells = frame_.value("cells").toArray();
    for (int i=0; i<cells.size(); ++i) {
        const auto cell = cells[i].toObject();
        const bool selected = anchor_ >= 0 && i >= std::min(anchor_,end_) && i <= std::max(anchor_,end_);
        // Fractional cell widths: snap edges so adjacent backgrounds do not blend.
        const qreal x=snap(6+(i%cols)*cellWidth_), y=snap(4+(i/cols)*cellHeight_);
        painter->fillRect(QRectF(x,y,snap(6+(i%cols+1)*cellWidth_)-x,snap(4+(i/cols+1)*cellHeight_)-y),QColor::fromRgb(selected ? 0x365880 : cell.value("background").toInt()));
    }
    for (int i=0; i<cells.size(); ++i) {
        const auto cell = cells[i].toObject();
        if (cell.value("width").toInt() == 0) continue;
        auto font = font_; font.setBold(cell.value("bold").toBool()); font.setItalic(cell.value("italic").toBool()); font.setUnderline(cell.value("underline").toBool());
        painter->setFont(font); painter->setPen(QColor::fromRgb(cell.value("foreground").toInt()));
        painter->drawText(QPointF(6+(i%cols)*cellWidth_,4+(i/cols)*cellHeight_+terminalFont.baseline),cell.value("text").toString());
    }
    const auto cursor = frame_.value("cursor").toArray();
    if (hasActiveFocus() && cursor.size()==2) {
        painter->setPen(Qt::white); painter->drawRect(QRectF(6+cursor[0].toInt()*cellWidth_,4+cursor[1].toInt()*cellHeight_,cellWidth_,cellHeight_));
    }
}
void TerminalSurface::copySelection() {
    if (anchor_<0) return;
    const auto cells = frame_.value("cells").toArray(); const int cols=frame_.value("cols").toInt();
    QString text;
    for (int i=std::min(anchor_,end_); i<=std::max(anchor_,end_) && i<cells.size(); ++i) {
        if (i>std::min(anchor_,end_) && i%cols==0) text += '\n';
        const auto cell=cells[i].toObject(); if (cell.value("width").toInt()==0) continue;
        const auto part=cell.value("text").toString(); text += part.isEmpty() ? QStringLiteral(" ") : part;
    }
    QGuiApplication::clipboard()->setText(text);
}
void TerminalSurface::keyPressEvent(QKeyEvent* event) {
    const auto mods=event->modifiers();
#ifdef Q_OS_MACOS
    const bool shortcut=mods.testFlag(Qt::ControlModifier);
#else
    const bool shortcut=mods.testFlag(Qt::ControlModifier) && mods.testFlag(Qt::ShiftModifier);
#endif
    if (shortcut && event->key()==Qt::Key_C) { copySelection(); event->accept(); return; }
    if (shortcut && event->key()==Qt::Key_V) { emit keyInput({},QGuiApplication::clipboard()->text(),0,true); event->accept(); return; }
    QString key;
    switch (event->key()) {
    case Qt::Key_Return: case Qt::Key_Enter: key="Enter"; break;
    case Qt::Key_Tab: case Qt::Key_Backtab: key="Tab"; break;
    case Qt::Key_Backspace: key="Backspace"; break;
    case Qt::Key_Escape: key="Escape"; break;
    case Qt::Key_Delete: key="Delete"; break;
    case Qt::Key_Insert: key="Insert"; break;
    case Qt::Key_Home: key="Home"; break;
    case Qt::Key_End: key="End"; break;
    case Qt::Key_PageUp: key="PageUp"; break;
    case Qt::Key_PageDown: key="PageDown"; break;
    case Qt::Key_Left: key="ArrowLeft"; break;
    case Qt::Key_Right: key="ArrowRight"; break;
    case Qt::Key_Up: key="ArrowUp"; break;
    case Qt::Key_Down: key="ArrowDown"; break;
    default:
        if(event->key()>=Qt::Key_F1 && event->key()<=Qt::Key_F12) key="F"+QString::number(event->key()-Qt::Key_F1+1);
        else if(event->key()>=Qt::Key_A && event->key()<=Qt::Key_Z) key=QChar('a'+event->key()-Qt::Key_A);
        else key=event->text();
    }
#ifdef Q_OS_MACOS
    const bool control=mods.testFlag(Qt::MetaModifier);
#else
    const bool control=mods.testFlag(Qt::ControlModifier);
#endif
    const int modifiers=(mods.testFlag(Qt::ShiftModifier)?1:0)|(control?2:0)|(mods.testFlag(Qt::AltModifier)?4:0);
    emit keyInput(key,event->text(),modifiers,false); event->accept();
}
void TerminalSurface::inputMethodEvent(QInputMethodEvent* event) {
    if (!event->commitString().isEmpty()) emit keyInput({},event->commitString(),0,false);
    event->accept();
}
QVariant TerminalSurface::inputMethodQuery(Qt::InputMethodQuery query) const {
    if (query==Qt::ImEnabled) return true;
    if (query==Qt::ImCursorRectangle) {
        const auto cursor=frame_.value("cursor").toArray();
        return QRectF(6+(cursor.size()==2?cursor[0].toInt():0)*cellWidth_,4+(cursor.size()==2?cursor[1].toInt():0)*cellHeight_,cellWidth_,cellHeight_);
    }
    return QQuickPaintedItem::inputMethodQuery(query);
}
int TerminalSurface::cellAt(QPointF point) const {
    const int cols=frame_.value("cols").toInt(1),rows=frame_.value("rows").toInt(1);
    return std::clamp(int((point.y()-4)/cellHeight_),0,rows-1)*cols+std::clamp(int((point.x()-6)/cellWidth_),0,cols-1);
}
void TerminalSurface::mousePressEvent(QMouseEvent* event) { forceActiveFocus(); anchor_=end_=cellAt(event->position()); update(); }
void TerminalSurface::mouseMoveEvent(QMouseEvent* event) { end_=cellAt(event->position()); update(); }
void TerminalSurface::wheelEvent(QWheelEvent* event) { emit scrolled(event->angleDelta().y()>0?-3:3); event->accept(); }
extern "C" void register_terminal_surface() { qmlRegisterType<TerminalSurface>("org.nativeworkspace.Terminal",1,0,"TerminalSurface"); }
