// The Home page's data: Pinned (the sidebar's folder places), Recent files
// (the freedesktop list `recently-used.xbel`) and Frequent folders (Files' own
// count of the folders the user went to, bounded and kept on this computer
// only, in telamon-explorerrc). Nothing is recommended and nothing comes from
// the cloud. What decides (which locations are counted, the bounds, which
// folders are frequent, reading the recent list as untrusted text) is the
// Rust core's (`home`); this file reads and writes the settings and moves
// results between a worker and the GUI thread. Stat calls run on the worker.
#pragma once

#include <QByteArray>
#include <QObject>
#include <QQmlEngine>
#include <QTimer>
#include <QUrl>
#include <QVariantList>

class HomeLogic : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    // Each is a list of {name, url, path, iconName, isDir, tip}.
    Q_PROPERTY(QVariantList pinned READ pinned NOTIFY pinnedChanged)
    Q_PROPERTY(QVariantList recent READ recent NOTIFY recentChanged)
    Q_PROPERTY(QVariantList frequent READ frequent NOTIFY frequentChanged)
    // How many folders are counted (even those visited once, which the list
    // leaves out): what Clear forgets.
    Q_PROPERTY(int counted READ countedFolders NOTIFY countedChanged)
    // Whether the files and folders lists are being read.
    Q_PROPERTY(bool loading READ loading NOTIFY loadingChanged)

public:
    explicit HomeLogic(QObject *parent = nullptr);
    ~HomeLogic() override;

    QVariantList pinned() const { return m_pinned; }
    QVariantList recent() const { return m_recent; }
    QVariantList frequent() const { return m_frequent; }
    bool loading() const { return m_loading; }

    // Reads the three lists again (the page asks when it is shown).
    Q_INVOKABLE void refresh();
    // The user went to this folder: it is counted (a location that isn't a
    // place to come back to, or that holds a password, is not).
    Q_INVOKABLE void visited(const QUrl &folder);
    // "Clear": forgets every count.
    Q_INVOKABLE void clearFrequent();
    int countedFolders() const;
    // Whether a section ("pinned", "recent", "frequent") is open, and keeps
    // that for the next start.
    Q_INVOKABLE bool sectionOpen(const QString &name) const;
    Q_INVOKABLE void setSectionOpen(const QString &name, bool open);
    // The list is written now (it is also written shortly after a change, and at exit).
    Q_INVOKABLE void flush();

Q_SIGNALS:
    void pinnedChanged();
    void recentChanged();
    void frequentChanged();
    void loadingChanged();
    void countedChanged();

private:
    void rebuildPinned();
    void readWorker();
    void setLoading(bool on);

    // The counts as the core writes them, one folder per line.
    QByteArray m_counts;
    bool m_dirty = false;
    QTimer m_timer;
    QVariantList m_pinned;
    QVariantList m_recent;
    QVariantList m_frequent;
    bool m_loading = false;
    int m_serial = 0;
};
