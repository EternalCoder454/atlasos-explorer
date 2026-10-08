// A drag of files over the window, as the window sees it: where the pointer
// is, which keys are held, and so what a drop would do (the badge that follows
// the pointer says it), and the spring-loaded folders: hold a drag over a
// folder, a path segment, a place or a tab for a second and it opens. The
// drop areas of QML each know their own target, so they tell this object
// (`mode`, `spring`); the pointer's place and keys are read from the drag
// events themselves, with an event filter on the window, so that targets
// QML has no drop area for (the sidebar's places) can be found by position.
// The drag itself is FileActions::startDrag's.
#pragma once

#include <QJSValue>
#include <QObject>
#include <QPointer>
#include <QQmlEngine>
#include <QQuickWindow>
#include <QTimer>

class DragWatch : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    // A drag of files is over the window.
    Q_PROPERTY(bool active READ active NOTIFY changed)
    // Where the pointer is, in the window's coordinates.
    Q_PROPERTY(qreal x READ x NOTIFY moved)
    Q_PROPERTY(qreal y READ y NOTIFY moved)
    // "ask": a drop with no key held asks what to do (Move Here, Copy Here,
    // Link Here); "direct": it moves, or copies with Ctrl held. The target
    // under the pointer says which.
    Q_PROPERTY(QString mode READ mode WRITE setMode NOTIFY changed)
    // What a drop would do now: "move", "copy", "link" or "ask"; and as the badge says it.
    Q_PROPERTY(QString kind READ kind NOTIFY changed)
    Q_PROPERTY(QString verb READ verb NOTIFY changed)
    // How long a drag rests on a folder before it opens, in milliseconds.
    Q_PROPERTY(int springDelay READ springDelay CONSTANT)

public:
    explicit DragWatch(QObject *parent = nullptr);

    bool active() const { return m_active; }
    qreal x() const { return m_pos.x(); }
    qreal y() const { return m_pos.y(); }
    QString mode() const { return m_mode; }
    void setMode(const QString &mode);
    QString kind() const;
    QString verb() const;
    int springDelay() const;

    // Starts watching the drags over `window` (the main window; once).
    Q_INVOKABLE void attach(QQuickWindow *window);
    // The drag rests on the target `key` (any text that names it; the same
    // while it stays there): after the delay `callback` is called, once.
    Q_INVOKABLE void spring(const QString &key, const QJSValue &callback);
    // The drag left the target `key` (or any, when empty).
    Q_INVOKABLE void springClear(const QString &key = {});

Q_SIGNALS:
    void changed();
    void moved();

protected:
    bool eventFilter(QObject *watched, QEvent *event) override;

private:
    void update(const QPointF &pos);
    void end();

    QPointer<QQuickWindow> m_window;
    bool m_active = false;
    bool m_fromArchive = false;
    QPointF m_pos;
    Qt::KeyboardModifiers m_mods;
    QString m_mode = QStringLiteral("ask");
    QTimer m_timer;
    QString m_key;
    QJSValue m_callback;
};
