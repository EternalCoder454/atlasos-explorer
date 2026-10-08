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

// ---- Search (src/search_ffi.rs) ----
struct TelamonSearchFilter {
    uint32_t kinds_mask;
    bool files_only;
    bool has_after;
    int64_t after;
    bool has_min;
    uint64_t min;
    bool has_max;
    uint64_t max;
};
using TelamonWalkCallback = void (*)(void *user, const uint8_t *batch, size_t len, uint32_t end);
void telamon_search_filter(uint32_t kind, uint32_t modified, uint32_t size, int64_t now, int64_t startOfToday, TelamonSearchFilter *out);
size_t telamon_search_kinds(uint32_t kind, uint8_t *out, size_t cap);
uint32_t telamon_search_route(uint32_t scope, bool local, bool indexed, bool indexOn);
uint32_t telamon_search_index_state(const uint8_t *state, size_t len);
bool telamon_search_index_on(uint32_t state);
size_t telamon_search_chip(uint32_t state, const uint8_t *error, size_t errorLen, uint8_t *out, size_t cap, uint32_t *level);
size_t telamon_search_text(uint32_t which, uint64_t n, uint32_t flags, uint8_t *out, size_t cap);
size_t telamon_search_limit(uint32_t which);
size_t telamon_search_path_text(const uint8_t *parent, size_t parentLen, const uint8_t *home, size_t homeLen, uint8_t *out, size_t cap);
bool telamon_search_covers(const uint8_t *folder, size_t folderLen, const uint8_t *roots, size_t rootsLen);
void *telamon_matcher_new(const uint8_t *query, size_t queryLen, const TelamonSearchFilter *filter, bool includeHidden);
uint32_t telamon_matcher_test(const void *matcher, const uint8_t *name, size_t nameLen, bool isDir, uint64_t size, int64_t mtime);
void telamon_matcher_free(void *matcher);
void *telamon_walk_start(const uint8_t *root, size_t rootLen, const uint8_t *query, size_t queryLen, const TelamonSearchFilter *filter, bool includeHidden,
                         size_t maxHits, TelamonWalkCallback callback, void *user);
void telamon_walk_stop(void *handle);
void telamon_walk_free(void *handle);

// ---- Quick Look, the preview pane and zoom (src/preview_ffi.rs) ----
uint32_t telamon_preview_classify(const uint8_t *mime, size_t len);
size_t telamon_preview_text_cap();
size_t telamon_preview_read_text(const uint8_t *path, size_t pathLen, uint8_t *out, size_t cap, uint32_t *status, bool *truncated);
size_t telamon_preview_text(uint32_t which, uint64_t n, uint8_t *out, size_t cap);
int32_t telamon_zoom_clamp(uint32_t kind, int32_t value);
int32_t telamon_zoom_step(uint32_t kind, int32_t current, int32_t steps);
int32_t telamon_zoom_default(uint32_t kind, int32_t defaultRow);
int32_t telamon_zoom_wheel(int32_t pending, int32_t delta, int32_t *rest);
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


// ---- Search ----

// A text of the search (see telamon_search_text for `which`).
inline QString rustSearchText(uint32_t which, quint64 n = 0, uint32_t flags = 0)
{
    QByteArray buf(256, 0);
    size_t len = telamon_search_text(which, n, flags, reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()));
    if (len > size_t(buf.size())) {
        buf.resize(qsizetype(len));
        len = telamon_search_text(which, n, flags, reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()));
    }
    return QString::fromUtf8(buf.constData(), qsizetype(qMin(len, size_t(buf.size()))));
}

// The folder of a result as the Path column writes it.
inline QString rustSearchPathText(const QString &parentUrl, const QString &home)
{
    const QByteArray p = parentUrl.toUtf8(), h = home.toUtf8();
    QByteArray buf(256, 0);
    auto call = [&] {
        return telamon_search_path_text(reinterpret_cast<const uint8_t *>(p.constData()), size_t(p.size()), reinterpret_cast<const uint8_t *>(h.constData()),
                                      size_t(h.size()), reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()));
    };
    size_t len = call();
    if (len > size_t(buf.size())) {
        buf.resize(qsizetype(len));
        len = call();
    }
    return QString::fromUtf8(buf.constData(), qsizetype(qMin(len, size_t(buf.size()))));
}


// ---- Quick Look and the preview pane ----

// The category (PreviewLoader::Category) of a MIME type name.
inline uint32_t rustPreviewClassify(const QString &mime)
{
    const QByteArray m = mime.toUtf8();
    return telamon_preview_classify(reinterpret_cast<const uint8_t *>(m.constData()), size_t(m.size()));
}

// A duration (which 0, `n` in ms) or dimensions (which 1, width << 32 | height) as the details write them.
inline QString rustPreviewText(uint32_t which, quint64 n)
{
    QByteArray buf(64, 0);
    const size_t len = telamon_preview_text(which, n, reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()));
    return QString::fromUtf8(buf.constData(), qsizetype(qMin(len, size_t(buf.size()))));
}
