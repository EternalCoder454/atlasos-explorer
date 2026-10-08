#include "NetworkModel.h"
#include "RustBridge.h"
#include "ServerLogic.h"

#include <KIO/Job>
#include <KIO/ListJob>
#include <KIO/UDSEntry>
#include <KProtocolInfo>

#include <QDBusConnection>
#include <QDBusConnectionInterface>
#include <QDBusMessage>
#include <QDBusPendingCall>
#include <QDBusPendingCallWatcher>

namespace
{
const QString AvahiService = QStringLiteral("org.freedesktop.Avahi");
const QString AvahiServer = QStringLiteral("org.freedesktop.Avahi.Server");
const QString AvahiBrowser = QStringLiteral("org.freedesktop.Avahi.ServiceBrowser");

// A scan ends after this long (computers found later are still added while the page is open).
constexpr int ScanMs = 10000;
// No more than this many computers are listed: a hostile network can announce anything.
constexpr int MaxRows = 500;
constexpr int NoInterface = -1;
constexpr int NoProtocol = -1;

struct Service {
    const char *type;
    // The core's protocol code (Sftp 0, Smb 1, Ftp 2, Webdavs 3, Webdav 4, Nfs 5).
    int protocol;
};

const Service Services[] = {
    {"_smb._tcp", 1},
    {"_sftp-ssh._tcp", 0},
    {"_ssh._tcp", 0},
    {"_ftp._tcp", 2},
    {"_webdavs._tcp", 3},
    {"_webdav._tcp", 4},
    {"_nfs._tcp", 5},
};

const Service *serviceOf(const QString &type)
{
    for (const Service &s : Services) {
        if (type == QLatin1String(s.type)) {
            return &s;
        }
    }
    return nullptr;
}

QString protocolLabel(int code)
{
    QByteArray buf(64, 0);
    size_t n = telamon_servers_protocol(uint32_t(code), 1, reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()));
    n = qMin(n, size_t(buf.size()));
    return QString::fromUtf8(buf.constData(), qsizetype(n));
}
}

NetworkModel::NetworkModel(QObject *parent)
    : QAbstractListModel(parent)
{
    m_deadline.setSingleShot(true);
    m_deadline.setInterval(ScanMs);
    connect(&m_deadline, &QTimer::timeout, this, [this] {
        // Avahi and the SMB worker had their time: the spinner goes. Avahi's
        // browsers stay, so a computer that switches on later shows up.
        if (m_smb) {
            m_smb->kill(KJob::Quietly);
            m_smb = nullptr;
        }
        setScanning(false);
    });
}

NetworkModel::~NetworkModel()
{
    stop();
    freeBrowsers();
}

int NetworkModel::rowCount(const QModelIndex &parent) const
{
    return parent.isValid() ? 0 : int(m_rows.size());
}

QHash<int, QByteArray> NetworkModel::roleNames() const
{
    return {{NameRole, "computerName"}, {UrlRole, "computerUrl"}, {KindRole, "computerKind"}, {IconNameRole, "computerIcon"}};
}

QVariant NetworkModel::data(const QModelIndex &index, int role) const
{
    if (!index.isValid() || index.row() < 0 || index.row() >= m_rows.size()) {
        return {};
    }
    const Row &r = m_rows.at(index.row());
    switch (role) {
    case Qt::DisplayRole:
    case NameRole:
        return r.name;
    case UrlRole:
        return r.url;
    case KindRole:
        return r.kind;
    case IconNameRole:
        return r.icon;
    default:
        return {};
    }
}

void NetworkModel::setScanning(bool on)
{
    if (m_scanning != on) {
        m_scanning = on;
        Q_EMIT scanningChanged();
    }
}

void NetworkModel::start()
{
    stop();
    if (!m_rows.isEmpty()) {
        beginResetModel();
        m_rows.clear();
        m_urls.clear();
        endResetModel();
        Q_EMIT countChanged();
    }
    ++m_generation;
    setScanning(true);
    m_deadline.start();
    const bool avahi = startAvahi();
    startSmb();
    if (!avahi && !m_smb) {
        setScanning(false);
        m_deadline.stop();
    }
}

void NetworkModel::stop()
{
    m_deadline.stop();
    ++m_generation;
    m_listening = false;
    if (m_smb) {
        m_smb->kill(KJob::Quietly);
        m_smb = nullptr;
    }
    freeBrowsers();
    setScanning(false);
}

