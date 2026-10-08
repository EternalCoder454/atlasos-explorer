// What each folder remembers of how it is shown (view, sort, icon size,
// grouping), kept in telamon-explorerrc and never in the folder: a bounded
// list of the most recently changed folders in [FolderViews], and the view
// that folders without one are shown in (or all of them, with "Use the same
// view for every folder") in [View]. The list's rules (its bound, bringing
// numbers into their limits, what a key may be) are the core's
// (atlas_explorer_core::views); this class reads and writes the file.
#pragma once

#include <QObject>
#include <QQmlEngine>
#include <QTimer>
#include <QUrl>
#include <QVariantMap>

class ViewMemory : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    // One view for every folder: changes go to the shared view and the
    // folders' own are not used (they are kept for when this is turned off).
    Q_PROPERTY(bool sameForAll READ sameForAll WRITE setSameForAll NOTIFY sameForAllChanged)
    // Bumped when a folder's remembered view was added or forgotten.
    Q_PROPERTY(int revision READ revision NOTIFY revisionChanged)

public:
    explicit ViewMemory(QObject *parent = nullptr);
    ~ViewMemory() override;

    bool sameForAll() const { return m_same; }
    void setSameForAll(bool on);
    int revision() const { return m_revision; }

    // How `folder` is shown: {mode ("details", "icons", "compact", "columns",
    // "gallery"), sort (the core's column number), descending, icon (pixels),
    // group (0 none, 1 name, 2 type, 3 modified), remembered}.
    Q_INVOKABLE QVariantMap prefsFor(const QUrl &folder) const;
    // `folder` was changed to this: kept for it (for every folder, with
    // sameForAll). A folder that cannot be a key is not kept.
    Q_INVOKABLE void remember(const QUrl &folder, const QString &mode, int sort, bool descending, int icon, int group);
    // Forgets what `folder` remembered, so it is shown as the shared view says.
    Q_INVOKABLE void forget(const QUrl &folder);
    Q_INVOKABLE bool hasEntry(const QUrl &folder) const;
    Q_INVOKABLE int entryCount() const;
    // The icon size of folders without one, and a size moved by `steps` (within the limits).
    Q_INVOKABLE int iconStep(int current, int steps) const;
    Q_INVOKABLE int iconDefault() const;
    // A wheel movement (angleDelta.y) with Ctrl held: the whole steps it adds up to.
    Q_INVOKABLE int iconWheelSteps(int delta);
    // The list is written now (it is also written shortly after a change, and at exit).
    Q_INVOKABLE void flush();

Q_SIGNALS:
    void sameForAllChanged();
    void revisionChanged();

private:
    QByteArray key(const QUrl &folder) const;
    QVariantMap shared() const;
    void touch();

    // The list as the core writes it, one folder per line.
    QByteArray m_text;
    bool m_same = false;
    bool m_dirty = false;
    int m_wheelRest = 0;
    int m_revision = 0;
    QTimer m_timer;
    // The view of folders without one (and of all, with sameForAll).
    QString m_mode;
    int m_sort = 0;
    bool m_desc = false;
    int m_icon = 0;
    int m_group = 0;
};
