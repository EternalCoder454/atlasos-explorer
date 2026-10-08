#include "SearchController.h"

#include "RustBridge.h"

#include <KIO/Global>
#include <KIO/Job>
#include <KIO/ListJob>
#include <KIO/UDSEntry>

#include <QCoreApplication>
#include <QDBusArgument>
#include <QDBusConnection>
#include <QDBusError>
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QDate>
#include <QDateTime>
#include <QDir>
#include <QElapsedTimer>
#include <QGuiApplication>
#include <QFile>
#include <QLoggingCategory>
#include <QMetaObject>
#include <QQuickWindow>
#include <QThreadPool>

#include <cstring>
#include <memory>
#include <utility>

namespace
{
Q_LOGGING_CATEGORY(lcSearch, "telamon.explorer.search", QtInfoMsg)

const QString ServiceName = QStringLiteral("net.eterneon.telamon.explorer.Search");
const QString ObjectPath = QStringLiteral("/net/eterneon/telamon/explorer/Search");
const QString Interface = QStringLiteral("net.eterneon.telamon.explorer.Search1");
// How long the service gets to answer; the first call may have to start it.
constexpr int CallTimeoutMs = 10000;

TelamonSearchFilter makeFilter(int kind, int modified, int size)
{
    const QDateTime now = QDateTime::currentDateTime();
    TelamonSearchFilter f{};
    telamon_search_filter(uint32_t(kind), uint32_t(modified), uint32_t(size), now.toSecsSinceEpoch(), QDate::currentDate().startOfDay().toSecsSinceEpoch(), &f);
    return f;
}

QStringList kindNames(int kind)
{
    QByteArray buf(128, 0);
    size_t n = telamon_search_kinds(uint32_t(kind), reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()));
    if (n > size_t(buf.size())) {
        buf.resize(qsizetype(n));
        n = telamon_search_kinds(uint32_t(kind), reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()));
    }
    const QString all = QString::fromUtf8(buf.constData(), qsizetype(qMin(n, size_t(buf.size()))));
    return all.isEmpty() ? QStringList() : all.split(QLatin1Char('\n'));
}

// The URL of a local file from the `file://` URI the index (or a walk) gives:
// made like the lister makes it (from the decoded path), so the same file is
// the same URL however its name was written.
QUrl localUrlOf(const QByteArray &uri)
{
    static const QByteArray prefix("file://");
    if (!uri.startsWith(prefix)) {
        return QUrl();
    }
    QByteArray path = uri.mid(prefix.size());
    if (path.startsWith("localhost/")) {
        path = path.mid(int(sizeof("localhost") - 1));
    }
    if (!path.startsWith('/')) {
        return QUrl();
    }
    return QUrl::fromLocalFile(QFile::decodeName(QByteArray::fromPercentEncoding(path)));
}

// The last part of an encoded URL's path, as a name safe to show.
QString nameOfEncoded(const QByteArray &encoded)
{
    QByteArray path = encoded;
    while (path.endsWith('/')) {
        path.chop(1);
    }
    const QByteArray last = path.mid(path.lastIndexOf('/') + 1);
    return rustDisplayName(QByteArray::fromPercentEncoding(last));
}
}

// ---- SearchService ----

SearchService *SearchService::instance()
{
    static SearchService *s = new SearchService(QCoreApplication::instance());
    return s;
}

SearchService::SearchService(QObject *parent)
    : QObject(parent)
{
}

QStringList SearchService::roots() const
{
    return m_roots.isEmpty() ? QStringList{QDir::homePath()} : m_roots;
}

void SearchService::subscribe()
{
    if (m_subscribed) {
        return;
    }
    m_subscribed = true;
    QDBusConnection::sessionBus().connect(ServiceName, ObjectPath, Interface, QStringLiteral("StatusChanged"), this, SLOT(onStatusChanged(QVariantMap)));
}

void SearchService::onStatusChanged(const QVariantMap &status)
{
    apply(status);
}

