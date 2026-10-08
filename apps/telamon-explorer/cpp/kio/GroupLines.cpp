#include "GroupLines.h"

#include <algorithm>
#include <cstdlib>

GroupLines::GroupLines(QObject *parent)
    : QAbstractListModel(parent)
{
    // Several changes in one turn of the event loop are one rebuild.
    m_timer.setSingleShot(true);
    m_timer.setInterval(0);
    connect(&m_timer, &QTimer::timeout, this, &GroupLines::rebuild);
}

void GroupLines::setSource(FolderModel *m)
{
    if (m == m_source) {
        return;
    }
    if (m_source) {
        m_source->disconnect(this);
    }
    m_source = m;
    if (m) {
        connect(m, &QAbstractItemModel::modelReset, this, &GroupLines::schedule);
        connect(m, &QAbstractItemModel::layoutChanged, this, &GroupLines::schedule);
        connect(m, &QAbstractItemModel::rowsInserted, this, &GroupLines::schedule);
        connect(m, &QAbstractItemModel::rowsRemoved, this, &GroupLines::schedule);
        connect(m, &QAbstractItemModel::rowsMoved, this, &GroupLines::schedule);
        connect(m, &FolderModel::groupChanged, this, &GroupLines::schedule);
        connect(m, &FolderModel::groupRevisionChanged, this, &GroupLines::schedule);
        connect(m, &QAbstractItemModel::dataChanged, this, [this](const QModelIndex &, const QModelIndex &, const QList<int> &roles) {
            if (roles.contains(FolderModel::GroupCollapsedRole) || roles.contains(FolderModel::GroupRole)) {
                schedule();
            }
        });
    }
    rebuild();
    Q_EMIT sourceChanged();
}

void GroupLines::setPerRow(int n)
{
    n = std::max(1, n);
    if (n != m_perRow) {
        m_perRow = n;
        schedule();
        Q_EMIT perRowChanged();
    }
}

void GroupLines::schedule()
{
    m_timer.start();
}

int GroupLines::rowCount(const QModelIndex &parent) const
{
    return parent.isValid() ? 0 : int(m_lines.size());
}

QHash<int, QByteArray> GroupLines::roleNames() const
{
    return {{KindRole, "kind"}, {LabelRole, "label"}, {CountRole, "count"}, {CollapsedRole, "collapsed"}, {FirstRole, "first"}, {CellsRole, "cells"}};
}

QVariant GroupLines::data(const QModelIndex &index, int role) const
{
    if (!index.isValid() || index.row() < 0 || index.row() >= m_lines.size()) {
        return {};
    }
    const Line &l = m_lines.at(index.row());
    switch (role) {
    case KindRole:
        return l.kind;
    case LabelRole:
        return l.label;
    case CountRole:
        return l.count;
    case CollapsedRole:
        return l.collapsed;
    case FirstRole:
        return l.first;
    case CellsRole:
        return l.cells;
    default:
        return {};
    }
}

// What the lines are now, from the folder's groups.
void GroupLines::rebuild()
{
    m_timer.stop();
    QList<Line> next;
    if (m_source) {
        const auto spans = m_source->groupSpans();
        next.reserve(int(m_source->count() / m_perRow + spans.size() + 1));
        for (const auto &s : spans) {
            // Rows not grouped yet (just added) have no name and no header.
            const bool named = !s.label.isEmpty();
            const bool collapsed = named && m_source->isGroupCollapsed(s.label);
            if (named) {
                next.append({0, s.label, s.count, collapsed, s.first, 0});
            }
            if (!collapsed) {
                for (int at = 0; at < s.count; at += m_perRow) {
                    next.append({1, QString(), 0, false, s.first + at, std::min(m_perRow, s.count - at)});
                }
            }
        }
    }
    // Only what changed is replaced, so a list being scrolled stays where it is.
    int prefix = 0;
    const int common = int(std::min(m_lines.size(), next.size()));
    while (prefix < common && m_lines.at(prefix) == next.at(prefix)) {
        ++prefix;
    }
    int suffix = 0;
    while (suffix < common - prefix && m_lines.at(m_lines.size() - 1 - suffix) == next.at(next.size() - 1 - suffix)) {
        ++suffix;
    }
    const int oldMid = int(m_lines.size()) - prefix - suffix;
    const int newMid = int(next.size()) - prefix - suffix;
    if (oldMid == 0 && newMid == 0) {
        return;
    }
    // The lines in between are taken out and the new ones put in (not changed
    // in place: a line that becomes a header changes its height, which the list
    // does not follow well).
    if (oldMid > 0) {
        beginRemoveRows({}, prefix, prefix + oldMid - 1);
        m_lines.remove(prefix, oldMid);
        endRemoveRows();
    }
    if (newMid > 0) {
        beginInsertRows({}, prefix, prefix + newMid - 1);
        for (int i = 0; i < newMid; ++i) {
            m_lines.insert(prefix + i, next.at(prefix + i));
        }
        endInsertRows();
    }
}

