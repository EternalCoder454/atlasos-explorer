// A folder as a list model over KCoreDirLister. Batches are appended in one
// beginInsertRows; keys, display names and the sort permutation are computed
// on a worker from a snapshot and applied with one layoutChanged
// (docs/DESIGN.md, "Threads").
#pragma once

#include <KCoreDirLister>
#include <KFileItem>

#include <QAbstractListModel>
#include <QItemSelection>
#include <QMimeDatabase>
#include <QQmlEngine>
#include <QThreadPool>
#include <QTimer>
#include <QUrl>

class FolderModel : public QAbstractListModel
{
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QUrl url READ url WRITE setUrl NOTIFY urlChanged)
    Q_PROPERTY(bool loading READ loading NOTIFY loadingChanged)
    Q_PROPERTY(QString errorText READ errorText NOTIFY errorTextChanged)
    Q_PROPERTY(QString notice READ notice NOTIFY noticeChanged)
    Q_PROPERTY(int count READ count NOTIFY countChanged)
    Q_PROPERTY(int folderCount READ folderCount NOTIFY countChanged)
    Q_PROPERTY(int fileCount READ fileCount NOTIFY countChanged)
    Q_PROPERTY(bool showHidden READ showHidden WRITE setShowHidden NOTIFY showHiddenChanged)
    Q_PROPERTY(SortColumn sortColumn READ sortColumn WRITE setSortColumn NOTIFY sortChanged)
    Q_PROPERTY(bool sortDescending READ sortDescending WRITE setSortDescending NOTIFY sortChanged)
    Q_PROPERTY(bool foldersFirst READ foldersFirst WRITE setFoldersFirst NOTIFY sortChanged)
    Q_PROPERTY(bool canWrite READ canWrite NOTIFY canWriteChanged)

public:
    // Same order as atlas_explorer_core::sort::Column.
    enum SortColumn { Name, Size, Type, Modified, Created, Accessed };
    Q_ENUM(SortColumn)

    enum Roles {
        NameRole = Qt::UserRole + 1,
        UrlRole,
        IconNameRole,
        IsDirRole,
        IsLinkRole,
        IsHiddenRole,
        SizeRole,
        ModifiedRole,
        TypeTextRole,
        ThumbnailSourceRole,
        SizeTextRole,
        ModifiedTextRole,
    };

    explicit FolderModel(QObject *parent = nullptr);
    ~FolderModel() override;

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

    QUrl url() const { return m_url; }
    void setUrl(const QUrl &url);
    bool loading() const { return m_loading; }
    QString errorText() const { return m_error; }
    QString notice() const { return m_notice; }
    int count() const { return int(m_rows.size()); }
    int folderCount() const { return m_folders; }
    int fileCount() const { return int(m_rows.size()) - m_folders; }
    bool showHidden() const { return m_showHidden; }
    void setShowHidden(bool on);
    SortColumn sortColumn() const { return m_sortColumn; }
    void setSortColumn(SortColumn c);
    bool sortDescending() const { return m_descending; }
    void setSortDescending(bool on);
    bool foldersFirst() const { return m_foldersFirst; }
    void setFoldersFirst(bool on);
    bool canWrite() const { return m_canWrite; }

    Q_INVOKABLE void refresh();
    Q_INVOKABLE QUrl urlAt(int row) const;
    Q_INVOKABLE int rowOfUrl(const QUrl &url) const;
    Q_INVOKABLE QVariantList urlsOf(const QVariantList &rows) const;
    // First row at or after startRow (wrapping) whose display name starts with prefix; -1 for none.
    Q_INVOKABLE int findPrefix(const QString &prefix, int startRow) const;
    Q_INVOKABLE bool isDirAt(int row) const;
    // The KFileItem of the shown entry with this URL (null when it isn't listed).
    KFileItem fileItemOf(const QUrl &url) const;
    Q_INVOKABLE QItemSelection rangeSelection(int from, int to) const;

Q_SIGNALS:
    void urlChanged();
    void loadingChanged();
    void errorTextChanged();
    void noticeChanged();
    void countChanged();
    void showHiddenChanged();
    void sortChanged();
    void canWriteChanged();

private:
    struct Entry {
        KFileItem item;
        QString display;
        QString type;
        QString icon;
        QByteArray key;
        quint64 size = 0;
        qint64 mtime = 0;
        qint64 ctime = 0;
        qint64 atime = 0;
        bool isDir = false;
    };
    struct SortRowIn {
        QString name;
        QByteArray key;
        QByteArray kind;
        quint64 size;
        qint64 mtime, ctime, atime;
        bool isDir;
        bool needDisplay;
    };
    struct SortResult {
        quint64 gen = 0;
        qsizetype n = 0;
        QList<quint32> perm;
        QList<QByteArray> keys;
        QList<QString> displays;
        bool ok = false;
    };

    static Entry makeEntry(const KFileItem &item);
    void fillType(Entry &e) const;
    void open(const QUrl &url, const QString &notice);
    void resetRows();
    void setLoading(bool on);
    void setError(const QString &text);
    void addItems(const KFileItemList &items);
    void removeItems(const KFileItemList &items);
    void refreshItems(const QList<QPair<KFileItem, KFileItem>> &items);
    void onCompleted();
    void onJobError(KIO::Job *job);
    void folderGone(const QString &why);
    void scheduleSort();
    void startSort();
    void applySort(SortResult r);
    void updateCounts();

    KCoreDirLister *m_lister;
    mutable QList<Entry> m_rows;
    mutable QMimeDatabase m_mime;
    QUrl m_url;
    QString m_error;
    QString m_notice;
    int m_folders = 0;
    bool m_loading = false;
    bool m_canWrite = false;
    bool m_showHidden = false;
    bool m_foldersFirst = true;
    bool m_descending = false;
    SortColumn m_sortColumn = Name;
    // Bumped by anything that moves or replaces rows (not by appends): a
    // sort result from before it is dropped.
    quint64 m_structGen = 0;
    bool m_sortDirty = false;
    bool m_sortRunning = false;
    bool m_gone = false;
    QTimer m_sortTimer;
    QThreadPool m_pool;
};
