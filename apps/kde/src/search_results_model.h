#pragma once
#include <QAbstractListModel>
#include <QVariantMap>

// Each occurrence is one record. Numeric positions remain numbers across QML.
class SearchResultsModel : public QAbstractListModel {
    Q_OBJECT
    Q_PROPERTY(QVariantList rows READ rows WRITE setRows NOTIFY rowsChanged)
    Q_PROPERTY(int count READ rowCount NOTIFY rowsChanged)
public:
    using QAbstractListModel::QAbstractListModel;
    enum Role { Id = Qt::UserRole + 1, Snippet, Line, Column, Start, End };
    int rowCount(const QModelIndex& parent = {}) const override { return parent.isValid() ? 0 : rows_.size(); }
    QVariant data(const QModelIndex& index, int role) const override {
        if (!index.isValid() || index.row() < 0 || index.row() >= rows_.size()) return {};
        return rows_[index.row()].toMap().value(QString::fromLatin1(roleNames().value(role)));
    }
    QHash<int, QByteArray> roleNames() const override {
        return {{Id,"id"}, {Snippet,"snippet"}, {Line,"line"}, {Column,"column"}, {Start,"start"}, {End,"end"}};
    }
    QVariantList rows() const { return rows_; }
    void setRows(const QVariantList& rows) {
        if (rows_ == rows) return;
        beginResetModel(); rows_ = rows; endResetModel(); emit rowsChanged();
    }
    Q_INVOKABLE QVariantMap get(int index) const { return index >= 0 && index < rows_.size() ? rows_[index].toMap() : QVariantMap{}; }
Q_SIGNALS:
    void rowsChanged();
private:
    QVariantList rows_;
};
extern "C" void register_search_results_model();