void SearchService::apply(const QVariantMap &status)
{
    const QByteArray state = status.value(QStringLiteral("state")).toString().toUtf8();
    const State s = State(telamon_search_index_state(reinterpret_cast<const uint8_t *>(state.constData()), size_t(state.size())));
    const QString error = status.value(QStringLiteral("error")).toString();
    QStringList roots;
    QStringList uris = status.value(QStringLiteral("roots")).toStringList();
    if (uris.isEmpty() && status.value(QStringLiteral("roots")).canConvert<QDBusArgument>()) {
        uris = qdbus_cast<QStringList>(status.value(QStringLiteral("roots")).value<QDBusArgument>());
    }
    for (const QString &u : std::as_const(uris)) {
        const QUrl url = QUrl::fromEncoded(u.toUtf8());
        if (url.isLocalFile()) {
            roots << url.toLocalFile();
        }
    }
    if (s != m_state || error != m_error || roots != m_roots) {
        m_state = s;
        m_error = error;
        m_roots = roots;
        Q_EMIT stateChanged();
    }
}

void SearchService::setUnavailable()
{
    if (m_state != Unavailable) {
        m_state = Unavailable;
        m_error.clear();
        Q_EMIT stateChanged();
    }
}

void SearchService::refreshStatus(QObject *context, std::function<void()> done)
{
    subscribe();
    if (done) {
        QPointer<QObject> ctx(context);
        m_waiting.append([ctx, done = std::move(done)] {
            if (ctx) {
                done();
            }
        });
    }
    if (m_asking) {
        return;
    }
    m_asking = true;
    const QDBusMessage msg = QDBusMessage::createMethodCall(ServiceName, ObjectPath, Interface, QStringLiteral("Status"));
    auto *watcher = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(msg, CallTimeoutMs), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher *w) {
        w->deleteLater();
        m_asking = false;
        const QDBusMessage reply = w->reply();
        if (reply.type() == QDBusMessage::ReplyMessage && !reply.arguments().isEmpty()) {
            apply(qdbus_cast<QVariantMap>(reply.arguments().first().value<QDBusArgument>()));
        } else {
            qCInfo(lcSearch) << "Status failed:" << reply.errorName();
            setUnavailable();
        }
        const auto waiting = std::exchange(m_waiting, {});
        for (const auto &fn : waiting) {
            fn();
        }
    });
}

void SearchService::search(const QString &query, uint limit, const QVariantMap &options, QObject *context, SearchDone done)
{
    subscribe();
    QDBusMessage msg = QDBusMessage::createMethodCall(ServiceName, ObjectPath, Interface, QStringLiteral("Search"));
    msg.setArguments({query, limit, options});
    QPointer<QObject> ctx(context);
    QElapsedTimer asked;
    asked.start();
    auto *watcher = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(msg, CallTimeoutMs), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [this, ctx, asked, done = std::move(done)](QDBusPendingCallWatcher *w) {
        w->deleteLater();
        if (!ctx) {
            return;
        }
        qCDebug(lcSearch) << "Search answered in" << asked.elapsed() << "ms";
        const QDBusMessage reply = w->reply();
        if (reply.type() != QDBusMessage::ReplyMessage || reply.arguments().isEmpty()) {
            qCInfo(lcSearch) << "Search failed:" << reply.errorName();
            if (reply.errorName() == QLatin1String("org.freedesktop.DBus.Error.LimitsExceeded")) {
                done(Busy, {});
                return;
            }
            setUnavailable();
            done(Unreachable, {});
            return;
        }
        QList<FolderModel::SearchHit> hits;
        const QDBusArgument arg = reply.arguments().first().value<QDBusArgument>();
        arg.beginArray();
        while (!arg.atEnd()) {
            QString uri, name, kind, mime, icon;
            qlonglong mtime = 0;
            qulonglong size = 0;
            double score = 0;
            arg.beginStructure();
            arg >> uri >> name >> kind >> mime >> icon >> mtime >> size >> score;
            arg.endStructure();
            const QUrl url = localUrlOf(uri.toUtf8());
            // The index holds this computer's files; anything else is not a hit.
            if (!url.isValid() || !url.isLocalFile()) {
                continue;
            }
            hits.append({url, name, kind == QLatin1String("folder"), quint64(size), qint64(mtime)});
        }
        arg.endArray();
        // An answer means it is there: learn its state if it was not known.
        if (!known()) {
            refreshStatus(this);
        }
        done(NoFailure, hits);
    });
}

