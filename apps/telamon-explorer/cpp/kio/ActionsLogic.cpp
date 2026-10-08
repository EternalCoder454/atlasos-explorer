#include "ActionsLogic.h"

#include "RustBridge.h"

#include <KConfigGroup>
#include <KSharedConfig>

#include <QDir>
#include <QFileInfo>
#include <QMimeDatabase>
#include <QMimeType>
#include <QProcess>
#include <QStandardPaths>

namespace
{
KConfigGroup actionsGroup()
{
    return KSharedConfig::openConfig(QStringLiteral("telamon-explorerrc"))->group(QStringLiteral("CustomActions"));
}

QByteArray pathEnv()
{
    return qgetenv("PATH");
}

QString decode(const QByteArray &f)
{
    return QString::fromUtf8(QByteArray::fromPercentEncoding(f));
}

QByteArray enc(const QString &s)
{
    return s.toUtf8().toPercentEncoding();
}

// The list as the core keeps it, whatever the settings file holds.
QByteArray cleanedText()
{
    const QByteArray raw = actionsGroup().readEntry("Items", QString()).toUtf8();
    return rustText([&](uint8_t *out, size_t cap) { return telamon_actions_clean(rustPtr(raw), size_t(raw.size()), out, cap); }, 2048);
}

struct Parsed {
    int id = 0;
    QString name, program, args, types;
    bool ask = false;
};

QList<Parsed> parse(const QByteArray &text)
{
    QList<Parsed> out;
    for (const QByteArray &line : text.split('\n')) {
        const QList<QByteArray> f = line.split('\t');
        if (f.size() < 6) {
            continue;
        }
        Parsed p;
        p.id = f[0].toInt();
        p.name = decode(f[1]);
        p.program = decode(f[2]);
        p.args = decode(f[3]);
        p.types = decode(f[4]);
        p.ask = f[5].trimmed() == "1";
        if (p.id > 0) {
            out.append(p);
        }
    }
    return out;
}

// One action as the core's record: name program args types ask.
QByteArray recordOf(const QString &name, const QString &program, const QString &args, const QString &types, bool ask)
{
    return enc(name) + '\t' + enc(program) + '\t' + enc(args) + '\t' + enc(types) + '\t' + (ask ? "1" : "0");
}

QString commandText(const QString &program, const QString &args)
{
    const QByteArray p = program.toUtf8();
    const QByteArray a = args.toUtf8();
    return QString::fromUtf8(rustText([&](uint8_t *out, size_t cap) { return telamon_actions_command_text(rustPtr(p), size_t(p.size()), rustPtr(a), size_t(a.size()), out, cap); }));
}

// The type names of one item: its MIME type and the types it inherits from.
QByteArray mimeNames(const KFileItem &it)
{
    static const QMimeDatabase db;
    QString name = it.isDir() ? QStringLiteral("inode/directory") : it.mimetype();
    if (name.isEmpty()) {
        name = db.mimeTypeForFile(it.name(), QMimeDatabase::MatchExtension).name();
    }
    QStringList names{name};
    const QMimeType t = db.mimeTypeForName(name);
    if (t.isValid()) {
        names += t.allAncestors();
    }
    return names.join(QLatin1Char('\n')).toUtf8();
}

// The items for the core: a line each, the path (empty when not on this computer) and the URL.
QByteArray itemLines(const QList<QUrl> &urls)
{
    QByteArray out;
    for (const QUrl &u : urls) {
        const QByteArray path = u.isLocalFile() ? u.toLocalFile().toUtf8() : QByteArray();
        out += path.toPercentEncoding() + '\t' + u.toString(QUrl::FullyEncoded | QUrl::RemovePassword).toUtf8().toPercentEncoding() + '\n';
    }
    return out;
}
}

ActionsLogic::ActionsLogic(QObject *parent)
    : QObject(parent)
{
    reload(cleanedText());
}

