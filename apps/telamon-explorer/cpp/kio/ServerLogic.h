// Connect to Server and what Files keeps about servers: the dialog's protocol
// list and the address built from its fields, the list of recent servers, the
// "Not encrypted" note for a location, and the Settings switch for previews
// of files on servers. What decides (the address rules, which protocols are
// encrypted, how many recents are kept) is the Rust core's (`servers`); this
// file reads and writes `telamon-explorerrc` and asks KIO what it supports.
// A password is never an argument here and never kept: KIO asks for it when
// the server wants one.
#pragma once

#include <QByteArray>
#include <QObject>
#include <QQmlEngine>
#include <QUrl>
#include <QVariantList>
#include <QVariantMap>

class ServerLogic : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    // Thumbnails and previews of files on servers (off: icons only, nothing
    // is downloaded to draw a file). Kept in [Remote] PreviewFiles.
    Q_PROPERTY(bool previewRemote READ previewRemote WRITE setPreviewRemote NOTIFY previewRemoteChanged)
    // Bumped when the recent servers change.
    Q_PROPERTY(int recentRevision READ recentRevision NOTIFY recentChanged)

public:
    explicit ServerLogic(QObject *parent = nullptr);

    // The switch, readable from anywhere (the model and the thumbnail
    // provider ask it for every file).
    static bool previewRemoteEnabled();
    // Whether a URL's scheme is a server Files talks to over the network
    // (smb, sftp, fish, ftp, ftps, webdav, webdavs, nfs).
    static bool isServerScheme(const QString &scheme);
    // The port a server of this scheme listens on when the URL names none; 0 for none.
    static int defaultPort(const QString &scheme);

    bool previewRemote() const { return previewRemoteEnabled(); }
    void setPreviewRemote(bool on);
    int recentRevision() const { return m_revision; }

    // build(), for C++: {ok, text}.
    static QVariantMap buildFor(int protocol, const QString &server, const QString &folder = QString(), const QString &user = QString());
    // The protocols the dialog lists, those KIO has a worker for: a list of
    // {code, scheme, label, port, encrypted}.
    Q_INVOKABLE QVariantList protocols() const;
    // The address for what the dialog holds: {ok, text} where text is the
    // address, or the reason in plain words. No password is ever part of it.
    Q_INVOKABLE QVariantMap build(int protocol, const QString &server, const QString &folder, const QString &user) const;
    // An address taken apart into the dialog's fields ({protocol, server,
    // folder, user}); empty for one that isn't ours.
    Q_INVOKABLE QVariantMap parse(const QUrl &url) const;
    // "Not encrypted" for a location on a server that does not protect what
    // it carries; empty for the rest.
    Q_INVOKABLE QString securityNote(const QUrl &url) const;
    // The recent servers, the newest first: a list of {url, label}.
    Q_INVOKABLE QVariantList recents() const;
    // `url` was connected to: it goes to the top of the list (an address
    // with a password, or one that isn't ours, is not kept).
    Q_INVOKABLE void remember(const QUrl &url);
    Q_INVOKABLE void forgetRecent(const QUrl &url);
    Q_INVOKABLE void clearRecents();

Q_SIGNALS:
    void previewRemoteChanged();
    void recentChanged();

private:
    void save();

    // The list as the core writes it, one address per line.
    QByteArray m_recent;
    int m_revision = 0;
};
