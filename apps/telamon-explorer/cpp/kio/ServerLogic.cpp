#include "ServerLogic.h"
#include "RustBridge.h"

#include <KConfigGroup>
#include <KProtocolInfo>
#include <KSharedConfig>

#include <atomic>

namespace
{
KConfigGroup remoteGroup()
{
    return KSharedConfig::openConfig(QStringLiteral("telamon-explorerrc"))->group(QStringLiteral("Remote"));
}

KConfigGroup serversGroup()
{
    return KSharedConfig::openConfig(QStringLiteral("telamon-explorerrc"))->group(QStringLiteral("Servers"));
}

// -1: not read yet.
std::atomic<int> g_previewRemote{-1};

// KIO's previews skip a file on a server unless the settings allow its size
// (PreviewSettings/MaximumRemoteSize, 0 by default). With the switch on, files
// up to 5 MB are previewed, and nothing else is downloaded to draw it.
void allowRemotePreviews()
{
    KConfigGroup g = KSharedConfig::openConfig()->group(QStringLiteral("PreviewSettings"));
    if (g.readEntry("MaximumRemoteSize", qint64(0)) <= 0) {
        g.writeEntry("MaximumRemoteSize", qint64(5) * 1024 * 1024);
    }
}

// The protocol's scheme (which 0) or its name (1).
QString protocolText(uint32_t code, uint32_t which)
{
    QByteArray buf(64, 0);
    size_t n = telamon_servers_protocol(code, which, reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()));
    if (n > size_t(buf.size())) {
        buf.resize(qsizetype(n));
        n = telamon_servers_protocol(code, which, reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()));
    }
    return QString::fromUtf8(buf.constData(), qsizetype(n));
}
}

ServerLogic::ServerLogic(QObject *parent)
    : QObject(parent)
{
    // A settings file can hold anything: the core keeps the addresses that are
    // ours (no password among them) and drops the rest.
    const QByteArray saved = serversGroup().readEntry("Recent", QStringList()).join(QLatin1Char('\n')).toUtf8();
    m_recent = rustBytes(telamon_servers_recent_clean, saved);
}

bool ServerLogic::previewRemoteEnabled()
{
    int v = g_previewRemote.load(std::memory_order_relaxed);
    if (v < 0) {
        v = remoteGroup().readEntry("PreviewFiles", false) ? 1 : 0;
        if (v == 1) {
            allowRemotePreviews();
        }
        g_previewRemote.store(v, std::memory_order_relaxed);
    }
    return v == 1;
}

bool ServerLogic::isServerScheme(const QString &scheme)
{
    static const QStringList schemes{QStringLiteral("smb"),    QStringLiteral("sftp"),   QStringLiteral("fish"),  QStringLiteral("ftp"),
                                     QStringLiteral("ftps"),   QStringLiteral("webdav"), QStringLiteral("webdavs"), QStringLiteral("nfs")};
    return schemes.contains(scheme.toLower());
}

int ServerLogic::defaultPort(const QString &scheme)
{
    const QString s = scheme.toLower();
    if (s == QLatin1String("sftp") || s == QLatin1String("fish")) {
        return 22;
    }
    if (s == QLatin1String("smb")) {
        return 445;
    }
    if (s == QLatin1String("ftp")) {
        return 21;
    }
    if (s == QLatin1String("ftps")) {
        return 990;
    }
    if (s == QLatin1String("webdav")) {
        return 80;
    }
    if (s == QLatin1String("webdavs")) {
        return 443;
    }
    if (s == QLatin1String("nfs")) {
        return 2049;
    }
    return 0;
}

void ServerLogic::setPreviewRemote(bool on)
{
    if (on == previewRemoteEnabled()) {
        return;
    }
    g_previewRemote.store(on ? 1 : 0, std::memory_order_relaxed);
    if (on) {
        allowRemotePreviews();
    }
    KConfigGroup g = remoteGroup();
    g.writeEntry("PreviewFiles", on);
    g.sync();
    Q_EMIT previewRemoteChanged();
}