// Asks Avahi for the computers offering each kind of service. The signals are
// taken for any browser (Avahi sends a client's signals to that client only),
// so none is missed between the call and its answer.
bool NetworkModel::startAvahi()
{
    QDBusConnection bus = QDBusConnection::systemBus();
    m_avahiOk = bus.isConnected() && bus.interface() && bus.interface()->isServiceRegistered(AvahiService);
    if (!m_avahiOk) {
        return false;
    }
    m_listening = true;
    if (!m_avahiConnected) {
        m_avahiConnected = true;
        bus.connect(AvahiService, QString(), AvahiBrowser, QStringLiteral("ItemNew"), this, SLOT(onItemNew(QDBusMessage)));
        bus.connect(AvahiService, QString(), AvahiBrowser, QStringLiteral("ItemRemove"), this, SLOT(onItemRemove(QDBusMessage)));
        bus.connect(AvahiService, QString(), AvahiBrowser, QStringLiteral("AllForNow"), this, SLOT(onAllForNow(QDBusMessage)));
        bus.connect(AvahiService, QString(), AvahiBrowser, QStringLiteral("Failure"), this, SLOT(onFailure(QDBusMessage)));
    }
    m_browsersAsked = 0;
    m_browsersDone = 0;
    m_early.clear();
    const int generation = m_generation;
    for (const Service &s : Services) {
        QDBusMessage call = QDBusMessage::createMethodCall(AvahiService, QStringLiteral("/"), AvahiServer, QStringLiteral("ServiceBrowserNew"));
        call << NoInterface << NoProtocol << QString::fromLatin1(s.type) << QString() << uint(0);
        ++m_browsersAsked;
        auto *watcher = new QDBusPendingCallWatcher(bus.asyncCall(call, 5000), this);
        connect(watcher, &QDBusPendingCallWatcher::finished, this, [this, watcher, generation] {
            watcher->deleteLater();
            const QDBusMessage reply = watcher->reply();
            if (generation != m_generation) {
                // The scan was stopped meanwhile: the browser is let go at once.
                if (reply.type() == QDBusMessage::ReplyMessage && !reply.arguments().isEmpty()) {
                    QDBusMessage free = QDBusMessage::createMethodCall(AvahiService, reply.arguments().first().value<QDBusObjectPath>().path(), AvahiBrowser,
                                                                       QStringLiteral("Free"));
                    QDBusConnection::systemBus().asyncCall(free, 2000);
                }
                return;
            }
            if (reply.type() != QDBusMessage::ReplyMessage || reply.arguments().isEmpty()) {
                ++m_browsersDone;
                maybeDone();
                return;
            }
            const QString path = reply.arguments().first().value<QDBusObjectPath>().path();
            m_browsers.insert(path);
            if (m_early.contains(path)) {
                ++m_browsersDone;
                maybeDone();
            }
        });
    }
    return true;
}

void NetworkModel::freeBrowsers()
{
    for (const QString &path : std::as_const(m_browsers)) {
        QDBusMessage free = QDBusMessage::createMethodCall(AvahiService, path, AvahiBrowser, QStringLiteral("Free"));
        QDBusConnection::systemBus().asyncCall(free, 2000);
    }
    m_browsers.clear();
    m_early.clear();
}

// Windows computers as the SMB worker finds them (`smb:/`). A hung or absent
// network ends with the scan's time, and the job never asks for a password.
void NetworkModel::startSmb()
{
    if (!KProtocolInfo::isKnownProtocol(QStringLiteral("smb"))) {
        return;
    }
    auto *job = KIO::listDir(QUrl(QStringLiteral("smb:/")), KIO::HideProgressInfo);
    // A scan never opens a password dialog.
    job->setUiDelegate(nullptr);
    m_smb = job;
    const int generation = m_generation;
    connect(job, &KIO::ListJob::entries, this, [this, generation](KIO::Job *, const KIO::UDSEntryList &list) {
        if (generation != m_generation) {
            return;
        }
        for (const KIO::UDSEntry &e : list) {
            const QString name = e.stringValue(KIO::UDSEntry::UDS_NAME);
            if (name.isEmpty() || name.startsWith(QLatin1Char('.')) || !e.isDir()) {
                continue;
            }
            // The core makes the address, and refuses a name that isn't a host name.
            const QVariantMap built = ServerLogic::buildFor(1, name);
            if (!built.value(QStringLiteral("ok")).toBool()) {
                continue;
            }
            Row row;
            row.key = QStringLiteral("smb\n") + name;
            row.name = rustDisplayName(name.toUtf8());
            row.url = QUrl::fromEncoded(built.value(QStringLiteral("text")).toString().toUtf8());
            row.kind = protocolLabel(1);
            row.icon = QStringLiteral("network-server");
            addRow(row);
        }
    });
    connect(job, &KJob::result, this, [this, generation] {
        if (generation == m_generation) {
            m_smb = nullptr;
            maybeDone();
        }
    });
}

