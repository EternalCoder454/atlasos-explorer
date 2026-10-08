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
#include <QPointer>
#include <QQmlEngine>
#include <QSet>
#include <QThreadPool>
#include <QTimer>
#include <QUrl>

class QTcpSocket;

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
    // Items in the folder that are not shown because hidden files are off
    // (0 while they are shown). Counted shortly after the listing changes.
    Q_PROPERTY(int hiddenCount READ hiddenCount NOTIFY hiddenCountChanged)
    Q_PROPERTY(bool showHidden READ showHidden WRITE setShowHidden NOTIFY showHiddenChanged)
    Q_PROPERTY(SortColumn sortColumn READ sortColumn WRITE setSortColumn NOTIFY sortChanged)
    Q_PROPERTY(bool sortDescending READ sortDescending WRITE setSortDescending NOTIFY sortChanged)
    Q_PROPERTY(bool foldersFirst READ foldersFirst WRITE setFoldersFirst NOTIFY sortChanged)
    Q_PROPERTY(bool canWrite READ canWrite NOTIFY canWriteChanged)
    // The folder is inside an archive (`zip:/...`, `tar:/...`): read-only, with
    // an Extract button in the window.
    Q_PROPERTY(bool inArchive READ inArchive NOTIFY archiveChanged)
    // True while the rows are search results (SearchController) and not the
    // folder's items. `url` stays the folder the search started in.
    Q_PROPERTY(bool searching READ searching NOTIFY searchingChanged)
    // What the rows are grouped by (Group by in the Sort menu). The rows of a
    // group are together, the groups first in the order the core gives; each
    // row's `groupKey` is its group's name. Search results are never grouped.
    Q_PROPERTY(GroupBy groupBy READ groupBy WRITE setGroupBy NOTIFY groupChanged)
    // Whether the rows are grouped now (groupBy is set and these are not results).
    Q_PROPERTY(bool grouped READ grouped NOTIFY groupChanged)
    // Bumped when the groups were worked out again (the headers' counts are read again).
    Q_PROPERTY(int groupRevision READ groupRevision NOTIFY groupRevisionChanged)
    // "home" or "network" while `url` is one of Files' own pages (the window
    // draws the page; nothing is listed), else empty. Search results are shown
    // in the page's place while a search is on.
    Q_PROPERTY(QString pageKind READ pageKind NOTIFY pageChanged)
    // The folder is on a server (smb, sftp, ftp, webdav, nfs ...).
    Q_PROPERTY(bool onServer READ onServer NOTIFY urlChanged)
    // "Not encrypted" for a folder on FTP, plain WebDAV or NFS; empty otherwise.
    Q_PROPERTY(QString securityNote READ securityNote NOTIFY urlChanged)
    // The server did not answer (or refused the connection): errorText says
    // so in plain words and Retry lists the folder again.
    Q_PROPERTY(bool unreachable READ unreachable NOTIFY unreachableChanged)
    // The listing was stopped by the user.
    Q_PROPERTY(bool stopped READ stopped NOTIFY stoppedChanged)
    // The folder is the Trash or a folder in it (not while search results are
    // shown): its rows have an Original Location and a Date Deleted.
    Q_PROPERTY(bool inTrash READ inTrash NOTIFY trashChanged)
    // The folder is the top of the Trash: what is listed there was trashed
    // itself, and can be restored.
    Q_PROPERTY(bool trashTop READ trashTop NOTIFY trashChanged)
    // The Details view shows the Dimensions, Duration or Date Taken column:
    // only then are those read (off the GUI thread, local files only).
    Q_PROPERTY(bool wantMeta READ wantMeta WRITE setWantMeta NOTIFY wantMetaChanged)
    // The folder filter (Ctrl+F): only the items whose names match the text
    // are rows; the others are held aside and come back when the text goes.
    // The words are matched the way the search matches names; with
    // `filterPattern` the text is a regular expression. An invalid pattern
    // filters nothing and says why in `filterError`. The filter is the
    // folder's own: another folder, or a search, ends it.
    Q_PROPERTY(QString filterText READ filterText WRITE setFilterText NOTIFY filterChanged)
    Q_PROPERTY(bool filterPattern READ filterPattern WRITE setFilterPattern NOTIFY filterChanged)
    Q_PROPERTY(QString filterError READ filterError NOTIFY filterChanged)
    // Items are being held back (the text is set and usable).
    Q_PROPERTY(bool filterActive READ filterActive NOTIFY filterChanged)
    // Items in the folder with the filter off: the rows plus the held ones.
    Q_PROPERTY(int filterTotal READ filterTotal NOTIFY countChanged)
    // The results carry a matching line each (a search inside files).
    Q_PROPERTY(bool hasSnippets READ hasSnippets NOTIFY snippetsChanged)
    // Git status badges on the items of a git work tree (Settings > View; off
    // by default). The badge of a row is `gitBadge`: 0 none, 1 modified, 2 new,
    // 3 ignored, 4 in conflict. Asked of `git` on a worker, only for a folder
    // on this computer, never for search results (atlas_explorer_core::gitstatus).
    Q_PROPERTY(bool gitBadges READ gitBadges WRITE setGitBadges NOTIFY gitBadgesChanged)

