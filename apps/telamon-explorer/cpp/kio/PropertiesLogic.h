// What the Properties window shows and does for the items it was opened on:
// the facts of General (name, kind, size, location, dates), the Permissions
// in plain words, Details from KFileMetaData, the tags and star rating, the
// application that opens the kind of file, and the two things that take time
// and are only done on request and can be stopped: the size of folders and
// the checksum of a file. Everything that reads the disk runs on a worker
// (KIO for items on servers); the window only draws what is here. Changes
// (a new name, tags, rating, permissions) are not made here: the window asks
// FileActions, which queues them, so they are undoable.
#pragma once

#include <QList>
#include <QMap>
#include <QObject>
#include <QPointer>
#include <QQmlEngine>
#include <QUrl>
#include <QVariantList>
#include <QVariantMap>

#include <atomic>
#include <memory>

class KJob;

class PropertiesLogic : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    // The items shown (at most 64).
    Q_PROPERTY(QList<QUrl> urls READ urls NOTIFY urlsChanged)
    // Facts are still being collected.
    Q_PROPERTY(bool busy READ busy NOTIFY busyChanged)
    // {title, iconName, single, name, nameEditable, kind, location, sizeText, sizeNote,
    //  created, modified, accessed, linkTarget, count, local, anyDir, onlyFolders, gone}
    Q_PROPERTY(QVariantMap general READ general NOTIFY generalChanged)
    // {available, editable, why, owner, group, isDir, rows: [{who, label, read, write, run, words}],
    //  runLabel, sentence, octal, symbolic, canRecurse}; read/write/run are Qt check states.
    Q_PROPERTY(QVariantMap perms READ perms NOTIFY permsChanged)
    // [{label, value}]: what KFileMetaData knows; empty when nothing is.
    Q_PROPERTY(QVariantList details READ details NOTIFY detailsChanged)
    Q_PROPERTY(bool detailsLoaded READ detailsLoaded NOTIFY detailsChanged)
    // {available, why, current: {id, name, icon}, apps: [{id, name, icon}], mime, kind}
    Q_PROPERTY(QVariantMap openWith READ openWith NOTIFY openWithChanged)
    // {available, why, tags: [{name, text, colour, state}], hasTags}
    Q_PROPERTY(QVariantMap tagsInfo READ tagsInfo NOTIFY attributesChanged)
    // 0 to 10; -1 when it can't be kept or is not the same for every item.
    Q_PROPERTY(int rating READ rating NOTIFY attributesChanged)
    Q_PROPERTY(bool ratingAvailable READ ratingAvailable NOTIFY attributesChanged)
    // {available, why, running, progress, alg, results: [{alg, name, hex}], compare: {state, text}}
    Q_PROPERTY(QVariantMap checksum READ checksum NOTIFY checksumChanged)
    // {available, running, state, text}
    Q_PROPERTY(QVariantMap folderSize READ folderSize NOTIFY folderSizeChanged)

public:
    explicit PropertiesLogic(QObject *parent = nullptr);
    ~PropertiesLogic() override;

    QList<QUrl> urls() const { return m_urls; }
    bool busy() const { return m_busy; }
    QVariantMap general() const { return m_general; }
    QVariantMap perms() const { return m_perms; }
    QVariantList details() const { return m_details; }
    bool detailsLoaded() const { return m_detailsLoaded; }
    QVariantMap openWith() const { return m_openWith; }
    QVariantMap tagsInfo() const { return m_tagsInfo; }
    int rating() const { return m_rating; }
    bool ratingAvailable() const { return m_ratingAvailable; }
    QVariantMap checksum() const { return m_checksum; }
    QVariantMap folderSize() const { return m_folderSize; }

    // Collects the facts of these items; stops whatever ran for the last ones.
    Q_INVOKABLE void load(const QList<QUrl> &urls);
    // Reads tags, rating and permissions again (a queued change ended).
    Q_INVOKABLE void reloadAttributes();
    // Stops everything that is running (the window closed).
    Q_INVOKABLE void stopAll();

    // The checksum of the one file shown. `alg`: 0 SHA-256, 1 SHA-1, 2 MD5, 3 SHA-512.
    Q_INVOKABLE void startChecksum(int alg);
    Q_INVOKABLE void cancelChecksum();
    // What was pasted to compare with; calculates the kind it is if that
    // hasn't been done yet.
    Q_INVOKABLE void compareWith(const QString &text);
    // The size of the folders shown and everything in them.
    Q_INVOKABLE void startFolderSize();
    Q_INVOKABLE void cancelFolderSize();
    // Makes an application the one that opens this kind of file.
    Q_INVOKABLE void setDefaultApp(const QString &serviceId);
    // The name of an item as the Rename box starts: the real name.
    Q_INVOKABLE QString editableName() const;
    // The URL `name` would have next to the item shown.
    Q_INVOKABLE QUrl renamedUrl(const QString &name) const;

Q_SIGNALS:
    void urlsChanged();
    void busyChanged();
    void generalChanged();
    void permsChanged();
    void detailsChanged();
    void openWithChanged();
    void attributesChanged();
    void checksumChanged();
    void folderSizeChanged();

public:
    // What is known of one item.
    struct Item {
        QUrl url;
        QString name;
        bool ok = false;
        bool isDir = false;
        bool isLink = false;
        bool local = false;
        QString linkTarget;
        quint64 size = 0;
        qint64 mtime = 0;
        qint64 ctime = 0;
        qint64 atime = 0;
        QString mime;
        QString kind;
        QString icon;
        // This computer's files only.
        uint mode = 0;
        uint uid = 0;
        QString owner;
        QString group;
        QStringList tags;
        int tagStatus = 3;
        int rating = -1;
    };

private:
    struct Read;
    void itemDone(quint64 gen, int index, const Item &item);
    void localDone(quint64 gen, const QList<Item> &items);
    void assemble(bool full = true);
    void assembleAttributes();
    void startDetails();
    void applyChecksum();
    void setChecksumState(bool running, double progress);
    void checksumFinished(const QString &hex, int alg, int rc, const QString &error);
    void updateCompare();
    void setFolderSizeState(const QString &state, const QString &text, bool running);
    QString sizeLine(quint64 bytes) const;

    QList<QUrl> m_urls;
    QList<Item> m_items;
    int m_pending = 0;
    quint64 m_gen = 0;
    bool m_busy = false;
    QVariantMap m_general;
    QVariantMap m_perms;
    QVariantList m_details;
    bool m_detailsLoaded = false;
    QVariantMap m_openWith;
    QVariantMap m_tagsInfo;
    int m_rating = -1;
    bool m_ratingAvailable = false;
    QVariantMap m_checksum;
    QVariantMap m_folderSize;
    // The folders' totals once counted (for the size line).
    bool m_sizeKnown = false;
    quint64 m_sizeBytes = 0;
    quint64 m_sizeFiles = 0;
    quint64 m_sizeFolders = 0;
    quint64 m_sizeOnDisk = 0;

    // Checksum: the stop flag the worker reads, what was calculated, what was pasted.
    std::shared_ptr<std::atomic<bool>> m_sumCancel;
    bool m_sumRunning = false;
    int m_sumAlg = 0;
    double m_sumProgress = 0;
    QMap<int, QString> m_sums;
    QString m_expected;
    QString m_sumError;
    int m_pendingCompareAlg = -1;
    std::shared_ptr<std::atomic<bool>> m_sizeCancel;
    QPointer<KJob> m_sizeJob;
    bool m_sizeRunning = false;
};