int ActionsLogic::maxActions() const
{
    return int(telamon_actions_limit(0));
}

int ActionsLogic::maxNameLength() const
{
    return int(telamon_actions_limit(1));
}

int ActionsLogic::maxArgsLength() const
{
    return int(telamon_actions_limit(3));
}

void ActionsLogic::reload(const QByteArray &text)
{
    m_text = text;
    m_items.clear();
    for (const Parsed &p : parse(text)) {
        QVariantMap m;
        m.insert(QStringLiteral("id"), p.id);
        m.insert(QStringLiteral("name"), p.name);
        m.insert(QStringLiteral("program"), p.program);
        m.insert(QStringLiteral("args"), p.args);
        m.insert(QStringLiteral("types"), p.types);
        m.insert(QStringLiteral("ask"), p.ask);
        m.insert(QStringLiteral("command"), commandText(p.program, p.args));
        m_items.append(m);
    }
    Q_EMIT changed();
}

void ActionsLogic::keep(const QByteArray &text)
{
    KConfigGroup g = actionsGroup();
    if (text.isEmpty()) {
        g.deleteEntry("Items");
    } else {
        g.writeEntry("Items", QString::fromUtf8(text));
    }
    g.sync();
    reload(text);
}

QString ActionsLogic::problem(const QString &name, const QString &program, const QString &args, const QString &types, bool ask) const
{
    const QByteArray rec = recordOf(name, program, args, types, ask);
    const QByteArray path = pathEnv();
    return QString::fromUtf8(rustText([&](uint8_t *out, size_t cap) { return telamon_actions_problem(rustPtr(rec), size_t(rec.size()), rustPtr(path), size_t(path.size()), out, cap); }));
}

int ActionsLogic::add(const QString &name, const QString &program, const QString &args, const QString &types, bool ask)
{
    const QByteArray rec = recordOf(name, program, args, types, ask);
    const QByteArray path = pathEnv();
    uint32_t status = 1;
    const QByteArray text = rustText([&](uint8_t *out, size_t cap) {
        return telamon_actions_add(rustPtr(m_text), size_t(m_text.size()), rustPtr(rec), size_t(rec.size()), rustPtr(path), size_t(path.size()), out, cap, &status);
    });
    if (status == 0) {
        keep(text);
    }
    return int(status);
}

int ActionsLogic::update(int id, const QString &name, const QString &program, const QString &args, const QString &types, bool ask)
{
    const QByteArray rec = recordOf(name, program, args, types, ask);
    const QByteArray path = pathEnv();
    uint32_t status = 1;
    const QByteArray text = rustText([&](uint8_t *out, size_t cap) {
        return telamon_actions_update(rustPtr(m_text), size_t(m_text.size()), uint32_t(id), rustPtr(rec), size_t(rec.size()), rustPtr(path), size_t(path.size()), out, cap, &status);
    });
    if (status == 0) {
        keep(text);
    }
    return int(status);
}

void ActionsLogic::remove(int id)
{
    const QByteArray text = rustText([&](uint8_t *out, size_t cap) { return telamon_actions_remove(rustPtr(m_text), size_t(m_text.size()), uint32_t(id), out, cap); });
    keep(text);
}

QVariantMap ActionsLogic::get(int id) const
{
    for (const QVariant &v : m_items) {
        if (v.toMap().value(QStringLiteral("id")).toInt() == id) {
            return v.toMap();
        }
    }
    return {};
}

bool ActionsLogic::asksFirst(int id) const
{
    return get(id).value(QStringLiteral("ask")).toBool();
}

QString ActionsLogic::confirmText(int id, const QList<QUrl> &urls) const
{
    const QVariantMap a = get(id);
    if (a.isEmpty()) {
        return {};
    }
    const QString what = urls.size() == 1 ? tr("1 item") : tr("%1 items").arg(urls.size());
    return tr("Run “%1” on %2?\n\nIt starts this program with these arguments, as written here (each file becomes one argument, and no shell is used):\n%3")
        .arg(a.value(QStringLiteral("name")).toString(), what, a.value(QStringLiteral("command")).toString());
}

