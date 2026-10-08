// The Settings window's own settings (the ones no other class keeps) and the
// index's folders. Each is one key of Files' settings file (telamon-explorerrc):
// the page a new tab opens ([General] StartPage), whether the search words are a
// pattern by default ([Search] UsePattern) and Git status badges ([View]
// GitBadges, off by default). The other settings of the window belong to the
// classes that already keep them (TabLogic, ViewMemory, ServerLogic, ColumnLogic,
// PreviewLogic, TrashLogic, MenuPrefs, ActionsLogic); the window is the one
// place the person changes them.
//
// The folders the file index holds are the index service's `indexrc`
// (~/.config/telamon-explorer/indexrc): this class reads and edits its
// `Roots=` line (the core checks every folder) and asks the service to read it
// again. Rebuild asks the service to rescan.
#pragma once

#include <QObject>
#include <QQmlEngine>
#include <QStringList>

class SettingsLogic : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    // 0 the Home page, 1 the home folder: what a new tab and the first window open.
    Q_PROPERTY(int startPage READ startPage WRITE setStartPage NOTIFY startPageChanged)
    // The search field's words, and the folder filter, are patterns (regular expressions) by default.
    Q_PROPERTY(bool usePattern READ usePattern WRITE setUsePattern NOTIFY usePatternChanged)
    // Show Git status badges on the items of a git work tree (off by default).
    Q_PROPERTY(bool gitBadges READ gitBadges WRITE setGitBadges NOTIFY gitBadgesChanged)
    // The folders the index holds, and a line about how it is.
    Q_PROPERTY(QStringList indexFolders READ indexFolders NOTIFY indexChanged)
    Q_PROPERTY(QString indexStatus READ indexStatus NOTIFY indexChanged)
    Q_PROPERTY(bool indexOff READ indexOff NOTIFY indexChanged)

public:
    explicit SettingsLogic(QObject *parent = nullptr);

    int startPage() const { return m_startPage; }
    void setStartPage(int page);
    bool usePattern() const { return m_usePattern; }
    void setUsePattern(bool on);
    bool gitBadges() const { return m_git; }
    void setGitBadges(bool on);
    QStringList indexFolders() const { return m_folders; }
    QString indexStatus() const;
    bool indexOff() const { return m_folders.isEmpty(); }

    // The index: ask the service how it is, read indexrc again (when the window opens).
    Q_INVOKABLE void refreshIndex();
    // Rescan now.
    Q_INVOKABLE void rebuildIndex();
    // Adds / removes a folder. Returns "" or why not, in plain words.
    Q_INVOKABLE QString addIndexFolder(const QString &path);
    Q_INVOKABLE QString removeIndexFolder(const QString &path);
    // The home folder is shown as "Home" and the rest by their paths.
    Q_INVOKABLE QString folderLabel(const QString &path) const;

Q_SIGNALS:
    void startPageChanged();
    void usePatternChanged();
    void gitBadgesChanged();
    void indexChanged();

private:
    QString indexrcPath() const;
    QByteArray readIndexrc() const;
    void readFolders();
    QString saveFolders(const QStringList &folders);

    int m_startPage = 0;
    bool m_usePattern = false;
    bool m_git = false;
    QStringList m_folders;
};
