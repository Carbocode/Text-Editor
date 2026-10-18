#pragma once
#include <QQuickPaintedItem>
#include <QJsonObject>
#include <QFont>

class TerminalSurface : public QQuickPaintedItem {
    Q_OBJECT
    Q_PROPERTY(QString snapshot READ snapshot WRITE setSnapshot NOTIFY snapshotChanged)
public:
    explicit TerminalSurface(QQuickItem* parent = nullptr);
    QString snapshot() const { return snapshot_; }
    void setSnapshot(const QString& value);
    void paint(QPainter*) override;
signals:
    void snapshotChanged();
    void keyInput(QString key, QString text, int modifiers, bool paste);
    void resized(int cols, int rows);
    void scrolled(int lines);
protected:
    void geometryChange(const QRectF&, const QRectF&) override;
    void keyPressEvent(QKeyEvent*) override;
    void inputMethodEvent(QInputMethodEvent*) override;
    QVariant inputMethodQuery(Qt::InputMethodQuery) const override;
    void mousePressEvent(QMouseEvent*) override;
    void mouseMoveEvent(QMouseEvent*) override;
    void wheelEvent(QWheelEvent*) override;
private:
    void resizeGrid();
    int cellAt(QPointF) const;
    void copySelection();
    QString snapshot_;
    QJsonObject frame_;
    QFont font_;
    qreal cellWidth_;
    qreal cellHeight_;
    int anchor_ = -1;
    int end_ = -1;
    qint64 active_ = 0;
};
extern "C" void register_terminal_surface();
extern "C" void configure_terminal_font(const char* family, double size, double cellWidth, double cellHeight,
    double baseline, const unsigned char* const* faces, const size_t* lengths, size_t count);