QVariantList ServerLogic::protocols() const
{
    QVariantList out;
    const size_t n = telamon_servers_protocol_count();
    for (size_t i = 0; i < n; ++i) {
        const auto code = uint32_t(i);
        const QString scheme = protocolText(code, 0);
        // Only what KIO has a worker for (NFS comes with kio-extras).
        if (!KProtocolInfo::isKnownProtocol(scheme)) {
            continue;
        }
        out.append(QVariantMap{{QStringLiteral("code"), int(i)},
                               {QStringLiteral("scheme"), scheme},
                               {QStringLiteral("label"), protocolText(code, 1)},
                               {QStringLiteral("port"), int(telamon_servers_protocol_port(code))},
                               {QStringLiteral("encrypted"), telamon_servers_protocol_encrypted(code)}});
    }
    return out;
}

QVariantMap ServerLogic::build(int protocol, const QString &server, const QString &folder, const QString &user) const
{
    return buildFor(protocol, server, folder, user);
}

QVariantMap ServerLogic::buildFor(int protocol, const QString &server, const QString &folder, const QString &user)
{
    // The core reads the four parts after NUL bytes; a field with a NUL of its own makes five.
    QByteArray in = QByteArray::number(protocol);
    for (const QString &part : {server, folder, user}) {
        in.append('\0');
        in.append(part.toUtf8());
    }
    const QByteArray r = rustBytes(telamon_servers_build, in);
    const bool ok = r.startsWith('U');
    return {{QStringLiteral("ok"), ok}, {QStringLiteral("text"), QString::fromUtf8(r.mid(1))}};
}

QVariantMap ServerLogic::parse(const QUrl &url) const
{
    const QByteArray r = rustBytes(telamon_servers_parse, url.toString(QUrl::FullyEncoded | QUrl::RemovePassword).toUtf8());
    const QList<QByteArray> parts = r.split('\0');
    if (parts.size() != 4) {
        return {};
    }
    return {{QStringLiteral("protocol"), parts.at(0).toInt()},
            {QStringLiteral("server"), QString::fromUtf8(parts.at(1))},
            {QStringLiteral("folder"), QString::fromUtf8(parts.at(2))},
            {QStringLiteral("user"), QString::fromUtf8(parts.at(3))}};
}

QString ServerLogic::securityNote(const QUrl &url) const
{
    return QString::fromUtf8(rustBytes(telamon_servers_note, url.toString(QUrl::FullyEncoded | QUrl::RemovePassword).toUtf8()));
}

QVariantList ServerLogic::recents() const
{
    QVariantList out;
    for (const QByteArray &line : m_recent.split('\n')) {
        if (line.isEmpty()) {
            continue;
        }
        const QString label = rustDisplayName(rustBytes(telamon_servers_label, line));
        out.append(QVariantMap{{QStringLiteral("url"), QUrl::fromEncoded(line)}, {QStringLiteral("label"), label}});
    }
    return out;
}

void ServerLogic::remember(const QUrl &url)
{
    const QByteArray u = url.toString(QUrl::FullyEncoded | QUrl::RemovePassword).toUtf8();
    // An address that had a password lost it above; the core still refuses
    // anything that isn't one of its own.
    const QByteArray next = rustBytes2(telamon_servers_recent_push, m_recent, u);
    if (next != m_recent) {
        m_recent = next;
        save();
    }
}

void ServerLogic::forgetRecent(const QUrl &url)
{
    const QByteArray next = rustBytes2(telamon_servers_recent_remove, m_recent, url.toString(QUrl::FullyEncoded | QUrl::RemovePassword).toUtf8());
    if (next != m_recent) {
        m_recent = next;
        save();
    }
}

void ServerLogic::clearRecents()
{
    if (!m_recent.isEmpty()) {
        m_recent.clear();
        save();
    }
}

void ServerLogic::save()
{
    KConfigGroup g = serversGroup();
    QStringList lines;
    for (const QByteArray &l : m_recent.split('\n')) {
        if (!l.isEmpty()) {
            lines << QString::fromUtf8(l);
        }
    }
    if (lines.isEmpty()) {
        g.deleteEntry("Recent");
    } else {
        g.writeEntry("Recent", lines);
    }
    g.sync();
    ++m_revision;
    Q_EMIT recentChanged();
}