public:
    // Same order as atlas_explorer_core::sort::Column.
    // Relevance is the order search results arrive in (best match first) and
    // only exists while searching; the core knows the others. OriginalLocation
    // and DateDeleted are the Trash's (the core's codes 7 and 8).
    enum SortColumn { Name, Size, Type, Modified, Created, Accessed, Relevance, OriginalLocation, DateDeleted };
    Q_ENUM(SortColumn)

    // Same order as atlas_explorer_core::group::GroupBy.
    enum GroupBy { GroupNone, GroupName, GroupType, GroupModified };
    Q_ENUM(GroupBy)

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
        PathTextRole,
        IsCutRole,
        GroupRole,
        GroupCollapsedRole,
        OriginTextRole,
        DeletedTextRole,
        // The tags (names, as `tags`), their colour dots (`tagColours`, a
        // colour for each colour tag) and the names written for a column.
        TagsRole,
        TagColoursRole,
        TagsTextRole,
        DimensionsRole,
        DurationRole,
        TakenRole,
        // The matching line of a result of a search inside files.
        SnippetRole,
        // The Git status badge (0 none).
        GitBadgeRole,
    };

    // A run of rows in one group (the groups are together): the group's
    // name, its first row and how many rows it has.
    struct GroupSpan {
        QString label;
        int first = 0;
        int count = 0;
    };
    QList<GroupSpan> groupSpans() const;

    // One search result: a lossless URL, the name made safe to show, and what
    // the row's columns need.
    struct SearchHit {
        QUrl url;
        QString name;
        bool isDir = false;
        quint64 size = 0;
        qint64 mtime = 0;
        // A search inside files: the first matching line (made safe to show),
        // its number and how many more lines match; empty/0 for a name hit.
        QString snippet;
        quint32 line = 0;
        quint32 more = 0;
    };

    explicit FolderModel(QObject *parent = nullptr);
    ~FolderModel() override;

    // What is known of one item's tags without asking the disk: `known` is
    // false until they have been read; `status` is the read's (0 read, 1 the
    // file system keeps no attributes, 2 a link, 3 not readable, 4 not text).
    struct TagInfo {
        bool known = false;
        QStringList names;
        int status = 0;
    };
    TagInfo tagInfoOf(const QUrl &url) const;
    // The items' tags, attributes or permissions changed (a queue operation,
    // an undo, or another program): every folder shown reads them again.
    static void invalidateAttributes(const QList<QUrl> &urls);
    bool gitBadges() const { return m_gitOn; }
    void setGitBadges(bool on);
    bool wantMeta() const { return m_wantMeta; }
    void setWantMeta(bool on);

    QString filterText() const { return m_filterText; }
    void setFilterText(const QString &text);
    bool filterPattern() const { return m_filterPattern; }
    void setFilterPattern(bool on);
    QString filterError() const { return m_filterError; }
    bool filterActive() const { return m_filter && !m_searching; }
    int filterTotal() const { return int(m_rows.size() + m_held.size()); }
    bool hasSnippets() const { return m_hasSnippets; }
    // The results show a matching line each (the Match column); set by the
    // search when it looks inside files, and ended with the search.
    void setSnippets(bool on);
    Q_INVOKABLE void clearFilter() { setFilterText(QString()); }

    // The items waiting to be moved (Cut, not yet pasted): their rows are
    // dimmed in every folder shown. Keys are percent-encoded URLs without a
    // trailing slash.
    static void setCutKeys(const QSet<QString> &keys);

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

    QUrl url() const { return m_url; }
    void setUrl(const QUrl &url);
    bool inArchive() const;
    QString pageKind() const;
    bool inTrash() const;
    bool trashTop() const;
    // Which page `url` is, whatever else is shown now.
    QString pageOfUrl() const;
    bool onServer() const;
    QString securityNote() const;
    bool unreachable() const { return m_unreachable; }
    bool stopped() const { return m_stopped; }
    bool loading() const { return m_loading; }
    QString errorText() const { return m_error; }
    QString notice() const { return m_notice; }
    int count() const { return int(m_rows.size()); }
    int folderCount() const { return m_folders; }
    int fileCount() const { return int(m_rows.size()) - m_folders; }
    int hiddenCount() const { return m_hidden; }
    bool showHidden() const { return m_showHidden; }
    void setShowHidden(bool on);
    SortColumn sortColumn() const { return m_sortColumn; }
    void setSortColumn(SortColumn c);
    bool sortDescending() const { return m_descending; }
    void setSortDescending(bool on);
    bool foldersFirst() const { return m_foldersFirst; }
    void setFoldersFirst(bool on);
    bool canWrite() const { return m_canWrite; }
    bool searching() const { return m_searching; }
    GroupBy groupBy() const { return m_groupBy; }
    void setGroupBy(GroupBy g);
    bool grouped() const { return m_groupBy != GroupNone && !m_searching; }
    int groupRevision() const { return m_groupRevision; }

    // Search mode: the rows are results until endSearch(), which lists the
    // folder again. Navigating (setUrl) ends it too.
    void beginSearch();
    void endSearch();
    // Replaces the results (an index search) or adds to them (a live walk).
    // They keep the order given while the sort is Relevance.
    void setSearchResults(const QList<SearchHit> &hits);
    void appendSearchResults(const QList<SearchHit> &hits);
    void setSearchBusy(bool on) { setLoading(on); }
    // Drops the results whose files are gone (after the files were trashed,
    // moved or renamed); the check runs on a worker.
    Q_INVOKABLE void pruneSearchResults();

    Q_INVOKABLE void refresh();
    // Stops listing (the Stop button): what has arrived stays.
    Q_INVOKABLE void stop();
    // The thumbnails are asked for again (the Settings switch for previews on servers changed).
    Q_INVOKABLE void thumbnailsChanged();
    Q_INVOKABLE QUrl urlAt(int row) const;
    // Where the Trash's item `url` was before it was trashed, as a path
    // ("" when it isn't a row, or the Trash did not say).
    Q_INVOKABLE QString originalPathOf(const QUrl &url) const;
    // The same for many items in one pass over the rows: url -> path (only
    // items that are rows and say where they were).
    QHash<QUrl, QString> originalPathsOf(const QList<QUrl> &urls) const;
    Q_INVOKABLE int rowOfUrl(const QUrl &url) const;
    Q_INVOKABLE QVariantList urlsOf(const QVariantList &rows) const;
    // First row at or after startRow (wrapping) whose display name starts with prefix; -1 for none.
    Q_INVOKABLE int findPrefix(const QString &prefix, int startRow) const;
    Q_INVOKABLE bool isDirAt(int row) const;
    // The KFileItem of the shown entry with this URL (null when it isn't listed).
    KFileItem fileItemOf(const QUrl &url) const;
    // The rows from `from` to `to`, leaving out the ones in collapsed groups.
    Q_INVOKABLE QItemSelection rangeSelection(int from, int to) const;
    // Collapsing a group hides its rows in the views (they stay in the model;
    // the views skip them and the keyboard goes past them). Collapsed groups
    // are forgotten when another folder is shown or the grouping changes.
    Q_INVOKABLE bool isGroupCollapsed(const QString &group) const { return m_collapsed.contains(group); }
    Q_INVOKABLE void toggleGroup(const QString &group);
    // Every group folded (or brought back): the keyboard's way to the headers.
    Q_INVOKABLE void setGroupsCollapsed(bool collapsed);
    // {first, last} rows of a group (they are together); empty for none.
    Q_INVOKABLE QVariantList groupRange(const QString &group) const;
    // How many rows the group had when the rows were last grouped.
    Q_INVOKABLE int groupCount(const QString &group) const { return m_groupCounts.value(group, 0); }
    // Whether the row is in a collapsed group.
    Q_INVOKABLE bool isRowCollapsed(int row) const;
    // The name of the group a row is in (empty for none).
    Q_INVOKABLE QString groupAt(int row) const { return row >= 0 && row < m_rows.size() && grouped() ? m_rows.at(row).group : QString(); }
    // The row from `row` going by `step` (1 or -1) that is not in a collapsed
    // group (`row` itself when it is not); -1 when there is none.
    Q_INVOKABLE int visibleRowFrom(int row, int step) const;
    // What a row shows, for Quick Look and the preview pane (no I/O):
    // {name, url, localPath, isDir, isLink, typeText, iconName, sizeText,
    // modifiedText, createdText, pathText}. Empty for a row that isn't there.
    Q_INVOKABLE QVariantMap detailsAt(int row) const;
    // The URLs of `urls` that are rows now, in the order given (one pass over the rows).
    Q_INVOKABLE QVariantList filterExisting(const QVariantList &urls) const;
    // What the rows hold: {files, folders, bytes}; bytes count the files only.
    Q_INVOKABLE QVariantMap selectionStats(const QVariantList &rows) const;