// ---- SearchController ----

// What a walk's thread knows: the controller (which may be gone by then) and
// the search it belongs to. Freed by the walk's thread when it ends.
struct SearchController::WalkContext {
    QPointer<SearchController> controller;
    quint64 serial;
};

namespace
{
void walkCallback(void *user, const uint8_t *batch, size_t len, uint32_t end)
{
    auto *ctx = static_cast<SearchController::WalkContext *>(user);
    // The application may be gone already (the window was closed during a walk).
    if (!QCoreApplication::instance()) {
        if (end != 0) {
            delete ctx;
        }
        return;
    }
    QByteArray data = batch ? QByteArray(reinterpret_cast<const char *>(batch), qsizetype(len)) : QByteArray();
    QMetaObject::invokeMethod(
        QCoreApplication::instance(),
        [ctl = ctx->controller, serial = ctx->serial, data = std::move(data), end] {
            if (ctl) {
                ctl->onWalkFromThread(serial, data, end);
            }
        },
        Qt::QueuedConnection);
    if (end != 0) {
        delete ctx;
    }
}
}

SearchController::SearchController(QObject *parent)
    : QObject(parent)
{
    m_kick.setSingleShot(true);
    m_kick.setInterval(0);
    connect(&m_kick, &QTimer::timeout, this, &SearchController::run);
    m_lastState = SearchService::instance()->state();
    connect(SearchService::instance(), &SearchService::stateChanged, this, [this] {
        refreshChip();
        // The index has caught up: the answer to the last search may have grown.
        const SearchService::State now = SearchService::instance()->state();
        if (m_lastState == SearchService::Updating && now == SearchService::Ready && active() && !live()) {
            rerun();
        }
        m_lastState = now;
    });
    refreshChip();
}

SearchController::~SearchController()
{
    stopLive();
}

bool SearchController::active() const
{
    return !m_text.trimmed().isEmpty() || m_kind != 0 || m_modified != 0 || m_size != 0;
}

void SearchController::setFolder(FolderModel *f)
{
    if (m_folder == f) {
        return;
    }
    if (m_folder) {
        m_folder->disconnect(this);
    }
    m_folder = f;
    if (f) {
        connect(f, &FolderModel::searchingChanged, this, [this] {
            // The folder ended the search (the tab went somewhere): so do we.
            if (m_folder && !m_folder->searching() && !m_resetting && (active() || m_route != NoRoute)) {
                stopLive();
                ++m_serial;
                resetState();
            }
        });
        connect(f, &FolderModel::searchRefreshRequested, this, &SearchController::rerun);
        connect(f, &FolderModel::showHiddenChanged, this, [this] {
            if (active()) {
                rerun();
            }
        });
    }
    Q_EMIT folderChanged();
}

void SearchController::changed()
{
    m_typed.restart();
    const bool a = active();
    if (a != m_wasActive) {
        m_wasActive = a;
        Q_EMIT activeChanged();
    }
    m_kick.start();
}

void SearchController::setText(const QString &t)
{
    if (t == m_text) {
        return;
    }
    m_text = t;
    Q_EMIT textChanged();
    changed();
}

void SearchController::setScope(int s)
{
    s = s == 1 ? 1 : 0;
    if (s == m_scope) {
        return;
    }
    m_scope = s;
    Q_EMIT scopeChanged();
    if (active()) {
        changed();
    }
}

void SearchController::setKind(int k)
{
    k = qBound(0, k, 7);
    if (k == m_kind) {
        return;
    }
    m_kind = k;
    Q_EMIT kindChanged();
    changed();
}

void SearchController::setModified(int m)
{
    m = qBound(0, m, 4);
    if (m == m_modified) {
        return;
    }
    m_modified = m;
    Q_EMIT modifiedChanged();
    changed();
}

void SearchController::setSize(int s)
{
    s = qBound(0, s, 3);
    if (s == m_size) {
        return;
    }
    m_size = s;
    Q_EMIT sizeChanged();
    changed();
}

void SearchController::clear()
{
    m_text.clear();
    m_kind = m_modified = m_size = 0;
    Q_EMIT textChanged();
    Q_EMIT kindChanged();
    Q_EMIT modifiedChanged();
    Q_EMIT sizeChanged();
    const bool a = active();
    if (a != m_wasActive) {
        m_wasActive = a;
        Q_EMIT activeChanged();
    }
    run();
}

