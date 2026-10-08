// The places of one sidebar section as a list model: what a Repeater needs to
// draw them. The data is PlacesLogic's; a section is picked by number
// (PlacesLogic.Favourites, .Drives, .Network, .Trash).
#pragma once

#include "PlacesLogic.h"

#include <QAbstractListModel>
#include <QQmlEngine>

class PlacesModel : public QAbstractListModel
{
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(int section READ section WRITE setSection NOTIFY sectionChanged)

Q_SIGNALS:
    void sectionChanged();

public:
    enum Role {
        KeyRole = Qt::UserRole + 1,
        TextRole,
        IconNameRole,
        UrlRole,
        KindRole,
        HiddenRole,
        MountedRole,
        BusyRole,
        UsagePercentRole,
        UsageTextRole,
        ValueRole,
        TipRole,
        ReorderRole,
        CanUnmountRole,
    };

    explicit PlacesModel(QObject *parent = nullptr);

    int section() const { return m_section; }
    void setSection(int s);

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

private:
    void refresh();

    int m_section = PlacesLogic::Favourites;
    QList<PlaceEntry> m_rows;
};
