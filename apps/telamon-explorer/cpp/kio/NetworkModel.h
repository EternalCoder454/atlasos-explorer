// The Network page: the computers on the local network that offer something
// Files can open, found by Avahi (mDNS/DNS-SD, over the system bus) and by the
// SMB worker's own browsing (`smb:/`: Windows computers by DNS-SD, WS-Discovery
// or NetBIOS). KIO has no `network:/` worker, so Files lists them itself.
//
// Everything is asynchronous on the GUI thread (D-Bus calls and a KIO job):
// nothing blocks, a scan ends by itself after ten seconds or at once when the
// user presses Stop, and the Avahi side keeps listening while the page is open
// so computers that appear later are added. Nothing is stored. A name found
// on the network is untrusted text: it is shown through the core's display
// names, and the address Files opens is built by the core (`servers`).
#pragma once

#include <QAbstractListModel>
#include <QHash>
#include <QPointer>
#include <QQmlEngine>
#include <QSet>
#include <QTimer>
#include <QUrl>

namespace KIO
{
class Job;
class ListJob;
}
class QDBusMessage;
class QDBusPendingCallWatcher;

class NetworkModel : public QAbstractListModel
{
    Q_OBJECT
    QML_ELEMENT
    // A scan is going on (the page shows its spinner and Stop).
    Q_PROPERTY(bool scanning READ scanning NOTIFY scanningChanged)
    // Avahi answers on the system bus (without it only the SMB worker looks).
    Q_PROPERTY(bool discoveryAvailable READ discoveryAvailable NOTIFY scanningChanged)
    Q_PROPERTY(int count READ count NOTIFY countChanged)

public:
    enum Roles { NameRole = Qt::UserRole + 1, UrlRole, KindRole, IconNameRole };

    explicit NetworkModel(QObject *parent = nullptr);
    ~NetworkModel() override;

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

    bool scanning() const { return m_scanning; }
    bool discoveryAvailable() const { return m_avahiOk; }
    int count() const { return int(m_rows.size()); }

    // Starts again: the list is cleared and a scan begins.
    Q_INVOKABLE void start();
    // Stops looking (what was found stays).
    Q_INVOKABLE void stop();

Q_SIGNALS:
    void scanningChanged();
    void countChanged();

private Q_SLOTS:
    void onItemNew(const QDBusMessage &msg);
    void onItemRemove(const QDBusMessage &msg);
    void onAllForNow(const QDBusMessage &msg);
    void onFailure(const QDBusMessage &msg);

private:
    struct Row {
        QString key; // service name + type: what Avahi removes it by
        QString name;
        QUrl url;
        QString kind;
        QString icon;
    };

    bool startAvahi();
    void startSmb();
    void freeBrowsers();
    void addRow(const Row &row);
    void setScanning(bool on);
    void maybeDone();

    QList<Row> m_rows;
    QSet<QString> m_urls;
    bool m_scanning = false;
    bool m_avahiOk = false;
    bool m_avahiConnected = false;
    // Avahi's signals are taken (from the start of a scan to Stop).
    bool m_listening = false;
    // The browsers Avahi made for us (object paths) and which of them said "all for now".
    QSet<QString> m_browsers;
    QSet<QString> m_early;
    int m_browsersAsked = 0;
    int m_browsersDone = 0;
    QPointer<KIO::ListJob> m_smb;
    QTimer m_deadline;
    int m_generation = 0;
};
