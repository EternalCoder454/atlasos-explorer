// Tags for the windows that list them: the seven colours, which tags are in
// use (the sidebar's Tags section: what the file index reports, plus the
// names seen in the folders shown this session), and the colour of a dot.
// The rules are the core's (atlas_explorer_core::tags); the index is asked
// through SearchService. Tags themselves are kept in an extended attribute
// of each file and changed through the operation queue (FileActions).
#pragma once

#include <QHash>
#include <QObject>
#include <QPair>
#include <QQmlEngine>
#include <QStringList>
#include <QTimer>
#include <QVariantList>

class TagLogic : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    // [{name, colour, count}] for the sidebar: the colours in use first, then
    // the named tags. Empty when no tag is known.
    Q_PROPERTY(QVariantList sidebarTags READ sidebarTags NOTIFY sidebarChanged)
    // [{name, colour}]: the seven colours, for the tag menus.
    Q_PROPERTY(QVariantList colours READ colours CONSTANT)

public:
    explicit TagLogic(QObject *parent = nullptr);
    ~TagLogic() override;

    static TagLogic *instance() { return s_instance; }
    // The `#rrggbb` of a colour tag's dot, empty for a named tag. Cached.
    static QString colourFor(const QString &name);
    // The dots for a list of tags: the colour of each colour tag, once, in order.
    static QStringList dotsFor(const QStringList &tags);
    // Names seen on items listed or changed (adds them to the sidebar).
    static void noteSeen(const QStringList &names);
    // Tags were changed by Files: the next start asks the index for them.
    static void noteChanged();

    QVariantList sidebarTags() const { return m_sidebar; }
    QVariantList colours() const;

    // Asks the index which tags are in use (and starts it when the person has
    // used tags before). Quiet when the service can't be reached.
    Q_INVOKABLE void refresh();
    // The name a person typed, made into a tag: {ok, name, text}.
    Q_INVOKABLE QVariantMap checkName(const QString &typed) const;
    // A tag name written to be shown (control and bidi characters made visible).
    Q_INVOKABLE QString shown(const QString &name) const;
    Q_INVOKABLE QString colourOf(const QString &name) const { return colourFor(name); }

Q_SIGNALS:
    void sidebarChanged();

private:
    void rebuild();
    void schedule();

    static TagLogic *s_instance;
    QStringList m_seen;
    QList<QPair<QString, uint>> m_indexed;
    QVariantList m_sidebar;
    QTimer m_later;
    bool m_used = false;
};
