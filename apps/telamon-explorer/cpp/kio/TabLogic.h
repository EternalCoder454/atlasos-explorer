// The tab strip's decisions (index arithmetic, limits, the saved session) from
// the Rust core, and the settings that keep the session. No tab lives here:
// the window owns them (qml/Main.qml, qml/FilesTabPage.qml).
#pragma once

#include <QObject>
#include <QQmlEngine>
#include <QStringList>
#include <QUrl>
#include <QVariantMap>

class TabLogic : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(int maxTabs READ maxTabs CONSTANT)
    Q_PROPERTY(int maxClosed READ maxClosed CONSTANT)
    Q_PROPERTY(int maxHistory READ maxHistory CONSTANT)

public:
    explicit TabLogic(QObject *parent = nullptr);

    int maxTabs() const;
    int maxClosed() const;
    int maxHistory() const;

    // The tab shown after `closed` is removed from `count` tabs; -1 when no
    // tab is left (or an index is out of range).
    Q_INVOKABLE int afterClose(int count, int current, int closed) const;
    Q_INVOKABLE int afterMove(int count, int current, int from, int to) const;
    // `step` places from `current`, wrapping around.
    Q_INVOKABLE int cycle(int count, int current, int step) const;
    // The tab for Alt+n (1 to 9, 9 is the last); -1 for none.
    Q_INVOKABLE int jump(int count, int n) const;
    Q_INVOKABLE int insertAfterOpener(int count, int opener, int run) const;
    Q_INVOKABLE int reopenIndex(int count, int original) const;

    // Whether Ctrl is held right now (a click or Enter with Ctrl opens a new tab).
    Q_INVOKABLE bool controlHeld() const;
    // A location as text for the saved session (percent-encoded, never "pretty").
    Q_INVOKABLE QString encode(const QUrl &url) const;

    // What the window saved for the next start, checked by the core: {urls, current}
    // with urls empty when there is nothing usable.
    Q_INVOKABLE QVariantMap checkSession(const QStringList &saved, int current) const;

    // The "Restore Tabs on Start" setting and the tabs kept for it.
    Q_INVOKABLE bool restoreOnStart() const;
    Q_INVOKABLE void setRestoreOnStart(bool on);
    Q_INVOKABLE QVariantMap savedSession() const;
    // Keeps `urls` for the next start; with the setting off nothing is kept.
    Q_INVOKABLE void saveSession(const QStringList &urls, int current);
};
