#include "DragWatch.h"

#include "RustBridge.h"

#include <QDragEnterEvent>
#include <QDragLeaveEvent>
#include <QDragMoveEvent>
#include <QDropEvent>
#include <QGuiApplication>
#include <QMimeData>

namespace
{
// How long a drag rests on a folder before it opens: the same for the view,
// the sidebar, the path bar and the tabs.
constexpr int SpringMs = 1000;
}

DragWatch::DragWatch(QObject *parent)
    : QObject(parent)
{
    m_timer.setSingleShot(true);
    m_timer.setInterval(SpringMs);
    connect(&m_timer, &QTimer::timeout, this, [this] {
        // Called once: the same target does not open again until the drag has been elsewhere.
        QJSValue cb = m_callback;
        m_callback = QJSValue();
        if (m_active && cb.isCallable()) {
            cb.call();
        }
    });
}

void DragWatch::setMode(const QString &mode)
{
    if (mode == m_mode) {
        return;
    }
    m_mode = mode;
    Q_EMIT changed();
}

int DragWatch::springDelay() const
{
    return SpringMs;
}

QString DragWatch::kind() const
{
    // What is in an archive can only be copied out of it.
    if (m_fromArchive) {
        return QStringLiteral("copy");
    }
    const bool ctrl = m_mods.testFlag(Qt::ControlModifier), shift = m_mods.testFlag(Qt::ShiftModifier);
    if (m_mode == QLatin1String("direct")) {
        return ctrl ? QStringLiteral("copy") : QStringLiteral("move");
    }
    if (ctrl && shift) {
        return QStringLiteral("link");
    }
    if (ctrl) {
        return QStringLiteral("copy");
    }
    if (shift) {
        return QStringLiteral("move");
    }
    return QStringLiteral("ask");
}

QString DragWatch::verb() const
{
    const QString k = kind();
    if (k == QLatin1String("copy")) {
        return tr("Copy");
    }
    if (k == QLatin1String("move")) {
        return tr("Move");
    }
    if (k == QLatin1String("link")) {
        return tr("Link");
    }
    return tr("Move, Copy or Link");
}

void DragWatch::attach(QQuickWindow *window)
{
    if (!window || m_window) {
        return;
    }
    m_window = window;
    window->installEventFilter(this);
}

void DragWatch::spring(const QString &key, const QJSValue &callback)
{
    if (key == m_key) {
        return;
    }
    m_key = key;
    m_callback = callback;
    m_timer.start();
}

void DragWatch::springClear(const QString &key)
{
    if (!key.isEmpty() && key != m_key) {
        return;
    }
    m_key.clear();
    m_callback = QJSValue();
    m_timer.stop();
}

void DragWatch::update(const QPointF &pos)
{
    // During a drag the application's own record of the keys is not updated:
    // the system is asked.
    const Qt::KeyboardModifiers mods = QGuiApplication::queryKeyboardModifiers();
    const bool posChanged = pos != m_pos;
    const bool modsChanged = mods != m_mods;
    m_pos = pos;
    m_mods = mods;
    // The targets under the pointer say again what a drop on them does (the
    // path bar's segments and the sidebar's places move, the rest ask).
    const bool modeChanged = m_mode != QLatin1String("ask");
    m_mode = QStringLiteral("ask");
    if (posChanged) {
        Q_EMIT moved();
    }
    if (modsChanged || modeChanged) {
        Q_EMIT changed();
    }
}

void DragWatch::end()
{
    const bool was = m_active;
    m_active = false;
    m_fromArchive = false;
    m_mode = QStringLiteral("ask");
    springClear();
    if (was) {
        Q_EMIT changed();
    }
}

bool DragWatch::eventFilter(QObject *watched, QEvent *event)
{
    if (watched != m_window) {
        return false;
    }
    switch (event->type()) {
    case QEvent::DragEnter: {
        auto *e = static_cast<QDragEnterEvent *>(event);
        if (!e->mimeData() || !e->mimeData()->hasUrls()) {
            break;
        }
        m_fromArchive = false;
        for (const QUrl &u : e->mimeData()->urls()) {
            m_fromArchive = m_fromArchive || rustArchiveScheme(u.scheme());
        }
        m_active = true;
        m_mode = QStringLiteral("ask");
        update(e->position());
        Q_EMIT changed();
        break;
    }
    case QEvent::DragMove: {
        auto *e = static_cast<QDragMoveEvent *>(event);
        if (m_active) {
            update(e->position());
        }
        break;
    }
    case QEvent::DragLeave:
        end();
        break;
    case QEvent::Drop:
        // After the drop areas have had it.
        QTimer::singleShot(0, this, &DragWatch::end);
        break;
    default:
        break;
    }
    return false;
}