QString ActionsLogic::run(int id, const QList<QUrl> &urls)
{
    // Read again, so what is run is what the settings file says now (a copy
    // another process changed is read from the disk, not from the cache).
    KSharedConfig::openConfig(QStringLiteral("telamon-explorerrc"))->reparseConfiguration();
    const QList<Parsed> list = parse(cleanedText());
    const Parsed *action = nullptr;
    for (const Parsed &p : list) {
        if (p.id == id) {
            action = &p;
            break;
        }
    }
    if (!action) {
        return tr("That action is not there any more.");
    }
    // The program, looked for now: it may have been removed since it was added.
    const QByteArray prog = action->program.toUtf8();
    const QByteArray path = pathEnv();
    uint32_t status = 1;
    const QString resolved = QString::fromUtf8(rustText([&](uint8_t *out, size_t cap) {
        return telamon_actions_resolve(rustPtr(prog), size_t(prog.size()), rustPtr(path), size_t(path.size()), out, cap, &status);
    }));
    if (status != 0) {
        return resolved;
    }
    const QByteArray args = action->args.toUtf8();
    const QByteArray items = itemLines(urls);
    const QByteArray text = rustText([&](uint8_t *out, size_t cap) { return telamon_actions_expand(rustPtr(args), size_t(args.size()), rustPtr(items), size_t(items.size()), out, cap); }, 4096);
    const QList<QByteArray> lines = text.split('\n');
    if (lines.isEmpty() || lines.first() != "0") {
        return lines.size() > 1 ? QString::fromUtf8(lines.at(1)) : tr("This action can't run on these items.");
    }
    QString workDir = QDir::homePath();
    for (const QUrl &u : urls) {
        if (u.isLocalFile()) {
            workDir = QFileInfo(u.toLocalFile()).absolutePath();
            break;
        }
    }
    int started = 0;
    for (qsizetype i = 1; i < lines.size(); ++i) {
        // The last piece is empty (the text ends with a line break).
        if (lines.at(i).isEmpty()) {
            continue;
        }
        // `r`, then a tab before each argument (no argument is `r` alone, an empty one `r` and a tab).
        QStringList argv;
        const QList<QByteArray> fields = lines.at(i).split('\t');
        for (qsizetype f = 1; f < fields.size(); ++f) {
            argv << QString::fromUtf8(QByteArray::fromPercentEncoding(fields.at(f)));
        }
        QProcess p;
        p.setProgram(resolved);
        p.setArguments(argv);
        p.setWorkingDirectory(workDir);
        p.setStandardInputFile(QProcess::nullDevice());
        p.setStandardOutputFile(QProcess::nullDevice());
        p.setStandardErrorFile(QProcess::nullDevice());
        if (!p.startDetached()) {
            return tr("“%1” could not be started: %2").arg(action->name, p.errorString());
        }
        ++started;
    }
    Q_UNUSED(started)
    return {};
}

QVariantList ActionsLogic::menuEntries(const KFileItemList &items)
{
    QVariantList out;
    if (items.isEmpty()) {
        return out;
    }
    QList<QByteArray> mimes;
    for (const KFileItem &it : items) {
        mimes << mimeNames(it);
    }
    for (const Parsed &p : parse(cleanedText())) {
        const QByteArray types = p.types.toUtf8();
        bool all = true;
        for (const QByteArray &m : std::as_const(mimes)) {
            if (!telamon_actions_type_matches(rustPtr(types), size_t(types.size()), rustPtr(m), size_t(m.size()))) {
                all = false;
                break;
            }
        }
        if (all) {
            out.append(QVariantMap{{QStringLiteral("text"), p.name}, {QStringLiteral("customId"), p.id}});
        }
    }
    return out;
}
