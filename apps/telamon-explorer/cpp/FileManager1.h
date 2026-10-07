// org.freedesktop.FileManager1 on the session bus (docs/DESIGN.md). Every
// call is untrusted: the URIs go to the backend's launch parsing (capped,
// validated) and are only ever shown, never opened with an application.
#pragma once

#include <QDBusAbstractAdaptor>
#include <QStringList>

#include <functional>

class FileManager1 : public QDBusAbstractAdaptor
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.freedesktop.FileManager1")

public:
    using Handler = std::function<void(const QStringList &uris, const QString &startupId)>;
    FileManager1(QObject *parent, Handler folders, Handler items, Handler properties);

    // Takes the bus name and exports the object; false (logged) when another
    // file manager owns the name.
    static bool registerOn(QObject *parent, Handler folders, Handler items, Handler properties);

public Q_SLOTS:
    void ShowFolders(const QStringList &uris, const QString &startupId);
    void ShowItems(const QStringList &uris, const QString &startupId);
    void ShowItemProperties(const QStringList &uris, const QString &startupId);

private:
    Handler m_folders, m_items, m_properties;
};
