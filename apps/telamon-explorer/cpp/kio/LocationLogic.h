// What the path bar and the status line need from the system: the segments of
// a location (from the Rust core), the subfolders of a folder for the chevron
// menu, completions of typed text, and the free space of a disk. Listing a
// local folder and statfs run on a worker; a folder on a server is listed by
// a KIO job (asynchronous on the GUI thread) that is stopped after a few
// seconds. Answers come back as signals that carry the number the request
// returned, so a slow answer to an old request is easy to drop.
#pragma once

#include <QByteArray>
#include <QElapsedTimer>
#include <QObject>
#include <QPointer>
#include <QQmlEngine>
#include <QUrl>
#include <QVariantList>

#include <functional>


class LocationLogic : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(int menuRows READ menuRows CONSTANT)
    Q_PROPERTY(int historyRows READ historyRows CONSTANT)

public:
    explicit LocationLogic(QObject *parent = nullptr);

    int menuRows() const;
    int historyRows() const;

    // The clickable parts of `url`: a list of {label, url}, the first being
    // Home, Root, Trash or the server.
    Q_INVOKABLE QVariantList segments(const QUrl &url) const;
    // The text the address field starts with: the path of a local folder, else
    // the URL without its password. Control or bidi characters in a path keep
    // it in URL form, so nothing in it can hide.
    Q_INVOKABLE QString editText(const QUrl &url) const;

    // Lists the subfolders of `folder` for a menu. Returns a number, and
    // `subfoldersListed` carries it back with {label, url} rows (at most
    // menuRows), how many more there are, and the reason when the folder
    // couldn't be listed.
    Q_INVOKABLE int listSubfolders(const QUrl &folder, bool showHidden);
    // Offers folder names for the text typed in the address field, relative to
    // `current`. Returns a number; `completionsReady` carries it back with the
    // texts to put in the field (best first), or the reason when the text is
    // refused.
    Q_INVOKABLE int complete(const QString &text, const QUrl &current);
    // The free space on the disk that holds `folder`; `freeSpaceReady` has -1
    // for a place that has no disk (a server, Trash, Recent).
    Q_INVOKABLE int queryFreeSpace(const QUrl &folder);
    // Size as people read it ("4.2 MB").
    Q_INVOKABLE QString sizeText(double bytes) const;

Q_SIGNALS:
    void subfoldersListed(int serial, const QVariantList &rows, int more, const QString &error);
    void completionsReady(int serial, const QStringList &texts, const QString &error);
    void freeSpaceReady(int serial, double bytes);

private:
    // The names in a folder: records for the core (flag, name, 0), the names
    // in the same order, and why a listing is missing or cut short.
    struct Listing {
        QByteArray records;
        QStringList names;
        QString error;
        bool ok = false;
    };
    void list(const QUrl &folder, std::function<void(const Listing &)> done);
    void listRemote(const QUrl &folder, std::function<void(const Listing &)> done);

    int m_serial = 0;
    // The last listing made for completion, kept for a few seconds so the
    // next key doesn't list a server again.
    QUrl m_cacheUrl;
    Listing m_cache;
    QElapsedTimer m_cacheAge;
    // Rows of one remote listing in flight, to stop a stale one.
    QPointer<QObject> m_remote;
};
