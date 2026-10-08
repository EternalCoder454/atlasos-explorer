#include "ArchiveClient.h"

#include "RustBridge.h"

#include <KIO/Global>

#include <QCoreApplication>
#include <QDBusArgument>
#include <QDBusConnection>
#include <QDBusError>
#include <QDBusObjectPath>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QDBusServiceWatcher>

namespace ArchiveBus
{
QList<Target> targets()
{
    return {
        {QStringLiteral("net.eterneon.telamon.archive"), QStringLiteral("/net/eterneon/telamon/archive"), QStringLiteral("net.eterneon.telamon.Archive1")},
        {QStringLiteral("net.eterneon.atlas.archive"), QStringLiteral("/net/eterneon/atlas/archive"), QStringLiteral("net.eterneon.atlas.Archive1")},
    };
}

QString busyText()
{
    return QCoreApplication::translate("ArchiveBus", "Archive is busy, try again when a job finishes.");
}

QString notRunningText()
{
    return QCoreApplication::translate("ArchiveBus", "Telamon Archive could not be started.");
}

Failure failureOf(const QString &errorName, const QString &message)
{
    Failure f;
    if (errorName.endsWith(QLatin1String(".TooManyJobs"))) {
        f.busy = true;
        f.text = busyText();
    } else if (errorName == QLatin1String("org.freedesktop.DBus.Error.ServiceUnknown") || errorName == QLatin1String("org.freedesktop.DBus.Error.NameHasNoOwner")
               || errorName == QLatin1String("org.freedesktop.DBus.Error.NoServer") || errorName == QLatin1String("org.freedesktop.DBus.Error.Disconnected")) {
        f.notRunning = true;
        f.text = notRunningText();
    } else if (errorName.endsWith(QLatin1String(".InvalidArgs"))) {
        f.text = QCoreApplication::translate("ArchiveBus", "Telamon Archive can't use these files. It opens only files on this computer.");
    } else if (errorName == QLatin1String("org.freedesktop.DBus.Error.NoReply") || errorName == QLatin1String("org.freedesktop.DBus.Error.Timeout")
               || errorName == QLatin1String("org.freedesktop.DBus.Error.TimedOut")) {
        f.text = QCoreApplication::translate("ArchiveBus", "Telamon Archive did not answer.");
    } else {
        f.text = QCoreApplication::translate("ArchiveBus", "Telamon Archive could not do this: %1").arg(rustDisplayName(message.toUtf8()));
    }
    return f;
}

namespace
{
// Tries Archive's names in turn; the next only when the one asked has no
// owner, and any other answer is the answer.
class CallChain : public QObject
{
public:
    CallChain(QObject *context, const QString &method, const QVariantList &args,
              std::function<void(const QDBusMessage &, const Target &, const Failure &)> done)
        : QObject(context)
        , m_method(method)
        , m_args(args)
        , m_done(std::move(done))
        , m_targets(targets())
    {
    }

    void next()
    {
        if (m_at >= m_targets.size()) {
            Failure f;
            f.notRunning = true;
            f.text = notRunningText();
            finish({}, {}, f);
            return;
        }
        const Target t = m_targets.at(m_at);
        QDBusMessage call = QDBusMessage::createMethodCall(t.service, t.path, t.iface, m_method);
        call.setArguments(m_args);
        auto *w = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(call, 10000), this);
        connect(w, &QDBusPendingCallWatcher::finished, this, [this, t, w] {
            const QDBusMessage reply = w->reply();
            const bool failed = w->isError();
            const QDBusError error = w->error();
            w->deleteLater();
            if (!failed) {
                finish(reply, t, {});
                return;
            }
            const Failure f = failureOf(error.name(), error.message());
            if (f.notRunning && m_at + 1 < m_targets.size()) {
                ++m_at;
                next();
                return;
            }
            finish({}, t, f);
        });
    }

private:
    void finish(const QDBusMessage &reply, const Target &t, const Failure &f)
    {
        auto done = std::move(m_done);
        deleteLater();
        if (done) {
            done(reply, t, f);
        }
    }

    QString m_method;
    QVariantList m_args;
    std::function<void(const QDBusMessage &, const Target &, const Failure &)> m_done;
    QList<Target> m_targets;
    int m_at = 0;
};
}

void call(QObject *context, const QString &method, const QVariantList &args, std::function<void(const QDBusMessage &, const Target &, const Failure &)> done)
{
    (new CallChain(context, method, args, std::move(done)))->next();
}
}

namespace
{
bool isTerminal(const QString &state)
{
    return state == QLatin1String("done") || state == QLatin1String("failed") || state == QLatin1String("cancelled");
}

QStringList stringList(const QVariant &v)
{
    if (v.canConvert<QDBusArgument>()) {
        return qdbus_cast<QStringList>(v);
    }
    return v.toStringList();
}

QVariantMap propertyMap(const QVariant &v)
{
    if (v.canConvert<QDBusArgument>()) {
        return qdbus_cast<QVariantMap>(v);
    }
    return v.toMap();
}
}