void NetworkModel::maybeDone()
{
    if (m_scanning && !m_smb && m_browsersDone >= m_browsersAsked) {
        m_deadline.stop();
        setScanning(false);
    }
}

void NetworkModel::addRow(const Row &row)
{
    // The same computer is found by more than one way (Avahi and the SMB
    // worker; "nas" and "nas.local"): one row for a name and kind.
    const QString same = row.kind + QLatin1Char('\n') + row.name.toLower();
    if (m_rows.size() >= MaxRows || m_urls.contains(row.url.toString()) || m_urls.contains(same)) {
        return;
    }
    m_urls.insert(row.url.toString());
    m_urls.insert(same);
    const int at = int(m_rows.size());
    beginInsertRows({}, at, at);
    m_rows.append(row);
    endInsertRows();
    Q_EMIT countChanged();
}

void NetworkModel::onItemNew(const QDBusMessage &msg)
{
    const QList<QVariant> a = msg.arguments();
    // interface, protocol, name, type, domain, flags
    if (a.size() < 6 || !m_listening) {
        return;
    }
    const QString name = a.at(2).toString();
    const QString type = a.at(3).toString();
    const QString domain = a.at(4).toString();
    const Service *svc = serviceOf(type);
    if (!svc || name.isEmpty() || name.size() > 255) {
        return;
    }
    QDBusMessage call = QDBusMessage::createMethodCall(AvahiService, QStringLiteral("/"), AvahiServer, QStringLiteral("ResolveService"));
    call << a.at(0).toInt() << a.at(1).toInt() << name << type << domain << NoProtocol << uint(0);
    const int generation = m_generation;
    const int protocol = svc->protocol;
    auto *watcher = new QDBusPendingCallWatcher(QDBusConnection::systemBus().asyncCall(call, 8000), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [this, watcher, generation, protocol, name, type] {
        watcher->deleteLater();
        if (generation != m_generation) {
            return;
        }
        const QDBusMessage reply = watcher->reply();
        const QList<QVariant> r = reply.arguments();
        // interface, protocol, name, type, domain, host, aprotocol, address, port, txt, flags
        if (reply.type() != QDBusMessage::ReplyMessage || r.size() < 9) {
            return;
        }
        QString host = r.at(5).toString();
        const uint port = r.at(8).toUInt();
        if (host.endsWith(QLatin1Char('.'))) {
            host.chop(1);
        }
        const QString address = r.at(7).toString();
        if (host.isEmpty()) {
            host = address;
        }
        // The address Files opens is the core's: a host name that isn't one is refused there.
        QVariantMap built = ServerLogic::buildFor(protocol, port > 0 && port < 65536 ? QStringLiteral("%1:%2").arg(host).arg(port) : host);
        if (!built.value(QStringLiteral("ok")).toBool() && address != host) {
            built = ServerLogic::buildFor(protocol, port > 0 && port < 65536 ? QStringLiteral("%1:%2").arg(address, QString::number(port)) : address);
        }
        if (!built.value(QStringLiteral("ok")).toBool()) {
            return;
        }
        QString shown = host;
        if (shown.endsWith(QStringLiteral(".local"))) {
            shown.chop(6);
        }
        Row row;
        row.key = name + QLatin1Char('\n') + type;
        row.name = rustDisplayName(shown.toUtf8());
        row.url = QUrl::fromEncoded(built.value(QStringLiteral("text")).toString().toUtf8());
        row.kind = protocolLabel(protocol);
        row.icon = QStringLiteral("network-server");
        addRow(row);
    });
}

void NetworkModel::onItemRemove(const QDBusMessage &msg)
{
    const QList<QVariant> a = msg.arguments();
    if (a.size() < 6) {
        return;
    }
    const QString key = a.at(2).toString() + QLatin1Char('\n') + a.at(3).toString();
    for (int i = 0; i < m_rows.size(); ++i) {
        if (m_rows.at(i).key == key) {
            beginRemoveRows({}, i, i);
            m_urls.remove(m_rows.at(i).url.toString());
            m_urls.remove(m_rows.at(i).kind + QLatin1Char('\n') + m_rows.at(i).name.toLower());
            m_rows.removeAt(i);
            endRemoveRows();
            Q_EMIT countChanged();
            return;
        }
    }
}

void NetworkModel::onAllForNow(const QDBusMessage &msg)
{
    const QString path = msg.path();
    if (m_browsers.contains(path)) {
        ++m_browsersDone;
        maybeDone();
    } else {
        // Said before the call that made the browser was answered.
        m_early.insert(path);
    }
}

void NetworkModel::onFailure(const QDBusMessage &msg)
{
    onAllForNow(msg);
}
