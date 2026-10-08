// The Rust core's display names, keys and sorting (src/ffi.rs), as C++ calls.
// No logic lives here, only buffers.
#pragma once

#include <QByteArray>
#include <QList>
#include <QPair>
#include <QString>

#include <cstddef>
#include <cstdint>

extern "C" {
struct TelamonSortRow {
    const uint8_t *key;
    size_t key_len;
    const uint8_t *kind;
    size_t kind_len;
    uint64_t size;
    int64_t mtime;
    int64_t ctime;
    int64_t atime;
    bool is_dir;
};
size_t telamon_display_name(const uint8_t *name, size_t len, uint8_t *out, size_t cap);
size_t telamon_name_key(const uint8_t *name, size_t len, uint8_t *out, size_t cap);
int telamon_validate_name(const uint8_t *name, size_t len, uint8_t *out, size_t cap, size_t *textLen);
int telamon_parse_address(const uint8_t *text, size_t len, const uint8_t *current, size_t currentLen, const uint8_t *home, size_t homeLen, uint8_t *out,
                        size_t cap, size_t *textLen);
bool telamon_sort_permutation(const TelamonSortRow *rows, size_t n, uint32_t column, bool descending, bool foldersFirst, uint32_t *out);
size_t telamon_path_segments(const uint8_t *url, size_t len, const uint8_t *home, size_t homeLen, uint8_t *out, size_t cap);
int telamon_split_for_completion(const uint8_t *text, size_t len, const uint8_t *current, size_t currentLen, const uint8_t *home, size_t homeLen,
                               uint8_t *out, size_t cap, size_t *textLen);
size_t telamon_rank_names(uint32_t mode, const uint8_t *prefix, size_t prefixLen, const uint8_t *entries, size_t entriesLen, bool showHidden, uint32_t *out,
                        size_t cap);
size_t telamon_completion_text(const uint8_t *typed, size_t len, const uint8_t *name, size_t nameLen, const uint8_t *dir, size_t dirLen, uint8_t *out,
                             size_t cap);
size_t telamon_location_limit(uint32_t which);
int64_t telamon_tabs_after_close(size_t len, size_t current, size_t closed);
size_t telamon_tabs_after_move(size_t len, size_t current, size_t from, size_t to);
size_t telamon_tabs_cycle(size_t len, size_t current, int64_t step);
int64_t telamon_tabs_jump(size_t len, size_t n);
size_t telamon_tabs_insert_after_opener(size_t len, size_t opener, size_t run);
size_t telamon_tabs_reopen_index(size_t len, size_t original);
size_t telamon_tabs_limit(uint32_t which);
uint32_t telamon_places_section(int32_t group, const uint8_t *scheme, size_t len);
uint32_t telamon_places_kind(uint32_t section, const uint8_t *scheme, size_t len, uint32_t flags);
uint32_t telamon_places_actions(uint32_t kind, uint32_t flags);
int64_t telamon_places_reorder_row(size_t src, size_t dst);
bool telamon_places_pinnable(const uint8_t *scheme, size_t len);
int32_t telamon_places_usage_percent(int64_t total, int64_t free);
size_t telamon_places_text(uint32_t which, const uint8_t *a, size_t aLen, const uint8_t *b, size_t bLen, uint64_t n, uint8_t *out, size_t cap);
int32_t telamon_places_nearly_full();
size_t telamon_tabs_restore(const uint8_t *saved, size_t len, size_t current, uint8_t *out, size_t cap, size_t *currentOut);
}

using RustFn = size_t (*)(const uint8_t *, size_t, uint8_t *, size_t);

inline QByteArray rustBytes(RustFn fn, const QByteArray &name)
{
    QByteArray buf(512, 0);
    const auto *in = reinterpret_cast<const uint8_t *>(name.constData());
    size_t need = fn(in, size_t(name.size()), reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()));
    if (need > size_t(buf.size())) {
        buf.resize(qsizetype(need));
        need = fn(in, size_t(name.size()), reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()));
    }
    buf.truncate(qsizetype(need));
    return buf;
}

