#include "StandardPlaces.h"
#include "RustBridge.h"

#include <KIO/Global>

#include <QDir>
#include <QFileInfo>
#include <QStandardPaths>

StandardPlaces::StandardPlaces(QObject *parent)
    : QObject(parent)
{
}

QUrl StandardPlaces::place(const QString &key) const
{
    if (key == QLatin1String("recent")) {
        return QUrl(QStringLiteral("recentlyused:/"));
    }
    if (key == QLatin1String("network")) {
        return QUrl(QStringLiteral("network:/"));
    }
    if (key == QLatin1String("trash")) {
        return QUrl(QStringLiteral("trash:/"));
    }
    QStandardPaths::StandardLocation loc = QStandardPaths::HomeLocation;
    if (key == QLatin1String("desktop")) {
        loc = QStandardPaths::DesktopLocation;
    } else if (key == QLatin1String("documents")) {
        loc = QStandardPaths::DocumentsLocation;
    } else if (key == QLatin1String("downloads")) {
        loc = QStandardPaths::DownloadLocation;
    } else if (key == QLatin1String("pictures")) {
        loc = QStandardPaths::PicturesLocation;
    } else if (key == QLatin1String("music")) {
        loc = QStandardPaths::MusicLocation;
    } else if (key == QLatin1String("videos")) {
        loc = QStandardPaths::MoviesLocation;
    }
    QString path = QStandardPaths::writableLocation(loc);
    if (path.isEmpty()) {
        path = QDir::homePath();
    }
    return QUrl::fromLocalFile(path);
}

QUrl StandardPlaces::parentUrl(const QUrl &url) const
{
    if (!url.isValid()) {
        return url;
    }
    // Up from the top of an archive is the folder the archive file is in.
    if (rustArchiveScheme(url.scheme())) {
        const QUrl up = rustArchiveParent(url);
        if (up.isValid()) {
            return up;
        }
    }
    return KIO::upUrl(url);
}

bool StandardPlaces::isLocalFile(const QUrl &url) const
{
    if (!url.isLocalFile()) {
        return false;
    }
    const QFileInfo info(url.toLocalFile());
    return info.exists() && !info.isDir();
}

QString StandardPlaces::displayLocation(const QUrl &url) const
{
    if (!url.isValid()) {
        return QString();
    }
    if (url.scheme() == QLatin1String("trash") && url.path().size() <= 1) {
        return tr("Trash");
    }
    if (url.scheme() == QLatin1String("recentlyused")) {
        return tr("Recent");
    }
    QString out;
    // An archive's contents are shown by where the archive is: /home/me/a.zip/folder.
    if (!url.isLocalFile() && !rustArchiveScheme(url.scheme())) {
        out = url.scheme() + QStringLiteral("://") + rustDisplayName(url.host().toUtf8());
    }
    const QStringList parts = url.path().split(QLatin1Char('/'), Qt::SkipEmptyParts);
    for (const QString &p : parts) {
        out += QLatin1Char('/') + rustDisplayName(p.toUtf8());
    }
    return out.isEmpty() ? QStringLiteral("/") : out;
}

QString StandardPlaces::tabTitle(const QUrl &url) const
{
    if (!url.isValid()) {
        return QString();
    }
    if (url.scheme() == QLatin1String("trash") && url.path().size() <= 1) {
        return tr("Trash");
    }
    if (url.scheme() == QLatin1String("recentlyused")) {
        return tr("Recent");
    }
    if (url.scheme() == QLatin1String("network") && url.path().size() <= 1) {
        return tr("Network");
    }
    if (url.isLocalFile() && QDir::cleanPath(url.path()) == QDir::cleanPath(QDir::homePath())) {
        return tr("Home");
    }
    const QStringList parts = url.path().split(QLatin1Char('/'), Qt::SkipEmptyParts);
    if (!parts.isEmpty()) {
        return rustDisplayName(parts.last().toUtf8());
    }
    // The top of a server (smb://nas/) is the server; the top of a disk is "/".
    if (!url.isLocalFile() && !url.host().isEmpty()) {
        return rustDisplayName(url.host().toUtf8());
    }
    return url.isLocalFile() ? QStringLiteral("/") : url.scheme();
}
