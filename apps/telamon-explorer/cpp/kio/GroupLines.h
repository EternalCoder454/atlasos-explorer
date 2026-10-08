// The Icons view with groups: a list of lines for a ListView, because a grid
// can't have a header across its width. A line is either a group's header or
// up to `perRow` cells (rows of the folder), and the lines of a collapsed
// group aren't there. RowSlice is the model of one line's cells.
#pragma once

#include "FolderModel.h"

#include <QAbstractListModel>
#include <QPointer>
#include <QQmlEngine>
#include <QTimer>

class GroupLines : public QAbstractListModel
{
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(FolderModel *source READ source WRITE setSource NOTIFY sourceChanged)
    Q_PROPERTY(int perRow READ perRow WRITE setPerRow NOTIFY perRowChanged)

public:
    enum Roles { KindRole = Qt::UserRole + 1, LabelRole, CountRole, CollapsedRole, FirstRole, CellsRole };
    // KindRole: 0 a group's header, 1 a line of cells.
    explicit GroupLines(QObject *parent = nullptr);

    FolderModel *source() const { return m_source; }
    void setSource(FolderModel *m);
    int perRow() const { return m_perRow; }
    void setPerRow(int n);

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

    // The line (an index of this model) that has the folder's row; -1 for none.
    Q_INVOKABLE int lineOf(int sourceRow) const;
    // The folder's row in cell `cell` of line `line`; -1 when there is none.
    Q_INVOKABLE int rowAtCell(int line, int cell) const;
    // The row `steps` lines of cells down (negative: up) from `row`, in the same
    // column or the last one of a short line, headers and folded groups skipped;
    // the first or last row's line at the ends.
    Q_INVOKABLE int rowAfterLines(int row, int steps) const;

Q_SIGNALS:
    void sourceChanged();
    void perRowChanged();

private:
    struct Line {
        int kind = 1;
        QString label;
        int count = 0;
        bool collapsed = false;
        int first = 0;
        int cells = 0;
        bool operator==(const Line &o) const
        {
            return kind == o.kind && label == o.label && count == o.count && collapsed == o.collapsed && first == o.first && cells == o.cells;
        }
    };
    void schedule();
    void rebuild();

    QPointer<FolderModel> m_source;
    int m_perRow = 1;
    QList<Line> m_lines;
    QTimer m_timer;
};

class RowSlice : public QAbstractListModel
{
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(FolderModel *source READ source WRITE setSource NOTIFY sourceChanged)
    Q_PROPERTY(int first READ first WRITE setFirst NOTIFY firstChanged)
    Q_PROPERTY(int count READ count WRITE setCount NOTIFY countChanged)

public:
    // The folder's own roles, and the folder's row of the cell.
    enum { SourceRowRole = Qt::UserRole + 100 };
    explicit RowSlice(QObject *parent = nullptr);

    FolderModel *source() const { return m_source; }
    void setSource(FolderModel *m);
    int first() const { return m_first; }
    void setFirst(int n);
    int count() const { return m_count; }
    void setCount(int n);

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

Q_SIGNALS:
    void sourceChanged();
    void firstChanged();
    void countChanged();

private:
    int size() const;
    void reset();

    QPointer<FolderModel> m_source;
    int m_first = 0;
    int m_count = 0;
    QTimer m_timer;
};
