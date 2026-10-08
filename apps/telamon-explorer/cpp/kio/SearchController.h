// The search of one tab. SearchService is the client of the file index
// (net.eterneon.telamon.explorer.Search1 over D-Bus, asynchronous, which
// starts the service when nobody has it); SearchController is what a tab's
// search field, chips and results talk to: it decides where a search runs
// (the index, or a live walk of folders the index doesn't hold), drops
// answers that are out of date, and hands the hits to the tab's FolderModel.
// What decides is in the Rust core (atlas_explorer_core::search,
// atlas_file_index::walk); see docs/DESIGN.md, "Search".
#pragma once

#include "FolderModel.h"

#include <QElapsedTimer>
#include <QList>
#include <QObject>
#include <QPointer>
#include <QQmlEngine>
#include <QStringList>
#include <QTimer>
#include <QUrl>
#include <QVariantMap>

#include <functional>

namespace KIO
{
class Job;
}
class KJob;

// The index as the window sees it: its state and indexed folders (kept
// current by the service's StatusChanged signal), and searches.
class SearchService : public QObject
{
    Q_OBJECT

public:
    // Same numbers as telamon_search_index_state.
    enum State { Unknown = 0, Ready = 1, Updating = 2, Off = 3, Problem = 4, Unavailable = 5 };

    static SearchService *instance();

    State state() const { return m_state; }
    QString errorText() const { return m_error; }
    // The folders the index holds; the home folder before the service has said.
    QStringList roots() const;
    // Whether the service has told us its state (and it is reachable).
    bool known() const { return m_state != Unknown && m_state != Unavailable; }

    // Asks Status (which starts the service if it isn't running); `done` runs
    // on the GUI thread, also when the service can't be reached.
    void refreshStatus(QObject *context, std::function<void()> done = {});

    enum Failure { NoFailure, Unreachable, Busy };
    using SearchDone = std::function<void(Failure failure, const QList<FolderModel::SearchHit> &hits)>;
    // Search(query, limit, options); `done` is not called when `context` is gone.
    void search(const QString &query, uint limit, const QVariantMap &options, QObject *context, SearchDone done);
    // Tags(): the tags in use (name, number of items), most used first. With
    // `activate` false the service is only asked when it is running already
    // (a call would start it). `done(false, {})` when it can't answer; not
    // called when `context` is gone.
    using TagsDone = std::function<void(bool ok, const QList<QPair<QString, uint>> &tags)>;
    void tags(QObject *context, bool activate, TagsDone done);
    // NotifyChanged(uris): tells the service that these items changed in a way
    // its watches may not show (a tag). Only when it is running; no answer is waited for.
    void notifyChanged(const QList<QUrl> &urls);

Q_SIGNALS:
    void stateChanged();

private Q_SLOTS:
    void onStatusChanged(const QVariantMap &status);

private:
    explicit SearchService(QObject *parent = nullptr);
    void apply(const QVariantMap &status);
    void setUnavailable();
    void subscribe();

    State m_state = Unknown;
    QString m_error;
    QStringList m_roots;
    bool m_subscribed = false;
    // Status calls in flight are answered together.
    bool m_asking = false;
    QList<std::function<void()>> m_waiting;
};

class SearchController : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(FolderModel *folder READ folder WRITE setFolder NOTIFY folderChanged)
    Q_PROPERTY(QString text READ text WRITE setText NOTIFY textChanged)
    // 0 This Folder, 1 Everywhere
    Q_PROPERTY(int scope READ scope WRITE setScope NOTIFY scopeChanged)
    // The chips (numbers as in atlas_explorer_core::search): 0 is "any".
    Q_PROPERTY(int kind READ kind WRITE setKind NOTIFY kindChanged)
    Q_PROPERTY(int modified READ modified WRITE setModified NOTIFY modifiedChanged)
    Q_PROPERTY(int size READ size WRITE setSize NOTIFY sizeChanged)
    // Only items with this tag (a name; empty for any): the sidebar's Tags
    // section and the "tag" chip.
    Q_PROPERTY(QString tag READ tag WRITE setTag NOTIFY tagChanged)
    // Something is being searched for: words or a chip.
    Q_PROPERTY(bool active READ active NOTIFY activeChanged)
    // A live walk is running (the Stop button shows).
    Q_PROPERTY(bool walking READ walking NOTIFY walkingChanged)
    // A search is out and nothing has come back yet.
    Q_PROPERTY(bool pending READ pending NOTIFY pendingChanged)
    // The search goes through a walk and not the index.
    Q_PROPERTY(bool live READ live NOTIFY routeChanged)
    // The status chip: how it is drawn (0 none, 1 good, 2 warning, 3 error)
    // and what it says; empty for a live search.
    Q_PROPERTY(int chipLevel READ chipLevel NOTIFY chipChanged)
    Q_PROPERTY(QString chipText READ chipText NOTIFY chipChanged)
    // "12 results", "Searching, 3 found" ...
    Q_PROPERTY(QString statusText READ statusText NOTIFY statusTextChanged)
    // Why there are no results to show, in plain words (empty when there is no failure).
    Q_PROPERTY(QString failureTitle READ failureTitle NOTIFY failureChanged)
    Q_PROPERTY(QString failureText READ failureText NOTIFY failureChanged)
    // Milliseconds from the last change of the text to the rows being set.
    Q_PROPERTY(int lastMs READ lastMs NOTIFY statusTextChanged)

