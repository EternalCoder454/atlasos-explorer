// The sidebar's Saved Searches: where they are kept (telamon-explorerrc,
// [SavedSearches]) and what the window asks of them. The list is text the Rust
// core reads, checks and changes (atlas_explorer_core::saved): a hand-edited or
// damaged settings file gives the searches that are fine. A saved search is
// the words, the scope, the chips and the two switches; never results.
#pragma once

#include <QObject>
#include <QQmlEngine>
#include <QVariantList>
#include <QVariantMap>

class SavedLogic : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    // [{id, name, tip, scope, folder, query, kind, modified, size, tag, pattern, contents}]
    Q_PROPERTY(QVariantList items READ items NOTIFY changed)
    Q_PROPERTY(int count READ count NOTIFY changed)
    Q_PROPERTY(int maxNameLength READ maxNameLength CONSTANT)

public:
    explicit SavedLogic(QObject *parent = nullptr);

    QVariantList items() const { return m_items; }
    int count() const { return int(m_items.size()); }
    int maxNameLength() const;

    // A name to offer for the search (SearchController.snapshot()).
    Q_INVOKABLE QString suggestName(const QVariantMap &search) const;
    // Saves `search` under `name`: 0 saved, 1 refused (no name, nothing to
    // look for), 2 there are too many already.
    Q_INVOKABLE int save(const QString &name, const QVariantMap &search);
    // 0 renamed, 1 no usable name, 3 no such search.
    Q_INVOKABLE int rename(int id, const QString &name);
    Q_INVOKABLE void remove(int id);
    // The search as SearchController.applySaved takes it ({} for none).
    Q_INVOKABLE QVariantMap get(int id) const;
    Q_INVOKABLE QString nameOf(int id) const;

Q_SIGNALS:
    void changed();

private:
    void reload(const QByteArray &text);
    void keep(const QByteArray &text);

    QByteArray m_text;
    QVariantList m_items;
};