// The state back to nothing, without touching the folder (it has left search itself).
void SearchController::resetState()
{
    m_kick.stop();
    const bool hadText = !m_text.isEmpty();
    m_text.clear();
    const bool chips = m_kind || m_modified || m_size;
    m_kind = m_modified = m_size = 0;
    if (hadText) {
        Q_EMIT textChanged();
    }
    if (chips) {
        Q_EMIT kindChanged();
        Q_EMIT modifiedChanged();
        Q_EMIT sizeChanged();
    }
    if (m_wasActive) {
        m_wasActive = false;
        Q_EMIT activeChanged();
    }
    setRoute(NoRoute);
    setWalking(false);
    setPending(false);
    m_found = 0;
    setStatusText(QString());
    setFailure(QString(), QString());
}

void SearchController::rerun()
{
    if (active()) {
        m_kick.start();
    }
}

QString SearchController::sizeHint(int size) const
{
    const auto text = [](size_t which) { return KIO::convertSize(KIO::filesize_t(telamon_search_limit(uint32_t(which)))); };
    switch (size) {
    case 1:
        return tr("Small (under %1)").arg(text(2));
    case 2:
        return tr("Medium (%1 to %2)").arg(text(2), text(3));
    case 3:
        return tr("Large (over %1)").arg(text(3));
    default:
        return QString();
    }
}

void SearchController::warm()
{
    SearchService::instance()->refreshStatus(this);
}

void SearchController::stop()
{
    if (!m_walking) {
        return;
    }
    stopLive();
    ++m_serial;
    m_stopped = true;
    showLive(false, true, false);
}

void SearchController::stopLive()
{
    if (m_walk) {
        telamon_walk_stop(m_walk);
        telamon_walk_free(m_walk);
        m_walk = nullptr;
    }
    if (m_job) {
        m_job->kill(KJob::Quietly);
        m_job = nullptr;
    }
    if (m_matcher) {
        telamon_matcher_free(m_matcher);
        m_matcher = nullptr;
    }
    setWalking(false);
    if (m_folder) {
        m_folder->setSearchBusy(false);
    }
}

void SearchController::setRoute(Route r)
{
    if (m_route != r) {
        const bool wasLive = live();
        m_route = r;
        Q_EMIT routeChanged();
        if (wasLive != live()) {
            refreshChip();
        }
    }
}

void SearchController::setPending(bool on)
{
    if (m_pending != on) {
        m_pending = on;
        Q_EMIT pendingChanged();
    }
}

void SearchController::setWalking(bool on)
{
    if (m_walking != on) {
        m_walking = on;
        Q_EMIT walkingChanged();
    }
}

void SearchController::refreshChip()
{
    int level = 0;
    QString text;
    if (!live()) {
        SearchService *svc = SearchService::instance();
        const QByteArray err = svc->errorText().toUtf8();
        QByteArray buf(256, 0);
        uint32_t lv = 0;
        auto call = [&] {
            return telamon_search_chip(uint32_t(svc->state()), reinterpret_cast<const uint8_t *>(err.constData()), size_t(err.size()),
                                     reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()), &lv);
        };
        size_t n = call();
        if (n > size_t(buf.size())) {
            buf.resize(qsizetype(n));
            n = call();
        }
        text = QString::fromUtf8(buf.constData(), qsizetype(qMin(n, size_t(buf.size()))));
        level = int(lv);
    }
    if (level != m_chipLevel || text != m_chipText) {
        m_chipLevel = level;
        m_chipText = text;
        Q_EMIT chipChanged();
    }
}

void SearchController::setStatusText(const QString &text)
{
    if (text != m_statusText) {
        m_statusText = text;
    }
    Q_EMIT statusTextChanged();
}

void SearchController::setFailure(const QString &title, const QString &text)
{
    if (title != m_failureTitle || text != m_failureText) {
        m_failureTitle = title;
        m_failureText = text;
        Q_EMIT failureChanged();
    }
}

void SearchController::showCount(int n, bool capped)
{
    setStatusText(rustSearchText(0, quint64(n), capped ? 1 : 0));
}