Q_SIGNALS:
    // The folder is, or is no longer, shown from inside an archive.
    void archiveChanged();
    void urlChanged();
    void loadingChanged();
    void errorTextChanged();
    void noticeChanged();
    void countChanged();
    void hiddenCountChanged();
    void showHiddenChanged();
    void sortChanged();
    void canWriteChanged();
    void searchingChanged();
    void groupChanged();
    void groupRevisionChanged();
    // Refresh (F5, the Retry button) while searching: run the search again.
    void searchRefreshRequested();
    // Refresh (F5) on one of Files' own pages: the page reads its data again.
    void pageRefreshRequested();
    void unreachableChanged();
    void stoppedChanged();
    void pageChanged();
    void trashChanged();
    void wantMetaChanged();
    void filterChanged();
    void snippetsChanged();
    void gitBadgesChanged();

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
        // Search results: the place in the list the search gave, and the folder
        // the result is in, written for the Path column (made when first shown).
        quint32 rank = 0;
        QString path;
        // The name of the group the row is in; set by the sort that groups.
        QString group;
        // The Trash: where the item was (a path), the folder it was in
        // written for the Original Location column (made when first shown),
        // and when it was deleted (seconds of local wall-clock time as if UTC).
        QString originPath;
        QString origin;
        qint64 deleted = 0;
        // Tags, read when a view first asks (0 not asked, 1 asked, 2 read);
        // `tagStatus` is how the read went (see TagInfo).
        QStringList tags;
        quint8 tagState = 0;
        quint8 tagStatus = 0;
        // Dimensions, duration and date taken, read only while a column wants them.
        QString dimensions;
        QString duration;
        QString taken;
        quint8 metaState = 0;
        // A search inside files: the matching line, shown after its number.
        QString snippet;
    };
    struct SortRowIn {
        QString name;
        QByteArray key;
        QByteArray kind;
        quint64 size;
        qint64 mtime, ctime, atime;
        bool isDir;
        bool needDisplay;
        quint32 rank;
        QString originDir;
        qint64 deleted;
    };
    struct SortResult {
        quint64 gen = 0;
        qsizetype n = 0;
        QList<quint32> perm;
        QList<QByteArray> keys;
        QList<QString> displays;
        QList<QString> groups;
        bool ok = false;
    };

    static Entry makeEntry(const KFileItem &item);
    static Entry makeSearchEntry(const SearchHit &hit, quint32 rank);
    void leaveSearch();
    // The folder filter: whether the entry stays a row, the filter built
    // again for the text, rows moved between `m_rows` and `m_held`.
    bool keeps(const Entry &e) const;
    void rebuildFilter();
    void applyFilter();
    // Takes the rows (ascending numbers) out of the list, into `into` when given.
    void takeRows(const QList<int> &rows, QList<Entry> *into);
    void removeSearchRows(const QSet<QUrl> &gone);
    void fillType(Entry &e) const;
    void open(const QUrl &url, const QString &notice);
    void resetRows();
    void setLoading(bool on);
    void setError(const QString &text);
    void setUnreachable(bool on);
    // A connection test to the server, with a 10 s limit: a server that does
    // not answer shows "Can't reach the server" and Retry instead of waiting
    // for KIO's much longer timeouts. Once the server accepts the connection
    // the test is over and KIO's job alone goes on (it may be waiting for a
    // password; Stop ends it).
    void startProbe(const QUrl &url);
    void stopProbe();
    void onUnreachable();
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
    void recountHidden();

    // Asks for the tags (or the details) of row `e` once, in a batch.
    void wantTags(Entry &e) const;
    void wantDetails(Entry &e) const;
    void readTagBatch();
    void readMetaBatch();
    struct TagResult;
    struct MetaResult;
    void applyTags(const QList<TagResult> &results);
    void applyMeta(const QList<MetaResult> &results);

    static QSet<QString> s_cut;
    static QList<FolderModel *> s_models;
    bool isCut(const KFileItem &item) const;

    // Git status badges: asked a moment after the folder or its items change,
    // answered by a worker; an answer for a folder that has been left is dropped.
    void scheduleGit();
    void runGit();
    void applyGit(quint64 serial, uint32_t status, const QByteArray &text);
    void forgetGit();
    bool m_gitOn = false;
    QHash<QString, int> m_gitMap;
    bool m_gitAll = false;
    quint64 m_gitSerial = 0;
    QTimer m_gitTimer;

    KCoreDirLister *m_lister;
    mutable QList<Entry> m_rows;
    // Entries the filter holds back (not rows; no signals for them).
    QList<Entry> m_held;
    QString m_filterText;
    bool m_filterPattern = false;
    QString m_filterError;
    // The Rust matcher for the text (null: every entry stays).
    void *m_filter = nullptr;
    bool m_hasSnippets = false;
    mutable QMimeDatabase m_mime;
    QUrl m_url;
    QString m_error;
    QString m_notice;
    int m_folders = 0;
    int m_hidden = 0;
    QTimer m_hiddenTimer;
    bool m_loading = false;
    bool m_canWrite = false;
    bool m_showHidden = false;
    bool m_foldersFirst = true;
    bool m_descending = false;
    SortColumn m_sortColumn = Name;
    GroupBy m_groupBy = GroupNone;
    QSet<QString> m_collapsed;
    QHash<QString, int> m_groupCounts;
    int m_groupRevision = 0;
    // Bumped by anything that moves or replaces rows (not by appends): a
    // sort result from before it is dropped.
    quint64 m_structGen = 0;
    bool m_sortDirty = false;
    bool m_sortRunning = false;
    bool m_gone = false;
    bool m_unreachable = false;
    bool m_stopped = false;
    QPointer<QTcpSocket> m_probe;
    QTimer m_probeTimer;
    bool m_searching = false;
    quint32 m_nextRank = 0;
    // The sort the folder had before the search, back when it ends.
    SortColumn m_folderSortColumn = Name;
    bool m_folderSortDescending = false;
    // The last URL that finished listing: only a folder seen before can be "removed".
    QUrl m_listedUrl;
    QTimer m_sortTimer;
    QThreadPool m_pool;
    // Rows that asked for their tags or details since the last batch.
    mutable QList<QUrl> m_tagWanted;
    mutable QList<QUrl> m_metaWanted;
    mutable QTimer m_tagTimer;
    mutable QTimer m_metaTimer;
    bool m_wantMeta = false;
};