// A file name (UTF-8 bytes) made safe to show.
inline QString rustDisplayName(const QByteArray &name)
{
    return QString::fromUtf8(rustBytes(telamon_display_name, name));
}

inline QByteArray rustNameKey(const QByteArray &name)
{
    return rustBytes(telamon_name_key, name);
}

// The result of a core check: ok, and the text (warnings, the refusal, the URL).
struct RustCheck {
    bool ok = false;
    QString text;
};

// `fn` is telamon_validate_name-shaped: ok is false when the core refused.
inline RustCheck rustValidateName(const QString &name)
{
    const QByteArray in = name.toUtf8();
    QByteArray buf(512, 0);
    size_t n = 0;
    const auto *p = reinterpret_cast<const uint8_t *>(in.constData());
    int rc = telamon_validate_name(p, size_t(in.size()), reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()), &n);
    if (n > size_t(buf.size())) {
        buf.resize(qsizetype(n));
        rc = telamon_validate_name(p, size_t(in.size()), reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()), &n);
    }
    return {rc == 0, QString::fromUtf8(buf.constData(), qsizetype(qMin(n, size_t(buf.size()))))};
}

inline RustCheck rustParseAddress(const QString &text, const QString &current, const QString &home)
{
    const QByteArray t = text.toUtf8(), c = current.toUtf8(), h = home.toUtf8();
    QByteArray buf(1024, 0);
    size_t n = 0;
    auto call = [&] {
        return telamon_parse_address(reinterpret_cast<const uint8_t *>(t.constData()), size_t(t.size()), reinterpret_cast<const uint8_t *>(c.constData()),
                                   size_t(c.size()), reinterpret_cast<const uint8_t *>(h.constData()), size_t(h.size()),
                                   reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()), &n);
    };
    int rc = call();
    if (n > size_t(buf.size())) {
        buf.resize(qsizetype(n));
        rc = call();
    }
    return {rc == 0, QString::fromUtf8(buf.constData(), qsizetype(qMin(n, size_t(buf.size()))))};
}

// The path bar's segments of a location: one {label, url} per segment, from
// the core. `home` is the home folder as a plain path.
inline QList<QPair<QString, QString>> rustPathSegments(const QString &url, const QString &home)
{
    const QByteArray u = url.toUtf8(), h = home.toUtf8();
    QByteArray buf(1024, 0);
    auto call = [&] {
        return telamon_path_segments(reinterpret_cast<const uint8_t *>(u.constData()), size_t(u.size()), reinterpret_cast<const uint8_t *>(h.constData()),
                                   size_t(h.size()), reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()));
    };
    size_t n = call();
    if (n > size_t(buf.size())) {
        buf.resize(qsizetype(n));
        n = call();
    }
    QList<QPair<QString, QString>> out;
    const QString all = QString::fromUtf8(buf.constData(), qsizetype(qMin(n, size_t(buf.size()))));
    for (const QString &line : all.split(QLatin1Char('\n'), Qt::SkipEmptyParts)) {
        const qsizetype tab = line.indexOf(QLatin1Char('\t'));
        if (tab > 0) {
            out.append({line.left(tab), line.mid(tab + 1)});
        }
    }
    return out;
}

// What the core says about typed text for completion: the folder to list (a
// URL) and the name prefix typed, or the reason it is refused.
struct RustSplit {
    bool ok = false;
    QString dir;
    QString prefix;
    QString why;
};