void SearchController::showLive(bool running, bool stopped, bool capped)
{
    setStatusText(rustSearchText(1, quint64(m_found), (running ? 1 : 0) | (stopped ? 2 : 0) | (capped ? 4 : 0)));
}

// ---- Running a search ----

void SearchController::run()
{
    m_kick.stop();
    stopLive();
    const quint64 serial = ++m_serial;
    m_stopped = false;
    m_found = 0;
    m_busyRetries = 0;
    if (!m_folder) {
        return;
    }
    if (!active()) {
        if (m_folder->searching()) {
            m_resetting = true;
            m_folder->endSearch();
            m_resetting = false;
        }
        setRoute(NoRoute);
        setPending(false);
        setStatusText(QString());
        setFailure(QString(), QString());
        return;
    }
    m_folder->beginSearch();
    setFailure(QString(), QString());
    setPending(true);
    startRoute(serial);
}

// The folder a search of "this folder" looks in: on Files' own pages (Home,
// Network) there is no folder to look in, so it is the home folder.
QUrl SearchController::searchFolder() const
{
    if (m_folder && !m_folder->pageOfUrl().isEmpty()) {
        return QUrl::fromLocalFile(QDir::homePath());
    }
    return m_folder ? m_folder->url() : QUrl();
}

void SearchController::startRoute(quint64 serial)
{
    SearchService *svc = SearchService::instance();
    auto go = [this, serial] {
        if (serial != m_serial || !m_folder) {
            return;
        }
        SearchService *svc = SearchService::instance();
        const QUrl folder = searchFolder();
        // Whether the index holds the folder is asked of a worker: it reads a few folders.
        if (m_scope == 0 && folder.isLocalFile() && telamon_search_index_on(uint32_t(svc->state()))) {
            const QString path = folder.toLocalFile();
            const QString roots = svc->roots().join(QLatin1Char('\n'));
            QPointer<SearchController> self(this);
            QThreadPool::globalInstance()->start([self, serial, path, roots] {
                const QByteArray p = path.toUtf8(), r = roots.toUtf8();
                const bool covered = telamon_search_covers(reinterpret_cast<const uint8_t *>(p.constData()), size_t(p.size()),
                                                         reinterpret_cast<const uint8_t *>(r.constData()), size_t(r.size()));
                QMetaObject::invokeMethod(
                    QCoreApplication::instance(),
                    [self, serial, covered] {
                        if (self) {
                            self->routeNow(serial, covered);
                        }
                    },
                    Qt::QueuedConnection);
            });
        } else {
            routeNow(serial, false);
        }
    };
    if (svc->known()) {
        go();
    } else {
        // The first search, or the service was not there: ask it for its state
        // first (which also starts it).
        svc->refreshStatus(this, go);
    }
}

void SearchController::routeNow(quint64 serial, bool covered)
{
    if (serial != m_serial || !m_folder) {
        return;
    }
    SearchService *svc = SearchService::instance();
    const QUrl folder = searchFolder();
    const Route r = Route(telamon_search_route(uint32_t(m_scope), folder.isLocalFile(), covered, telamon_search_index_on(uint32_t(svc->state()))));
    setRoute(r);
    switch (r) {
    case IndexEverywhere:
    case IndexFolder:
        searchIndex(serial, r);
        break;
    case LiveFolder:
        startWalk(serial, folder);
        break;
    case LiveHome:
        startWalk(serial, QUrl::fromLocalFile(QDir::homePath()));
        break;
    case LiveRemote:
        startKio(serial, folder);
        break;
    case NoRoute:
        break;
    }
}

