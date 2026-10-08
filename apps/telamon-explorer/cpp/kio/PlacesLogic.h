// The sidebar's places: one KIO `KFilePlacesModel` (so pins live in
// `user-places.xbel` and show in every Open and Save dialog; drives come from
// Solid), turned into entries the sidebar draws, plus everything the sidebar
// does with them: open (mounting a drive first), unmount, rename, hide,
// remove, reorder, pin a dropped folder, count the Trash and empty it, open a
// drive in Telamon Disks. What decides (the section of a place, its menu, the
// texts) is in the Rust core (`places`); this file only moves data between
// KDE's classes and the core. KIO and Solid run asynchronously on the GUI
// thread; the disk usage is read on a worker.
#pragma once

#include <QElapsedTimer>
#include <QHash>
#include <QList>
#include <QObject>
#include <QPointer>
#include <QQmlEngine>
#include <QTimer>
#include <QUrl>
#include <QVariantMap>

class KCoreDirLister;
class KFilePlacesModel;

// One place as the sidebar shows it.
struct PlaceEntry {
    QString key; // the device's UDI, else the bookmark's ID: stable across changes
    QString text; // the name, made safe to show
    QString iconName;
    QUrl url; // where it opens; invalid for a drive that is not mounted
    QUrl rawUrl; // the bookmark's own URL (what editing it must keep)
    int section = 4;
    int kind = 8;
    int sourceRow = -1;
    bool hidden = false;
    bool device = false;
    bool mounted = false;
    bool busy = false; // mounting or unmounting
    bool setupNeeded = false;
    int usagePercent = -1; // of a mounted disk; -1 for none
    QString usageText; // "12 GiB free of 64 GiB"
    QString value; // beside the name (the Trash's count)
    QString tooltip;
    quint32 actions = 0;

    bool operator==(const PlaceEntry &o) const = default;
};

class PlacesLogic : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(bool showHidden READ showHidden WRITE setShowHidden NOTIFY showHiddenChanged)
    Q_PROPERTY(int hiddenCount READ hiddenCount NOTIFY hiddenCountChanged)
    Q_PROPERTY(int trashCount READ trashCount NOTIFY trashCountChanged)
    // The percentage at which a disk's bar turns to a warning.
    Q_PROPERTY(int nearlyFullPercent READ nearlyFullPercent CONSTANT)

public:
    // Same numbers as the core's `places::Section` and `places::Kind`.
    enum Section { Favourites = 0, Drives = 1, Network = 2, Trash = 3, Unlisted = 4 };
    Q_ENUM(Section)
    enum Kind { Folder = 0, Recent, NetworkPlace, Server, TrashPlace, Drive, Removable, Phone, Other };
    Q_ENUM(Kind)

    static PlacesLogic *instance();
    static PlacesLogic *create(QQmlEngine *, QJSEngine *);

    bool showHidden() const { return m_showHidden; }
    void setShowHidden(bool on);
    int hiddenCount() const { return m_hiddenCount; }
    int trashCount() const { return m_trashCount; }
    int nearlyFullPercent() const;
    const QList<PlaceEntry> &entries() const { return m_entries; }

    // Opens a place in the tab shown (or a new background tab). A drive that
    // is not mounted is mounted first and opens when it is ready.
    Q_INVOKABLE void open(const QString &key, bool newTab);
    // Unmounts a drive; says "Safe to remove" for one that can be taken out.
    Q_INVOKABLE void unmount(const QString &key);
    Q_INVOKABLE void rename(const QString &key, const QString &text);
    Q_INVOKABLE void setHidden(const QString &key, bool hidden);
    Q_INVOKABLE void remove(const QString &key);
    // Drops place `src` on place `dst`: it takes that position.
    Q_INVOKABLE void moveTo(const QString &src, const QString &dst);
    // Something dropped on place `target`: the Trash trashes it; folders are
    // pinned (after the target when that is a pin); files go into the target
    // folder (moved, or copied when `copy`). KIO says which is which.
    Q_INVOKABLE void handleDrop(const QList<QUrl> &urls, const QString &target, bool copy);
    // Whether `url` is a place already (hidden or not).
    Q_INVOKABLE bool isPinned(const QUrl &url) const;
    Q_INVOKABLE bool sameLocation(const QUrl &a, const QUrl &b) const;
    // The menu of a place: {open, newTab, mount, unmount, openInDisks,
    // emptyTrash, rename, hide, unhide, remove, kind, name, removable}.
    // Asked when the menu opens, so "Open in Disks" follows what is installed.
    Q_INVOKABLE QVariantMap menuFor(const QString &key) const;
    Q_INVOKABLE QString nameOf(const QString &key) const;
    Q_INVOKABLE QUrl urlOf(const QString &key) const;
    Q_INVOKABLE bool acceptsFiles(const QString &key) const;
    Q_INVOKABLE bool disksInstalled() const;
    Q_INVOKABLE void openInDisks(const QString &key);
    // Asks how big the Trash is, then `emptyTrashAsk` carries the question
    // (the text names the size) for the window to show.
    // (Emptying it is an operation of the queue: FileActions.emptyTrash.)
    Q_INVOKABLE void requestEmptyTrash();
    // Pin to Sidebar in menus: pins `url` once KIO says it is a folder.
    Q_INVOKABLE void pinFolder(const QUrl &url);

Q_SIGNALS:
    void entriesChanged();
    void showHiddenChanged();
    void hiddenCountChanged();
    void trashCountChanged();
    // A place to show in the tab shown, or in a new background tab.
    void openRequested(const QUrl &url, bool newTab);
    // A line for the window to show for a moment (a toast).
    void message(const QString &text);
    void emptyTrashAsk(int count, const QString &text);
    // Files dropped on a folder place: move them there, or copy them.
    void filesDropped(const QList<QUrl> &files, const QUrl &destination, bool copy);
    // Anything dropped on the Trash: move it to the Trash.
    void trashDropped(const QList<QUrl> &urls);

private:
    explicit PlacesLogic(QObject *parent);
    void seedStandardPlaces();
    void scheduleRebuild();
    void rebuild();
    void checkPending();
    void refreshUsage();
    void updateTrashCount();
    int rowOf(const QString &key) const;
    bool quietAfterError() const;
    QModelIndex indexOf(const PlaceEntry &e) const;
    QString keyAt(int sourceRow) const;
    void pinDirs(const QList<QUrl> &dirs, const QString &afterKey);
    QModelIndex indexForUrl(const QUrl &url) const;

    struct Usage {
        qint64 total = -1;
        qint64 free = -1;
    };
    struct Pending {
        QString key;
        bool newTab = false;
        int serial = 0;
        bool unmount = false;
        int kind = 0;
        QString name;
    };

    void addPending(const Pending &p);

    KFilePlacesModel *m_src = nullptr;
    KCoreDirLister *m_trash = nullptr;
    QList<PlaceEntry> m_entries;
    QStringList m_lastMounted;
    QHash<QString, Usage> m_usage; // by mount path
    QTimer m_rebuildTimer;
    QTimer m_usageTimer;
    QTimer m_trashTimer;
    // Mounts and unmounts asked for and not finished, by place key.
    QHash<QString, Pending> m_pending;
    int m_pendingSerial = 0;
    // When the model last said why something failed (it also says it through
    // setupDone/teardownDone; one toast is enough).
    QElapsedTimer m_errorAge;
    bool m_showHidden = false;
    int m_hiddenCount = 0;
    int m_trashCount = 0;
    bool m_askingTrash = false;
    int m_usageSerial = 0;
};
