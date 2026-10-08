#include "PlacesModel.h"

namespace
{
constexpr quint32 ActReorder = 1u << 9;
constexpr quint32 ActUnmount = 1u << 5;
}

PlacesModel::PlacesModel(QObject *parent)
    : QAbstractListModel(parent)
{
    connect(PlacesLogic::instance(), &PlacesLogic::entriesChanged, this, &PlacesModel::refresh);
    refresh();
}

void PlacesModel::setSection(int s)
{
    if (m_section != s) {
        m_section = s;
        emit sectionChanged();
        refresh();
    }
}

int PlacesModel::rowCount(const QModelIndex &parent) const
{
    return parent.isValid() ? 0 : int(m_rows.size());
}

QHash<int, QByteArray> PlacesModel::roleNames() const
{
    return {
        {KeyRole, "placeKey"},
        {TextRole, "placeText"},
        {IconNameRole, "placeIcon"},
        {UrlRole, "placeUrl"},
        {KindRole, "placeKind"},
        {HiddenRole, "placeHidden"},
        {MountedRole, "placeMounted"},
        {BusyRole, "placeBusy"},
        {UsagePercentRole, "placeUsage"},
        {UsageTextRole, "placeUsageText"},
        {ValueRole, "placeValue"},
        {TipRole, "placeTip"},
        {ReorderRole, "placeReorder"},
        {CanUnmountRole, "placeCanUnmount"},
    };
}

QVariant PlacesModel::data(const QModelIndex &index, int role) const
{
    if (!index.isValid() || index.row() < 0 || index.row() >= m_rows.size()) {
        return {};
    }
    const PlaceEntry &e = m_rows.at(index.row());
    switch (role) {
    case KeyRole:
        return e.key;
    case TextRole:
        return e.text;
    case IconNameRole:
        return e.iconName;
    case UrlRole:
        return e.url;
    case KindRole:
        return e.kind;
    case HiddenRole:
        return e.hidden;
    case MountedRole:
        return e.mounted;
    case BusyRole:
        return e.busy;
    case UsagePercentRole:
        return e.usagePercent;
    case UsageTextRole:
        return e.usageText;
    case ValueRole:
        return e.value;
    case TipRole:
        return e.tooltip;
    case ReorderRole:
        return bool(e.actions & ActReorder);
    case CanUnmountRole:
        return bool(e.actions & ActUnmount);
    default:
        return {};
    }
}

// Takes the section's places from PlacesLogic. The same places in the same
// order only change their values in place (a delegate keeps its hover, focus
// and drag); anything else resets the list.
void PlacesModel::refresh()
{
    QList<PlaceEntry> rows;
    const bool showHidden = PlacesLogic::instance()->showHidden();
    for (const PlaceEntry &e : PlacesLogic::instance()->entries()) {
        if (e.section == m_section && (showHidden || !e.hidden)) {
            rows.append(e);
        }
    }
    bool sameShape = rows.size() == m_rows.size();
    for (int i = 0; sameShape && i < rows.size(); ++i) {
        sameShape = rows.at(i).key == m_rows.at(i).key;
    }
    if (!sameShape) {
        beginResetModel();
        m_rows = rows;
        endResetModel();
        return;
    }
    for (int i = 0; i < rows.size(); ++i) {
        if (!(rows.at(i) == m_rows.at(i))) {
            m_rows[i] = rows.at(i);
            emit dataChanged(index(i), index(i));
        }
    }
}