void SearchController::searchIndex(quint64 serial, Route route)
{
    const TelamonSearchFilter f = makeFilter(m_kind, m_modified, m_size);
    QVariantMap o;
    const QStringList kinds = kindNames(m_kind);
    if (!kinds.isEmpty()) {
        o.insert(QStringLiteral("kinds"), kinds);
    }
    if (f.files_only) {
        o.insert(QStringLiteral("kind"), QStringLiteral("file"));
    }
    if (f.has_after) {
        o.insert(QStringLiteral("modified_after"), qlonglong(f.after));
    }
    if (f.has_min) {
        o.insert(QStringLiteral("size_min"), qulonglong(f.min));
    }
    if (f.has_max) {
        o.insert(QStringLiteral("size_max"), qulonglong(f.max));
    }
    if (m_folder && m_folder->showHidden()) {
        o.insert(QStringLiteral("include_hidden"), true);
    }
    if (route == IndexFolder && m_folder) {
        o.insert(QStringLiteral("root"), QString::fromLatin1(searchFolder().toEncoded()));
    }
    const uint limit = uint(telamon_search_limit(0));
    SearchService::instance()->search(m_text.trimmed(), limit, o, this, [this, serial, route, limit](SearchService::Failure failure, const QList<FolderModel::SearchHit> &hits) {
        if (serial != m_serial || !m_folder) {
            return; // an answer to an older search
        }
        if (failure == SearchService::Busy) {
            // The service is answering as many as it allows: try again in a moment.
            // At most a few seconds of it: then it is a failure like any other.
            if (++m_busyRetries <= 20) {
                QTimer::singleShot(150, this, [this, serial, route] {
                    if (serial == m_serial) {
                        searchIndex(serial, route);
                    }
                });
                return;
            }
            failure = SearchService::Unreachable;
        }
        setPending(false);
        if (failure != SearchService::NoFailure) {
            m_folder->setSearchResults({});
            setFailure(rustSearchText(2), rustSearchText(3));
            setStatusText(rustSearchText(4));
            return;
        }
        m_folder->setSearchResults(hits);
        m_found = int(hits.size());
        showCount(m_found, hits.size() >= qsizetype(limit));
        m_lastMs = m_typed.isValid() ? int(m_typed.elapsed()) : -1;
        qCDebug(lcSearch).nospace() << "search \"" << m_text << "\": " << hits.size() << " hits in " << m_lastMs << " ms";
        if (lcSearch().isDebugEnabled()) {
            // The same until the first frame that shows them (for the smoke test's latency number).
            if (auto *window = qobject_cast<QQuickWindow *>(QGuiApplication::focusWindow())) {
                const QElapsedTimer typed = m_typed;
                const QString text = m_text;
                connect(window, &QQuickWindow::frameSwapped, this, [typed, text] { qCDebug(lcSearch).nospace() << "search \"" << text << "\": shown after " << typed.elapsed() << " ms"; }, Qt::SingleShotConnection);
            }
        }
    });
}

void SearchController::startWalk(quint64 serial, const QUrl &root)
{
    const TelamonSearchFilter f = makeFilter(m_kind, m_modified, m_size);
    const QByteArray path = QFile::encodeName(root.toLocalFile());
    const QByteArray query = m_text.trimmed().toUtf8();
    // A new walk starts from no results.
    m_folder->setSearchResults({});
    auto *ctx = new WalkContext{QPointer<SearchController>(this), serial};
    m_walk = telamon_walk_start(reinterpret_cast<const uint8_t *>(path.constData()), size_t(path.size()), reinterpret_cast<const uint8_t *>(query.constData()),
                                size_t(query.size()), &f, m_folder && m_folder->showHidden(), telamon_search_limit(1), &walkCallback, ctx);
    if (!m_walk) {
        delete ctx;
        setPending(false);
        setFailure(tr("Can't Search This Folder"), tr("Files couldn't start looking through it."));
        return;
    }
    setPending(false);
    setWalking(true);
    m_folder->setSearchBusy(true);
    showLive(true, false, false);
}