inline RustSplit rustSplitForCompletion(const QString &text, const QString &current, const QString &home)
{
    const QByteArray t = text.toUtf8(), c = current.toUtf8(), h = home.toUtf8();
    QByteArray buf(1024, 0);
    size_t n = 0;
    auto call = [&] {
        return telamon_split_for_completion(reinterpret_cast<const uint8_t *>(t.constData()), size_t(t.size()), reinterpret_cast<const uint8_t *>(c.constData()),
                                          size_t(c.size()), reinterpret_cast<const uint8_t *>(h.constData()), size_t(h.size()),
                                          reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()), &n);
    };
    int rc = call();
    if (n > size_t(buf.size())) {
        buf.resize(qsizetype(n));
        rc = call();
    }
    const QString out = QString::fromUtf8(buf.constData(), qsizetype(qMin(n, size_t(buf.size()))));
    RustSplit r;
    r.ok = rc == 0;
    if (!r.ok) {
        r.why = out;
        return r;
    }
    const qsizetype nl = out.indexOf(QLatin1Char('\n'));
    r.dir = nl < 0 ? out : out.left(nl);
    r.prefix = nl < 0 ? QString() : out.mid(nl + 1);
    return r;
}

// Which of a folder's names to offer and in what order (mode 0: completion of
// `prefix`; mode 1: the subfolder menu). `records` is one flag byte (never 0:
// 1 folder, 2 hidden), the name, a 0 byte per name; the result is positions in it.
inline QList<quint32> rustRankNames(uint32_t mode, const QString &prefix, const QByteArray &records, bool showHidden)
{
    const QByteArray p = prefix.toUtf8();
    QList<quint32> out(64);
    auto call = [&] {
        return telamon_rank_names(mode, reinterpret_cast<const uint8_t *>(p.constData()), size_t(p.size()), reinterpret_cast<const uint8_t *>(records.constData()),
                                size_t(records.size()), showHidden, out.data(), size_t(out.size()));
    };
    size_t n = call();
    if (n > size_t(out.size())) {
        out.resize(qsizetype(n));
        n = call();
    }
    out.resize(qsizetype(n));
    return out;
}

inline QString rustCompletionText(const QString &typed, const QString &name, const QString &dir)
{
    const QByteArray t = typed.toUtf8(), nm = name.toUtf8(), d = dir.toUtf8();
    QByteArray buf(512, 0);
    auto call = [&] {
        return telamon_completion_text(reinterpret_cast<const uint8_t *>(t.constData()), size_t(t.size()), reinterpret_cast<const uint8_t *>(nm.constData()),
                                     size_t(nm.size()), reinterpret_cast<const uint8_t *>(d.constData()), size_t(d.size()),
                                     reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()));
    };
    size_t n = call();
    if (n > size_t(buf.size())) {
        buf.resize(qsizetype(n));
        n = call();
    }
    return QString::fromUtf8(buf.constData(), qsizetype(qMin(n, size_t(buf.size()))));
}

// ---- Sidebar places (core `places` module) ----

// A sidebar text from the core (see telamon_places_text for `which`).
inline QString rustPlacesText(uint32_t which, const QString &a = QString(), const QString &b = QString(), quint64 n = 0)
{
    const QByteArray ab = a.toUtf8(), bb = b.toUtf8();
    QByteArray buf(256, 0);
    auto call = [&] {
        return telamon_places_text(which, reinterpret_cast<const uint8_t *>(ab.constData()), size_t(ab.size()), reinterpret_cast<const uint8_t *>(bb.constData()),
                                 size_t(bb.size()), n, reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()));
    };
    size_t len = call();
    if (len > size_t(buf.size())) {
        buf.resize(qsizetype(len));
        len = call();
    }
    return QString::fromUtf8(buf.constData(), qsizetype(qMin(len, size_t(buf.size()))));
}

inline uint32_t rustPlacesSection(int group, const QString &scheme)
{
    const QByteArray s = scheme.toUtf8();
    return telamon_places_section(group, reinterpret_cast<const uint8_t *>(s.constData()), size_t(s.size()));
}

inline uint32_t rustPlacesKind(uint32_t section, const QString &scheme, uint32_t flags)
{
    const QByteArray s = scheme.toUtf8();
    return telamon_places_kind(section, reinterpret_cast<const uint8_t *>(s.constData()), size_t(s.size()), flags);
}

inline bool rustPlacesPinnable(const QString &scheme)
{
    const QByteArray s = scheme.toUtf8();
    return telamon_places_pinnable(reinterpret_cast<const uint8_t *>(s.constData()), size_t(s.size()));
}
