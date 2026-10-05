#include "StandardPlaces.h"
#include "RustBridge.h"

#include <KIO/Global>

#include <QDir>
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
    return url.isValid() ? KIO::upUrl(url) : url;
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
    if (!url.isLocalFile()) {
        out = url.scheme() + QStringLiteral("://") + rustDisplayName(url.host().toUtf8());
    }
    const QStringList parts = url.path().split(QLatin1Char('/'), Qt::SkipEmptyParts);
    for (const QString &p : parts) {
        out += QLatin1Char('/') + rustDisplayName(p.toUtf8());
    }
    return out.isEmpty() ? QStringLiteral("/") : out;
}