ArchiveJob::ArchiveJob(const QString &method, const QVariantList &args, const QVariantMap &options, QObject *parent)
    : KJob(parent)
    , m_method(method)
    , m_args(args)
    , m_options(options)
{
    m_lateFinish.setSingleShot(true);
    m_lateFinish.setInterval(1500);
    connect(&m_lateFinish, &QTimer::timeout, this, [this] {
        // The Job object said it is over, and its Finished never came.
        finish(m_state, {});
    });
    // Starts itself, as KIO's jobs do (the queue only attaches to it).
    QMetaObject::invokeMethod(this, &ArchiveJob::start, Qt::QueuedConnection);
}

ArchiveJob::~ArchiveJob() = default;

void ArchiveJob::start()
{
    if (m_asked || m_finished) {
        return;
    }
    m_asked = true;
    begin();
}

void ArchiveJob::begin()
{
    // Finished is listened for before the call: a short job can end before the
    // reply that names it has been read.
    for (const ArchiveBus::Target &t : ArchiveBus::targets()) {
        QDBusConnection::sessionBus().connect(t.service, QString(), t.jobIface(), QStringLiteral("Finished"), this, SLOT(onFinishedMessage(QDBusMessage)));
    }
    QVariantList args = m_args;
    args << m_options;
    QPointer<ArchiveJob> self(this);
    const auto shared = m_shared;
    ArchiveBus::call(qApp, m_method, args, [self, shared](const QDBusMessage &reply, const ArchiveBus::Target &target, const ArchiveBus::Failure &failure) {
        if (!failure.text.isEmpty()) {
            if (self) {
                self->failWith(failure.text);
            }
            return;
        }
        QString path;
        if (!reply.arguments().isEmpty() && reply.arguments().first().canConvert<QDBusObjectPath>()) {
            path = reply.arguments().first().value<QDBusObjectPath>().path();
        }
        shared->target = target;
        shared->path = path;
        // Asked to stop before the job had a name: stop it now.
        if (shared->cancel) {
            if (!path.isEmpty()) {
                send(target, path, QStringLiteral("Cancel"));
            }
            return;
        }
        if (!self) {
            // The job was thrown away (killed): don't leave Archive's running.
            if (!path.isEmpty()) {
                send(target, path, QStringLiteral("Cancel"));
            }
            return;
        }
        if (path.isEmpty()) {
            // A call that makes no job (a dialog): nothing to follow.
            self->finish(QStringLiteral("done"), {});
            return;
        }
        self->attachTo(path);
        if (shared->pause) {
            send(target, path, QStringLiteral("Pause"));
        }
    });
}

void ArchiveJob::attachTo(const QString &path)
{
    const ArchiveBus::Target &t = m_shared->target;
    QDBusConnection bus = QDBusConnection::sessionBus();
    bus.connect(t.service, path, QStringLiteral("org.freedesktop.DBus.Properties"), QStringLiteral("PropertiesChanged"), this,
                SLOT(onPropertiesChanged(QString, QVariantMap, QStringList)));
    // Archive going away takes the job with it.
    auto *watcher = new QDBusServiceWatcher(t.service, bus, QDBusServiceWatcher::WatchForUnregistration, this);
    connect(watcher, &QDBusServiceWatcher::serviceUnregistered, this, [this] {
        if (m_finished) {
            return;
        }
        // Archive may leave as soon as it is done: what it already said stands.
        if (isTerminal(m_state)) {
            finish(m_state, {});
        } else {
            failWith(tr("Telamon Archive stopped before it finished."));
        }
    });
    // Finished messages that were early.
    const QList<QDBusMessage> early = std::exchange(m_early, {});
    for (const QDBusMessage &m : early) {
        onFinishedMessage(m);
    }
    if (m_finished) {
        return;
    }
    QDBusMessage get = QDBusMessage::createMethodCall(t.service, path, QStringLiteral("org.freedesktop.DBus.Properties"), QStringLiteral("GetAll"));
    get << t.jobIface();
    auto *w = new QDBusPendingCallWatcher(bus.asyncCall(get, 5000), this);
    connect(w, &QDBusPendingCallWatcher::finished, this, [this, w] {
        const QDBusMessage reply = w->reply();
        const bool ok = !w->isError() && !reply.arguments().isEmpty();
        w->deleteLater();
        if (ok && !m_finished) {
            readProperties(propertyMap(reply.arguments().first()));
        }
    });
}

void ArchiveJob::onPropertiesChanged(const QString &, const QVariantMap &changed, const QStringList &)
{
    if (!m_finished) {
        readProperties(changed);
    }
}