public:
    // Same numbers as atlas_explorer_core::search::Route.
    enum Route { IndexEverywhere, IndexFolder, LiveFolder, LiveHome, LiveRemote, NoRoute = -1 };
    Q_ENUM(Route)

    explicit SearchController(QObject *parent = nullptr);
    ~SearchController() override;

    FolderModel *folder() const { return m_folder; }
    void setFolder(FolderModel *f);
    QString text() const { return m_text; }
    void setText(const QString &t);
    int scope() const { return m_scope; }
    void setScope(int s);
    int kind() const { return m_kind; }
    void setKind(int k);
    int modified() const { return m_modified; }
    void setModified(int m);
    int size() const { return m_size; }
    void setSize(int s);
    QString tag() const { return m_tag; }
    void setTag(const QString &t);
    bool active() const;
    bool walking() const { return m_walking; }
    bool pending() const { return m_pending; }
    bool live() const { return m_route >= LiveFolder; }
    int chipLevel() const { return m_chipLevel; }
    QString chipText() const { return m_chipText; }
    QString statusText() const { return m_statusText; }
    QString failureTitle() const { return m_failureTitle; }
    QString failureText() const { return m_failureText; }
    int lastMs() const { return m_lastMs; }

    // Ends the search: no words, no chips, the folder shown again.
    Q_INVOKABLE void clear();
    // Lists every item with the tag, from everywhere: the words and chips are
    // cleared and the scope is Everywhere (the index, or a walk of the home
    // folder with a Stop button when the index is off).
    Q_INVOKABLE void showTag(const QString &name);
    // Stops a live walk; what it found stays.
    Q_INVOKABLE void stop();
    // The field was focused: ask the service for its state now, so the first
    // key doesn't wait for it (and the service starts if it isn't running).
    Q_INVOKABLE void warm();
    // Runs the search again (files changed, Refresh).
    Q_INVOKABLE void rerun();
    // "Small (under 1.0 MiB)" for the Size chip's menu (1 small, 2 medium, 3 large).
    Q_INVOKABLE QString sizeHint(int size) const;

    // The walk's thread reports here, through the event loop.
    struct WalkContext;
    void onWalkFromThread(quint64 serial, const QByteArray &batch, uint end);

Q_SIGNALS:
    void folderChanged();
    void textChanged();
    void scopeChanged();
    void kindChanged();
    void modifiedChanged();
    void sizeChanged();
    void tagChanged();
    void activeChanged();
    void walkingChanged();
    void pendingChanged();
    void routeChanged();
    void chipChanged();
    void statusTextChanged();
    void failureChanged();

private:
    QUrl searchFolder() const;
    void changed();
    void run();
    void startRoute(quint64 serial);
    void routeNow(quint64 serial, bool covered);
    void searchIndex(quint64 serial, Route route);
    void startWalk(quint64 serial, const QUrl &root);
    void startKio(quint64 serial, const QUrl &root);
    void stopLive();
    void setRoute(Route r);
    void setWalking(bool on);
    void setPending(bool on);
    void refreshChip();
    void setStatusText(const QString &text);
    void setFailure(const QString &title, const QString &text);
    void showCount(int n, bool capped);
    void showLive(bool running, bool stopped, bool capped);
    void resetState();
    void applyHits(quint64 serial, const QList<FolderModel::SearchHit> &hits, bool append);

    QPointer<FolderModel> m_folder;
    QString m_text;
    int m_scope = 0;
    int m_kind = 0;
    int m_modified = 0;
    int m_size = 0;
    QString m_tag;
    bool m_wasActive = false;
    SearchService::State m_lastState = SearchService::Unknown;
    bool m_resetting = false;
    QTimer m_kick;
    // Bumped by every new search: an answer carrying an older number is dropped.
    quint64 m_serial = 0;
    Route m_route = NoRoute;
    bool m_walking = false;
    bool m_pending = false;
    int m_busyRetries = 0;
    bool m_stopped = false;
    int m_found = 0;
    int m_chipLevel = 0;
    QString m_chipText;
    QString m_statusText;
    QString m_failureTitle;
    QString m_failureText;
    int m_lastMs = -1;
    QElapsedTimer m_typed;
    // A live search in flight: a walk on a thread, or a KIO listing.
    void *m_walk = nullptr;
    QPointer<KIO::Job> m_job;
    void *m_matcher = nullptr;
    QUrl m_kioRoot;
};