int GroupLines::lineOf(int sourceRow) const
{
    for (int i = 0; i < m_lines.size(); ++i) {
        const Line &l = m_lines.at(i);
        if (l.kind == 1 && sourceRow >= l.first && sourceRow < l.first + l.cells) {
            return i;
        }
    }
    return -1;
}

int GroupLines::rowAfterLines(int row, int steps) const
{
    int at = lineOf(row);
    if (at < 0 || steps == 0) {
        return row;
    }
    const int column = row - m_lines.at(at).first;
    const int dir = steps < 0 ? -1 : 1;
    for (int left = std::abs(steps); left > 0;) {
        int next = at + dir;
        while (next >= 0 && next < m_lines.size() && m_lines.at(next).kind != 1) {
            next += dir;
        }
        if (next < 0 || next >= m_lines.size()) {
            break;
        }
        at = next;
        --left;
    }
    const Line &l = m_lines.at(at);
    return l.first + std::min(column, l.cells - 1);
}

int GroupLines::rowAtCell(int line, int cell) const
{
    if (line < 0 || line >= m_lines.size()) {
        return -1;
    }
    const Line &l = m_lines.at(line);
    return l.kind == 1 && cell >= 0 && cell < l.cells ? l.first + cell : -1;
}

// ---- RowSlice ----

RowSlice::RowSlice(QObject *parent)
    : QAbstractListModel(parent)
{
    m_timer.setSingleShot(true);
    m_timer.setInterval(0);
    connect(&m_timer, &QTimer::timeout, this, [this] {
        beginResetModel();
        endResetModel();
    });
}

int RowSlice::size() const
{
    if (!m_source || m_first < 0) {
        return 0;
    }
    return std::max(0, std::min(m_count, m_source->count() - m_first));
}

void RowSlice::reset()
{
    // Many changes in one turn of the event loop reset the cells once.
    m_timer.start();
}

void RowSlice::setSource(FolderModel *m)
{
    if (m == m_source) {
        return;
    }
    if (m_source) {
        m_source->disconnect(this);
    }
    m_source = m;
    if (m) {
        connect(m, &QAbstractItemModel::modelReset, this, &RowSlice::reset);
        // A new order changes what the cells show, not how many there are.
        connect(m, &QAbstractItemModel::layoutChanged, this, [this] {
            if (size() > 0) {
                Q_EMIT dataChanged(index(0), index(size() - 1));
            }
        });
        connect(m, &QAbstractItemModel::rowsInserted, this, &RowSlice::reset);
        connect(m, &QAbstractItemModel::rowsRemoved, this, &RowSlice::reset);
        connect(m, &QAbstractItemModel::rowsMoved, this, &RowSlice::reset);
        connect(m, &QAbstractItemModel::dataChanged, this, [this](const QModelIndex &a, const QModelIndex &b, const QList<int> &roles) {
            const int lo = std::max(a.row(), m_first), hi = std::min(b.row(), m_first + size() - 1);
            if (lo <= hi) {
                Q_EMIT dataChanged(index(lo - m_first), index(hi - m_first), roles);
            }
        });
    }
    beginResetModel();
    endResetModel();
    Q_EMIT sourceChanged();
}

void RowSlice::setFirst(int n)
{
    if (n != m_first) {
        beginResetModel();
        m_first = n;
        endResetModel();
        Q_EMIT firstChanged();
    }
}

void RowSlice::setCount(int n)
{
    if (n != m_count) {
        beginResetModel();
        m_count = n;
        endResetModel();
        Q_EMIT countChanged();
    }
}

int RowSlice::rowCount(const QModelIndex &parent) const
{
    return parent.isValid() ? 0 : size();
}

QHash<int, QByteArray> RowSlice::roleNames() const
{
    QHash<int, QByteArray> roles = m_source ? m_source->roleNames() : QHash<int, QByteArray>();
    roles.insert(SourceRowRole, "sourceRow");
    return roles;
}

QVariant RowSlice::data(const QModelIndex &index, int role) const
{
    if (!index.isValid() || index.row() < 0 || index.row() >= size()) {
        return {};
    }
    if (role == SourceRowRole) {
        return m_first + index.row();
    }
    return m_source->data(m_source->index(m_first + index.row(), 0), role);
}
