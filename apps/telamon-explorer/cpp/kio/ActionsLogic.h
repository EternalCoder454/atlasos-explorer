// Custom actions (Settings > Context Menu and Actions): the commands the user
// adds to "More Actions". The list is text in Files' settings file
// (telamon-explorerrc, [CustomActions], key Items) that the Rust core reads,
// checks and changes (atlas_explorer_core::actions); this class adds, edits and
// removes actions, offers them to the context menu for the items under the
// pointer, and runs one. A command is run as a program with an argument list
// (QProcess, never a shell): the core splits the typed arguments into words,
// replaces the placeholders (%f %F %u %U %d) by the items, and every file name
// is one argument whatever it holds.
#pragma once

#include <KFileItem>

#include <QObject>
#include <QQmlEngine>
#include <QUrl>
#include <QVariantList>
#include <QVariantMap>

class ActionsLogic : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    // [{id, name, program, args, types, ask, command}]; `types` is the patterns
    // as typed (space-separated), `command` the program and arguments as shown.
    Q_PROPERTY(QVariantList items READ items NOTIFY changed)
    Q_PROPERTY(int count READ count NOTIFY changed)
    Q_PROPERTY(int maxActions READ maxActions CONSTANT)
    Q_PROPERTY(int maxNameLength READ maxNameLength CONSTANT)
    Q_PROPERTY(int maxArgsLength READ maxArgsLength CONSTANT)

public:
    explicit ActionsLogic(QObject *parent = nullptr);

    QVariantList items() const { return m_items; }
    int count() const { return int(m_items.size()); }
    int maxActions() const;
    int maxNameLength() const;
    int maxArgsLength() const;

    // Why this action can't be kept, in plain words ("": it can). Looks for the program.
    Q_INVOKABLE QString problem(const QString &name, const QString &program, const QString &args, const QString &types, bool ask) const;
    // 0 added, 1 refused (see `problem`), 2 there are too many already.
    Q_INVOKABLE int add(const QString &name, const QString &program, const QString &args, const QString &types, bool ask);
    // 0 changed, 1 refused, 3 no such action.
    Q_INVOKABLE int update(int id, const QString &name, const QString &program, const QString &args, const QString &types, bool ask);
    Q_INVOKABLE void remove(int id);
    Q_INVOKABLE QVariantMap get(int id) const;
    Q_INVOKABLE bool asksFirst(int id) const;
    // The question of "ask first": what runs, on how many items.
    Q_INVOKABLE QString confirmText(int id, const QList<QUrl> &urls) const;
    // Runs action `id` on `urls`. Returns "" when it started, else why not, in plain words.
    Q_INVOKABLE QString run(int id, const QList<QUrl> &urls);

    // The actions that apply to all of `items`, for the menu: [{text, customId}].
    // Read from the settings file when asked, so a change shows in the next menu.
    static QVariantList menuEntries(const KFileItemList &items);

Q_SIGNALS:
    void changed();

private:
    void reload(const QByteArray &text);
    void keep(const QByteArray &text);

    QByteArray m_text;
    QVariantList m_items;
};
