#include "FileManager1.h"

#include <QDBusConnection>

FileManager1::FileManager1(QObject *parent, Handler folders, Handler items, Handler properties)
    : QDBusAbstractAdaptor(parent)
    , m_folders(std::move(folders))
    , m_items(std::move(items))
    , m_properties(std::move(properties))
{
}

bool FileManager1::registerOn(QObject *parent, Handler folders, Handler items, Handler properties)
{
    auto bus = QDBusConnection::sessionBus();
    if (!bus.isConnected()) {
        return false;
    }
    new FileManager1(parent, std::move(folders), std::move(items), std::move(properties));
    if (!bus.registerObject(QStringLiteral("/org/freedesktop/FileManager1"), parent, QDBusConnection::ExportAdaptors)) {
        qWarning("atlas-explorer: could not export /org/freedesktop/FileManager1");
        return false;
    }
    if (!bus.registerService(QStringLiteral("org.freedesktop.FileManager1"))) {
        qWarning("atlas-explorer: org.freedesktop.FileManager1 is owned by another file manager");
        return false;
    }
    return true;
}

// At most 64 URIs are passed on, as the launch rules read no more.
void FileManager1::ShowFolders(const QStringList &uris, const QString &startupId)
{
    m_folders(uris.mid(0, 65), startupId);
}

void FileManager1::ShowItems(const QStringList &uris, const QString &startupId)
{
    m_items(uris.mid(0, 65), startupId);
}

void FileManager1::ShowItemProperties(const QStringList &uris, const QString &startupId)
{
    m_properties(uris.mid(0, 65), startupId);
}