void ArchiveJob::readProperties(const QVariantMap &props)
{
    if (props.contains(QStringLiteral("Error"))) {
        m_error = props.value(QStringLiteral("Error")).toString();
    }
    if (props.contains(QStringLiteral("TotalBytes"))) {
        setTotalAmount(KJob::Bytes, props.value(QStringLiteral("TotalBytes")).toULongLong());
    }
    if (props.contains(QStringLiteral("ProcessedBytes"))) {
        setProcessedAmount(KJob::Bytes, props.value(QStringLiteral("ProcessedBytes")).toULongLong());
    }
    if (props.contains(QStringLiteral("TotalItems"))) {
        setTotalAmount(KJob::Files, props.value(QStringLiteral("TotalItems")).toULongLong());
    }
    if (props.contains(QStringLiteral("ProcessedItems"))) {
        setProcessedAmount(KJob::Files, props.value(QStringLiteral("ProcessedItems")).toULongLong());
    }
    if (props.contains(QStringLiteral("State"))) {
        m_state = props.value(QStringLiteral("State")).toString();
        if (m_state == QLatin1String("waiting-for-user")) {
            Q_EMIT needsUser();
        }
        if (isTerminal(m_state) && !m_lateFinish.isActive()) {
            m_lateFinish.start();
        }
    }
}

void ArchiveJob::onFinishedMessage(const QDBusMessage &message)
{
    if (m_finished) {
        return;
    }
    if (m_shared->path.isEmpty()) {
        // The call has not returned its path yet: keep it until it does.
        // Other jobs of Archive's end here too: only the shape of ours is kept.
        if (m_early.size() < 256 && message.arguments().size() >= 2) {
            m_early.append(message);
        }
        return;
    }
    if (message.path() != m_shared->path || message.arguments().size() < 2) {
        return;
    }
    finish(message.arguments().at(0).toString(), stringList(message.arguments().at(1)));
}

void ArchiveJob::finish(const QString &state, const QStringList &results)
{
    if (m_finished) {
        return;
    }
    m_finished = true;
    m_lateFinish.stop();
    for (const QString &r : results.mid(0, 1000)) {
        const QUrl u = QUrl::fromEncoded(r.toUtf8());
        if (u.isLocalFile()) {
            m_results << u;
        }
    }
    if (state == QLatin1String("done")) {
        m_reported = true;
        emitResult();
    } else if (state == QLatin1String("cancelled")) {
        m_reported = true;
        setError(KIO::ERR_USER_CANCELED);
        emitResult();
    } else {
        // The reason is in the job's Error property, which is read again
        // here: the signal can come before the property's change.
        const ArchiveBus::Target t = m_shared->target;
        const QString path = m_shared->path;
        if (m_error.isEmpty() && !path.isEmpty()) {
            QDBusMessage get = QDBusMessage::createMethodCall(t.service, path, QStringLiteral("org.freedesktop.DBus.Properties"), QStringLiteral("Get"));
            get << t.jobIface() << QStringLiteral("Error");
            auto *w = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(get, 3000), this);
            connect(w, &QDBusPendingCallWatcher::finished, this, [this, w] {
                const QDBusMessage reply = w->reply();
                if (!w->isError() && !reply.arguments().isEmpty()) {
                    m_error = reply.arguments().first().value<QDBusVariant>().variant().toString();
                }
                w->deleteLater();
                failWith(m_error);
            });
            return;
        }
        failWith(m_error);
    }
}

void ArchiveJob::failWith(const QString &text)
{
    if (m_reported) {
        return;
    }
    m_reported = true;
    m_finished = true;
    m_lateFinish.stop();
    setError(KJob::UserDefinedError);
    // Archive's words can hold names from the archive: made safe to show.
    setErrorText(text.isEmpty() ? tr("Telamon Archive did not say why.") : rustDisplayName(text.toUtf8()));
    emitResult();
}

void ArchiveJob::send(const ArchiveBus::Target &target, const QString &path, const QString &method)
{
    QDBusMessage m = QDBusMessage::createMethodCall(target.service, path, target.jobIface(), method);
    // No answer is waited for: Archive does what it can.
    QDBusConnection::sessionBus().asyncCall(m, 5000);
}

bool ArchiveJob::doKill()
{
    m_shared->cancel = true;
    m_finished = true;
    m_reported = true;
    m_lateFinish.stop();
    if (!m_shared->path.isEmpty()) {
        send(m_shared->target, m_shared->path, QStringLiteral("Cancel"));
    }
    return true;
}

bool ArchiveJob::doSuspend()
{
    m_shared->pause = true;
    if (!m_shared->path.isEmpty()) {
        send(m_shared->target, m_shared->path, QStringLiteral("Pause"));
    }
    return true;
}

bool ArchiveJob::doResume()
{
    m_shared->pause = false;
    if (!m_shared->path.isEmpty()) {
        send(m_shared->target, m_shared->path, QStringLiteral("Resume"));
    }
    return true;
}
