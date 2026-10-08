#include "TrashLogic.h"

#include "RustBridge.h"

#include <KConfigGroup>
#include <KSharedConfig>

#include <QDateTime>
#include <QFile>
#include <QPointer>
#include <QStandardPaths>
#include <QThreadPool>
#include <QTimeZone>

namespace
{
constexpr int StartDelayMs = 4000;
constexpr int DayMs = 24 * 60 * 60 * 1000;

KConfigGroup trashGroup()
{
    return KSharedConfig::openConfig(QStringLiteral("telamon-explorerrc"))->group(QStringLiteral("Trash"));
}
}

TrashLogic::TrashLogic(QObject *parent)
    : QObject(parent)
{
    const KConfigGroup g = trashGroup();
    m_on = g.readEntry("AutoEmpty", false);
    // A settings file can hold anything: the core brings the days into their limits.
    m_days = int(telamon_trash_clamp_days(g.readEntry("AutoEmptyDays", int(telamon_trash_limit(0)))));
    m_askDays = m_days;
    // A day; the tests shorten it (TELAMON_EXPLORER_TEST_TRASH_TICK_MS) to see the second run.
    bool shortened = false;
    const int tick = qEnvironmentVariableIntValue("TELAMON_EXPLORER_TEST_TRASH_TICK_MS", &shortened);
    m_daily.setInterval(shortened && tick >= 200 ? tick : DayMs);
    m_daily.setTimerType(Qt::VeryCoarseTimer);
    connect(&m_daily, &QTimer::timeout, this, [this] {
        if (m_on) {
            Q_EMIT emptyOldRequested(m_days);
        }
    });
}

int TrashLogic::minDays() const
{
    return int(telamon_trash_limit(1));
}

int TrashLogic::maxDays() const
{
    return int(telamon_trash_limit(2));
}

void TrashLogic::begin()
{
    schedule();
    if (m_on) {
        // Not while the window is still being made.
        QPointer<TrashLogic> self(this);
        QTimer::singleShot(StartDelayMs, this, [self] {
            if (self && self->m_on) {
                Q_EMIT self->emptyOldRequested(self->m_days);
            }
        });
    }
}

// The daily timer runs only while the switch is on.
void TrashLogic::schedule()
{
    if (m_on) {
        if (!m_daily.isActive()) {
            m_daily.start();
        }
    } else {
        m_daily.stop();
    }
}

void TrashLogic::apply(bool on, int days)
{
    days = int(telamon_trash_clamp_days(days));
    KConfigGroup g = trashGroup();
    g.writeEntry("AutoEmpty", on);
    g.writeEntry("AutoEmptyDays", days);
    g.sync();
    m_on = on;
    m_days = days;
    schedule();
    Q_EMIT autoEmptyChanged();
}

void TrashLogic::requestAutoEmpty(bool on)
{
    if (on == m_on) {
        return;
    }
    if (!on) {
        apply(false, m_days);
        return;
    }
    ask(true, m_days);
}

void TrashLogic::requestDays(int days)
{
    days = int(telamon_trash_clamp_days(days));
    if (days == m_days) {
        return;
    }
    if (!m_on || days > m_days) {
        // Nothing more can go because of this: kept, and used at the next run.
        apply(m_on, days);
        return;
    }
    ask(true, days);
}

// Counts what would go now (nothing is changed), then asks.
void TrashLogic::ask(bool on, int days)
{
    m_askOn = on;
    m_askDays = days;
    const quint64 serial = ++m_askSerial;
    const QDateTime now = QDateTime::currentDateTime();
    const qint64 local = QDateTime(now.date(), now.time(), QTimeZone::UTC).toSecsSinceEpoch();
    const QByteArray dataHome = QFile::encodeName(QStandardPaths::writableLocation(QStandardPaths::GenericDataLocation));
    const bool alreadyOn = m_on;
    QPointer<TrashLogic> self(this);
    QThreadPool::globalInstance()->start([self, serial, days, local, dataHome, alreadyOn] {
        TelamonTrashReport report{};
        const bool ok = telamon_trash_purge(quint32(days), local, 0, reinterpret_cast<const uint8_t *>(dataHome.constData()), size_t(dataHome.size()), &report);
        if (!self) {
            return;
        }
        const quint64 would = ok ? quint64(report.removed) : 0;
        QMetaObject::invokeMethod(self.data(), [self, serial, days, would, alreadyOn] {
            if (!self || serial != self->m_askSerial) {
                return;
            }
            // Lowering the days while on: nothing to ask when nothing would go.
            if (alreadyOn && would == 0) {
                self->apply(true, days);
                return;
            }
            Q_EMIT self->confirmRequested(rustTrashText(2, QString(), quint64(days) | (would << 32)));
        });
    });
}

void TrashLogic::confirm()
{
    ++m_askSerial;
    apply(m_askOn, m_askDays);
    if (m_on) {
        Q_EMIT emptyOldRequested(m_days);
    }
}

void TrashLogic::cancel()
{
    // The controls show what is kept.
    ++m_askSerial;
    Q_EMIT autoEmptyChanged();
}
