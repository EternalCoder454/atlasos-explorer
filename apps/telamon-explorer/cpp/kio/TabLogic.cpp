#include "TabLogic.h"

#include "RustBridge.h"

#include <climits>

#include <KConfigGroup>
#include <KSharedConfig>

#include <QGuiApplication>

namespace
{
constexpr uint32_t LimitTabs = 0, LimitClosed = 1, LimitHistory = 2;

// A Qt index as the core's: a negative one is out of range for it.
size_t idx(int i)
{
    return i < 0 ? SIZE_MAX : size_t(i);
}

// A Qt count as the core's: a negative one is none.
size_t cnt(int i)
{
    return i < 0 ? 0 : size_t(i);
}

int toInt(int64_t i)
{
    return i < 0 || i > INT_MAX ? -1 : int(i);
}

KConfigGroup tabsGroup()
{
    return KSharedConfig::openConfig(QStringLiteral("telamon-explorerrc"))->group(QStringLiteral("Tabs"));
}
}

TabLogic::TabLogic(QObject *parent)
    : QObject(parent)
{
}

int TabLogic::maxTabs() const
{
    return int(telamon_tabs_limit(LimitTabs));
}

int TabLogic::maxClosed() const
{
    return int(telamon_tabs_limit(LimitClosed));
}

int TabLogic::maxHistory() const
{
    return int(telamon_tabs_limit(LimitHistory));
}

int TabLogic::afterClose(int count, int current, int closed) const
{
    return toInt(telamon_tabs_after_close(cnt(count), idx(current), idx(closed)));
}

int TabLogic::afterMove(int count, int current, int from, int to) const
{
    return int(telamon_tabs_after_move(cnt(count), idx(current), idx(from), idx(to)));
}

int TabLogic::cycle(int count, int current, int step) const
{
    return int(telamon_tabs_cycle(cnt(count), idx(current), step));
}

int TabLogic::jump(int count, int n) const
{
    return toInt(telamon_tabs_jump(cnt(count), cnt(n)));
}

int TabLogic::insertAfterOpener(int count, int opener, int run) const
{
    return int(telamon_tabs_insert_after_opener(cnt(count), idx(opener), idx(run)));
}

int TabLogic::reopenIndex(int count, int original) const
{
    return int(telamon_tabs_reopen_index(cnt(count), cnt(original)));
}

bool TabLogic::controlHeld() const
{
    return QGuiApplication::keyboardModifiers().testFlag(Qt::ControlModifier);
}

QString TabLogic::encode(const QUrl &url) const
{
    // Never the password: the session lands in a settings file.
    return url.toString(QUrl::FullyEncoded | QUrl::RemovePassword);
}

QVariantMap TabLogic::checkSession(const QStringList &saved, int current) const
{
    const QByteArray in = saved.join(QLatin1Char('\n')).toUtf8();
    QByteArray buf(8192, 0);
    size_t cur = 0;
    auto call = [&] {
        return telamon_tabs_restore(reinterpret_cast<const uint8_t *>(in.constData()), size_t(in.size()), idx(current),
                                    reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()), &cur);
    };
    size_t n = call();
    if (n > size_t(buf.size())) {
        buf.resize(qsizetype(n));
        n = call();
    }
    QStringList urls;
    if (n > 0 && n <= size_t(buf.size())) {
        urls = QString::fromUtf8(buf.constData(), qsizetype(n)).split(QLatin1Char('\n'), Qt::SkipEmptyParts);
    }
    return {{QStringLiteral("urls"), urls}, {QStringLiteral("current"), urls.isEmpty() ? 0 : int(qMin(cur, size_t(urls.size() - 1)))}};
}

bool TabLogic::restoreOnStart() const
{
    return tabsGroup().readEntry("RestoreOnStart", false);
}

void TabLogic::setRestoreOnStart(bool on)
{
    KConfigGroup g = tabsGroup();
    g.writeEntry("RestoreOnStart", on);
    if (!on) {
        // Nothing stays behind that the user turned off.
        g.deleteEntry("Urls");
        g.deleteEntry("Current");
        g.deleteEntry("Splits");
        g.deleteEntry("Active");
    }
    g.sync();
}

QVariantMap TabLogic::savedSession() const
{
    const KConfigGroup g = tabsGroup();
    const QStringList saved = g.readEntry("Urls", QStringList());
    const QStringList splits = g.readEntry("Splits", QStringList());
    const QList<int> active = g.readEntry("Active", QList<int>());
    const int current = g.readEntry("Current", 0);
    // Each entry is checked on its own, so that a tab and its second pane
    // stay together when another tab's entry is refused.
    QStringList urls, kept;
    QList<int> keptActive;
    int shown = -1;
    for (int i = 0; i < saved.size() && urls.size() < maxTabs(); ++i) {
        const QStringList one = checkSession({saved.at(i)}, 0).value(QStringLiteral("urls")).toStringList();
        if (one.size() != 1) {
            continue;
        }
        if (shown < 0 && i >= current) {
            shown = int(urls.size());
        }
        urls << one.first();
        QString split;
        if (!splits.value(i).isEmpty()) {
            split = checkSession({splits.at(i)}, 0).value(QStringLiteral("urls")).toStringList().value(0);
        }
        kept << split;
        keptActive << (split.isEmpty() ? 0 : qBound(0, active.value(i), 1));
    }
    if (urls.isEmpty()) {
        return {{QStringLiteral("urls"), QStringList()}, {QStringLiteral("current"), 0}, {QStringLiteral("splits"), QStringList()}, {QStringLiteral("active"), QVariantList()}};
    }
    QVariantList act;
    for (int a : std::as_const(keptActive)) {
        act << a;
    }
    return {{QStringLiteral("urls"), urls},
            {QStringLiteral("current"), shown < 0 ? int(urls.size()) - 1 : shown},
            {QStringLiteral("splits"), kept},
            {QStringLiteral("active"), act}};
}

void TabLogic::saveSession(const QStringList &urls, int current, const QStringList &splits, const QList<int> &active)
{
    if (!restoreOnStart()) {
        return;
    }
    KConfigGroup g = tabsGroup();
    g.writeEntry("Urls", urls.mid(0, maxTabs()));
    g.writeEntry("Current", current);
    g.writeEntry("Splits", splits.mid(0, maxTabs()));
    g.writeEntry("Active", active.mid(0, maxTabs()));
    g.sync();
}