// Hits from the walk's thread: a batch of records, then one call with its end.
void SearchController::onWalkFromThread(quint64 serial, const QByteArray &batch, uint end)
{
    if (serial != m_serial || !m_folder) {
        return; // the search was stopped or replaced
    }
    QList<FolderModel::SearchHit> hits;
    const char *p = batch.constData();
    qsizetype left = batch.size();
    while (left >= 22) {
        const bool isDir = p[0] != 0;
        quint64 size;
        qint64 mtime;
        quint32 n;
        std::memcpy(&size, p + 2, 8);
        std::memcpy(&mtime, p + 10, 8);
        std::memcpy(&n, p + 18, 4);
        if (qsizetype(n) > left - 22) {
            break;
        }
        const QByteArray uri(p + 22, qsizetype(n));
        const QUrl url = localUrlOf(uri);
        if (url.isValid()) {
            hits.append({url, nameOfEncoded(uri), isDir, isDir ? 0 : size, mtime});
        }
        p += 22 + n;
        left -= 22 + qsizetype(n);
    }
    if (!hits.isEmpty()) {
        m_folder->appendSearchResults(hits);
        m_found += int(hits.size());
    }
    if (end == 0) {
        showLive(true, false, false);
        return;
    }
    if (m_walk) {
        telamon_walk_free(m_walk);
        m_walk = nullptr;
    }
    setWalking(false);
    m_folder->setSearchBusy(false);
    if (end == 4 && m_found == 0) {
        setFailure(tr("Can't Search This Folder"), tr("You don't have permission to look through this folder, or it isn't there."));
    }
    showLive(false, false, end == 3);
    m_lastMs = m_typed.isValid() ? int(m_typed.elapsed()) : -1;
}

void SearchController::startKio(quint64 serial, const QUrl &root)
{
    const TelamonSearchFilter f = makeFilter(m_kind, m_modified, m_size);
    const bool hidden = m_folder && m_folder->showHidden();
    const QByteArray query = m_text.trimmed().toUtf8();
    m_matcher = telamon_matcher_new(reinterpret_cast<const uint8_t *>(query.constData()), size_t(query.size()), &f, hidden);
    const size_t limit = telamon_search_limit(1);
    m_folder->setSearchResults({});
    KIO::ListJob *job = KIO::listRecursive(root, KIO::HideProgressInfo, hidden ? KIO::ListJob::ListFlag::IncludeHidden : KIO::ListJob::ListFlags());
    m_job = job;
    setPending(false);
    setWalking(true);
    m_folder->setSearchBusy(true);
    showLive(true, false, false);
    connect(job, &KIO::ListJob::entries, this, [this, serial, root, limit](KIO::Job *, const KIO::UDSEntryList &list) {
        if (serial != m_serial || !m_folder || !m_matcher) {
            return;
        }
        QList<FolderModel::SearchHit> hits;
        for (const KIO::UDSEntry &e : list) {
            const QString rel = e.stringValue(KIO::UDSEntry::UDS_NAME);
            if (rel.isEmpty() || rel == QLatin1String(".") || rel == QLatin1String("..")) {
                continue;
            }
            const QString base = rel.mid(rel.lastIndexOf(QLatin1Char('/')) + 1);
            const QByteArray name = base.toUtf8();
            const bool isDir = e.isDir();
            const quint64 size = isDir ? 0 : quint64(e.numberValue(KIO::UDSEntry::UDS_SIZE, 0));
            const qint64 mtime = e.numberValue(KIO::UDSEntry::UDS_MODIFICATION_TIME, 0);
            if (telamon_matcher_test(m_matcher, reinterpret_cast<const uint8_t *>(name.constData()), size_t(name.size()), isDir, size, mtime) == 0) {
                continue;
            }
            QUrl url = root;
            url.setPath(QDir::cleanPath(root.path() + QLatin1Char('/') + rel));
            hits.append({url, rustDisplayName(name), isDir, size, mtime});
            if (size_t(m_found + hits.size()) >= limit) {
                break;
            }
        }
        if (!hits.isEmpty()) {
            m_folder->appendSearchResults(hits);
            m_found += int(hits.size());
        }
        showLive(true, false, false);
        if (size_t(m_found) >= limit && m_job) {
            m_job->kill(KJob::Quietly);
            m_job = nullptr;
            setWalking(false);
            m_folder->setSearchBusy(false);
            showLive(false, false, true);
        }
    });
    connect(job, &KJob::result, this, [this, serial](KJob *j) {
        if (serial != m_serial || !m_folder) {
            return;
        }
        m_job = nullptr;
        setWalking(false);
        m_folder->setSearchBusy(false);
        if (j->error() && m_found == 0) {
            setFailure(tr("Can't Search This Folder"), tr("This location couldn't be searched."));
        }
        showLive(false, false, false);
        m_lastMs = m_typed.isValid() ? int(m_typed.elapsed()) : -1;
    });
}
