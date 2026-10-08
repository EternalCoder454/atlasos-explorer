// The Rust core's display names, keys and sorting (src/ffi.rs), as C++ calls.
// No logic lives here, only buffers.
#pragma once

#include <QByteArray>
#include <QList>
#include <QPair>
#include <QString>
#include <QStringList>

#include <cstddef>
#include <cstdint>

extern "C" {
struct TelamonSortRow {
    const uint8_t *key;
    size_t key_len;
    const uint8_t *kind;
    size_t kind_len;
    const uint8_t *group;
    size_t group_len;
    uint64_t size;
    int64_t mtime;
    int64_t ctime;
    int64_t atime;
    // The Trash's columns: the sort key of the folder an item came from, and when it was deleted.
    const uint8_t *origin;
    size_t origin_len;
    int64_t deleted;
    bool is_dir;
};
size_t telamon_display_name(const uint8_t *name, size_t len, uint8_t *out, size_t cap);
size_t telamon_name_key(const uint8_t *name, size_t len, uint8_t *out, size_t cap);
int telamon_validate_name(const uint8_t *name, size_t len, uint8_t *out, size_t cap, size_t *textLen);
int telamon_parse_address(const uint8_t *text, size_t len, const uint8_t *current, size_t currentLen, const uint8_t *home, size_t homeLen, uint8_t *out,
                        size_t cap, size_t *textLen);
bool telamon_sort_permutation(const TelamonSortRow *rows, size_t n, uint32_t column, bool descending, bool foldersFirst, bool groupsReversed,
                              uint32_t *out);
size_t telamon_group_of(uint32_t by, const uint8_t *key, size_t keyLen, const uint8_t *kind, size_t kindLen, bool isDir, int64_t mtime, int64_t now,
                        int64_t tz, uint32_t weekStart, uint8_t *out, size_t cap);
bool telamon_group_reversed(uint32_t by, uint32_t column, bool descending);

// ---- What each folder remembers of its view (src/views_ffi.rs) ----
struct TelamonViewPrefs {
    uint32_t mode;
    uint32_t sort;
    bool descending;
    int32_t icon;
    uint32_t group;
};
bool telamon_views_get(const uint8_t *saved, size_t savedLen, const uint8_t *key, size_t keyLen, TelamonViewPrefs *out);
size_t telamon_views_set(const uint8_t *saved, size_t savedLen, const uint8_t *key, size_t keyLen, const TelamonViewPrefs *prefs, uint8_t *out, size_t cap);
size_t telamon_views_forget(const uint8_t *saved, size_t savedLen, const uint8_t *key, size_t keyLen, uint8_t *out, size_t cap);
bool telamon_views_valid_key(const uint8_t *key, size_t keyLen);
size_t telamon_views_limit(uint32_t which);
size_t telamon_views_mode_name(uint32_t mode, uint8_t *out, size_t cap);
int32_t telamon_views_mode_code(const uint8_t *name, size_t len);
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
size_t telamon_menu_state(uint32_t kind, size_t count, size_t folders, uint32_t flags, uint8_t *out, size_t cap);
bool telamon_menu_paste_into_folder(size_t count, size_t folders);
size_t telamon_menu_text(uint32_t which, const uint8_t *a, size_t aLen, uint8_t *out, size_t cap);
int telamon_zip_end(const uint8_t *tail, size_t len, uint64_t fileLen, uint64_t *out);
bool telamon_zip64_directory(const uint8_t *record, size_t len, uint64_t endAt, uint64_t *out);
bool telamon_zip_encrypted(const uint8_t *directory, size_t len);
bool telamon_archive_is_scheme(const uint8_t *scheme, size_t len);
int telamon_archive_check(const uint8_t *records, size_t recordsLen, bool archiveInstalled, uint8_t *out, size_t cap, size_t *textLen);
size_t telamon_archive_locate(const uint8_t *url, size_t len, uint8_t *out, size_t cap);
size_t telamon_archive_parent(const uint8_t *url, size_t len, uint8_t *out, size_t cap);
size_t telamon_menu_free_name(const uint8_t *wanted, size_t wantedLen, bool (*exists)(void *, const uint8_t *, size_t), void *ctx, uint8_t *out, size_t cap);
int32_t telamon_menu_hidden(bool add, const uint8_t *content, size_t contentLen, const uint8_t *names, size_t namesLen, uint8_t *out, size_t cap,
                          size_t *textLen);

// ---- Quick actions on pictures (src/image_ffi.rs) ----
uint32_t telamon_image_kind(const uint8_t *mime, size_t len);
bool telamon_image_accepts(uint32_t action, uint32_t kind);
bool telamon_image_skips(uint32_t action, uint32_t kind);
size_t telamon_image_names(uint32_t action, const uint32_t *kinds, size_t count, const uint8_t *names, size_t namesLen, uint8_t *out, size_t cap);
uint64_t telamon_image_limit(uint32_t which);
size_t telamon_image_check_input(uint64_t size, uint64_t pixels, uint8_t *out, size_t cap);
uint32_t telamon_jpeg_orientation(const uint8_t *data, size_t len);
bool telamon_jpeg_reset_orientation(uint8_t *data, size_t len);
uint32_t telamon_jpeg_turn(uint32_t orientation, bool clockwise);
int32_t telamon_pdf_merge(const uint8_t *paths, size_t pathsLen, const uint8_t *output, size_t outputLen, const uint8_t *cancel, uint32_t *pages,
                        uint32_t *bad);

// ---- The Trash (src/trash_ffi.rs) ----
struct TelamonTrashReport {
    size_t folders;
    size_t removed;
    size_t kept;
    size_t skipped;
    size_t failed;
};
bool telamon_trash_purge(uint32_t days, int64_t nowLocal, uint32_t mode, const uint8_t *dataHome, size_t len, TelamonTrashReport *out);
size_t telamon_trash_log_line(const TelamonTrashReport *report, uint32_t days, bool applied, uint8_t *out, size_t cap);
uint32_t telamon_trash_clamp_days(int64_t days);
uint32_t telamon_trash_limit(uint32_t which);
int64_t telamon_trash_parse_date(const uint8_t *text, size_t len);
size_t telamon_trash_text(uint32_t which, const uint8_t *a, size_t aLen, uint64_t n, uint8_t *out, size_t cap);

// ---- Batch Rename (src/batch_ffi.rs) ----
struct TelamonBatchSpec {
    uint32_t mode;
    const uint8_t *first;
    size_t firstLen;
    const uint8_t *second;
    size_t secondLen;
    bool matchCase;
    bool regex;
    uint64_t start;
    uint64_t step;
    uint32_t padding;
    bool atEnd;
    uint32_t caseMode;
};
size_t telamon_name_stem_len(const uint8_t *name, size_t len, bool isDir);
size_t telamon_batch_plan(const TelamonBatchSpec *spec, const uint8_t *items, size_t itemsLen, bool (*exists)(void *, const uint8_t *, size_t), void *ctx,
                        uint8_t *out, size_t cap);

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
uint32_t telamon_search_route_live(uint32_t scope, bool local);
uint32_t telamon_search_index_state(const uint8_t *state, size_t len);
bool telamon_search_index_on(uint32_t state);
size_t telamon_search_chip(uint32_t state, const uint8_t *error, size_t errorLen, uint8_t *out, size_t cap, uint32_t *level);
size_t telamon_search_text(uint32_t which, uint64_t n, uint32_t flags, uint8_t *out, size_t cap);
size_t telamon_search_limit(uint32_t which);
size_t telamon_search_path_text(const uint8_t *parent, size_t parentLen, const uint8_t *home, size_t homeLen, uint8_t *out, size_t cap);
bool telamon_search_covers(const uint8_t *folder, size_t folderLen, const uint8_t *roots, size_t rootsLen);
void *telamon_matcher_new(const uint8_t *query, size_t queryLen, const TelamonSearchFilter *filter, bool includeHidden, bool usePattern);
uint32_t telamon_matcher_test(const void *matcher, const uint8_t *name, size_t nameLen, bool isDir, uint64_t size, int64_t mtime);
void telamon_matcher_free(void *matcher);
void *telamon_walk_start(const uint8_t *root, size_t rootLen, const uint8_t *query, size_t queryLen, const TelamonSearchFilter *filter, bool includeHidden,
                         bool usePattern, const uint8_t *tag, size_t tagLen, size_t maxHits, TelamonWalkCallback callback, void *user);
void telamon_walk_stop(void *handle);
void telamon_walk_free(void *handle);

// Patterns, the folder filter and search inside files (src/search_ffi.rs).
size_t telamon_pattern_check(const uint8_t *text, size_t len, bool usePattern, bool contents, uint8_t *out, size_t cap);
void *telamon_namefilter_new(const uint8_t *text, size_t len, bool usePattern, uint8_t *err, size_t errCap, size_t *errLen);
bool telamon_namefilter_is_all(const void *filter);
bool telamon_namefilter_test(const void *filter, const uint8_t *name, size_t len);
void telamon_namefilter_free(void *filter);
struct TelamonContentStats {
    uint32_t searched;
    uint32_t binary;
    uint32_t tooLarge;
    uint32_t pdfNoTool;
    uint32_t pdfTimeout;
    uint32_t unreadable;
};
using TelamonContentCallback = void (*)(void *user, const uint8_t *batch, size_t len, uint32_t end, const TelamonContentStats *stats);
void *telamon_content_start(const uint8_t *root, size_t rootLen, const uint8_t *query, size_t queryLen, bool usePattern, const TelamonSearchFilter *filter,
                            bool includeHidden, const uint8_t *tag, size_t tagLen, size_t maxHits, TelamonContentCallback callback, void *user);
size_t telamon_content_text(uint64_t found, uint32_t end, const TelamonContentStats *stats, uint8_t *out, size_t cap);
size_t telamon_content_note(uint8_t *out, size_t cap);
bool telamon_content_pdf_available();

// Saved searches (src/saved_ffi.rs).
size_t telamon_saved_limit(uint32_t which);
size_t telamon_saved_clean(const uint8_t *list, size_t listLen, uint8_t *out, size_t cap);
size_t telamon_saved_add(const uint8_t *list, size_t listLen, const uint8_t *record, size_t recordLen, uint8_t *out, size_t cap, uint32_t *status);
size_t telamon_saved_rename(const uint8_t *list, size_t listLen, uint32_t id, const uint8_t *name, size_t nameLen, uint8_t *out, size_t cap, uint32_t *status);
size_t telamon_saved_remove(const uint8_t *list, size_t listLen, uint32_t id, uint8_t *out, size_t cap);
size_t telamon_saved_default_name(const uint8_t *record, size_t len, uint8_t *out, size_t cap);
size_t telamon_saved_describe(const uint8_t *list, size_t listLen, uint32_t id, const uint8_t *folderLabel, size_t labelLen, uint8_t *out, size_t cap);

// ---- Quick Look, the preview pane and zoom (src/preview_ffi.rs) ----
uint32_t telamon_preview_classify(const uint8_t *mime, size_t len);
size_t telamon_preview_text_cap();
size_t telamon_preview_read_text(const uint8_t *path, size_t pathLen, uint8_t *out, size_t cap, uint32_t *status, bool *truncated);
size_t telamon_preview_text(uint32_t which, uint64_t n, uint8_t *out, size_t cap);
int32_t telamon_zoom_clamp(uint32_t kind, int32_t value);
int32_t telamon_zoom_step(uint32_t kind, int32_t current, int32_t steps);
int32_t telamon_zoom_default(uint32_t kind, int32_t defaultRow);
int32_t telamon_zoom_wheel(int32_t pending, int32_t delta, int32_t *rest);

// ---- The Home page and Connect to Server (src/home_ffi.rs) ----
bool telamon_home_is_home(const uint8_t *url, size_t len);
size_t telamon_home_key(const uint8_t *input, size_t len, uint8_t *out, size_t cap);
size_t telamon_home_visit(const uint8_t *saved, size_t savedLen, const uint8_t *key, size_t keyLen, int64_t now, uint8_t *out, size_t cap);
size_t telamon_home_forget(const uint8_t *saved, size_t savedLen, const uint8_t *key, size_t keyLen, uint8_t *out, size_t cap);
size_t telamon_home_top(const uint8_t *saved, size_t savedLen, size_t n, uint8_t *out, size_t cap);
size_t telamon_home_limit(uint32_t which);
size_t telamon_home_recent_files(const uint8_t *path, size_t pathLen, size_t n, uint8_t *out, size_t cap);
size_t telamon_servers_protocol_count();
size_t telamon_servers_protocol(uint32_t code, uint32_t which, uint8_t *out, size_t cap);
uint32_t telamon_servers_protocol_port(uint32_t code);
bool telamon_servers_protocol_encrypted(uint32_t code);
int32_t telamon_servers_protocol_code(const uint8_t *scheme, size_t len);
size_t telamon_servers_build(const uint8_t *input, size_t len, uint8_t *out, size_t cap);
size_t telamon_servers_note(const uint8_t *url, size_t len, uint8_t *out, size_t cap);
size_t telamon_servers_parse(const uint8_t *url, size_t len, uint8_t *out, size_t cap);
size_t telamon_servers_clean(const uint8_t *url, size_t len, uint8_t *out, size_t cap);
size_t telamon_servers_label(const uint8_t *url, size_t len, uint8_t *out, size_t cap);
size_t telamon_servers_recent_clean(const uint8_t *saved, size_t savedLen, uint8_t *out, size_t cap);
size_t telamon_servers_recent_push(const uint8_t *saved, size_t savedLen, const uint8_t *url, size_t urlLen, uint8_t *out, size_t cap);
size_t telamon_servers_recent_remove(const uint8_t *saved, size_t savedLen, const uint8_t *url, size_t urlLen, uint8_t *out, size_t cap);
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

using RustFn2 = size_t (*)(const uint8_t *, size_t, const uint8_t *, size_t, uint8_t *, size_t);

// A core call with two text inputs.
inline QByteArray rustBytes2(RustFn2 fn, const QByteArray &a, const QByteArray &b)
{
    QByteArray buf(512, 0);
    const auto *pa = reinterpret_cast<const uint8_t *>(a.constData());
    const auto *pb = reinterpret_cast<const uint8_t *>(b.constData());
    size_t need = fn(pa, size_t(a.size()), pb, size_t(b.size()), reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()));
    if (need > size_t(buf.size())) {
        buf.resize(qsizetype(need));
        need = fn(pa, size_t(a.size()), pb, size_t(b.size()), reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()));
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


// ---- Archives opened as folders (core `archive` module) ----

inline bool rustArchiveScheme(const QString &scheme)
{
    const QByteArray b = scheme.toUtf8();
    return telamon_archive_is_scheme(reinterpret_cast<const uint8_t *>(b.constData()), size_t(b.size()));
}

// Whether what an archive lists can be taken out below a folder: ok, or the
// reason in plain words. `records` as the core's `archive::parse_records`.
struct RustArchiveCheck {
    bool ok;
    QString text;
};
inline RustArchiveCheck rustArchiveCheck(const QByteArray &records, bool archiveInstalled)
{
    QByteArray buf(512, 0);
    size_t len = 0;
    auto call = [&] {
        return telamon_archive_check(reinterpret_cast<const uint8_t *>(records.constData()), size_t(records.size()), archiveInstalled,
                                     reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()), &len);
    };
    int rc = call();
    if (len > size_t(buf.size())) {
        buf.resize(qsizetype(len));
        rc = call();
    }
    return {rc == 0, QString::fromUtf8(buf.constData(), qsizetype(qMin(len, size_t(buf.size()))))};
}

// Where a location in an archive is: the archive file, the archive's top and
// the path inside; all empty when it isn't one.
struct RustArchiveLocation {
    QUrl file;
    QUrl root;
    QString inner;
    // The archive file's name, and the folder name to extract it to.
    QString name;
    QString folderName;
    bool valid = false;
};
inline RustArchiveLocation rustArchiveLocate(const QUrl &url)
{
    const QByteArray u = url.toString(QUrl::FullyEncoded).toUtf8();
    QByteArray buf(512, 0);
    auto call = [&] { return telamon_archive_locate(reinterpret_cast<const uint8_t *>(u.constData()), size_t(u.size()), reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size())); };
    size_t len = call();
    if (len > size_t(buf.size())) {
        buf.resize(qsizetype(len));
        len = call();
    }
    RustArchiveLocation out;
    if (len == 0) {
        return out;
    }
    const QStringList lines = QString::fromUtf8(buf.constData(), qsizetype(qMin(len, size_t(buf.size())))).split(QLatin1Char('\n'));
    if (lines.size() == 5) {
        out.file = QUrl::fromEncoded(lines[0].toUtf8());
        out.root = QUrl::fromEncoded(lines[1].toUtf8());
        out.inner = lines[2];
        out.name = lines[3];
        out.folderName = lines[4];
        out.valid = true;
    }
    return out;
}

// Where Up goes from a location in an archive (invalid: it isn't one).
inline QUrl rustArchiveParent(const QUrl &url)
{
    const QByteArray u = url.toString(QUrl::FullyEncoded).toUtf8();
    QByteArray buf(512, 0);
    auto call = [&] { return telamon_archive_parent(reinterpret_cast<const uint8_t *>(u.constData()), size_t(u.size()), reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size())); };
    size_t len = call();
    if (len > size_t(buf.size())) {
        buf.resize(qsizetype(len));
        len = call();
    }
    return len == 0 ? QUrl() : QUrl::fromEncoded(QByteArray(buf.constData(), qsizetype(qMin(len, size_t(buf.size())))));
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


// ---- Context menus (core `menu` module) ----

// The facts a menu is decided from (the core's menu::F_*).
namespace MenuFlag
{
constexpr uint32_t Writable = 1;
constexpr uint32_t Searching = 1u << 1;
constexpr uint32_t Recent = 1u << 2;
constexpr uint32_t InTrash = 1u << 3;
constexpr uint32_t Local = 1u << 4;
constexpr uint32_t CanPaste = 1u << 5;
constexpr uint32_t Archive = 1u << 6;
constexpr uint32_t Pinnable = 1u << 7;
constexpr uint32_t FolderWritable = 1u << 8;
constexpr uint32_t OpenWith = 1u << 9;
constexpr uint32_t Hideable = 1u << 10;
constexpr uint32_t Unhideable = 1u << 11;
constexpr uint32_t Terminal = 1u << 12;
constexpr uint32_t CanUndo = 1u << 13;
constexpr uint32_t CanRedo = 1u << 14;
constexpr uint32_t ArchiveItems = 1u << 15;
constexpr uint32_t TrashTop = 1u << 16;
}

// The entries a menu has and whether each is enabled: key -> enabled. `items`: the menu of items; else the background's.
inline QList<QPair<QString, bool>> rustMenuState(bool items, size_t count, size_t folders, uint32_t flags)
{
    QByteArray buf(512, 0);
    auto call = [&] { return telamon_menu_state(items ? 0 : 1, count, folders, flags, reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size())); };
    size_t len = call();
    if (len > size_t(buf.size())) {
        buf.resize(qsizetype(len));
        len = call();
    }
    QList<QPair<QString, bool>> out;
    const QString text = QString::fromUtf8(buf.constData(), qsizetype(qMin(len, size_t(buf.size()))));
    for (const QString &line : text.split(QLatin1Char('\n'), Qt::SkipEmptyParts)) {
        out.append({line.chopped(1), line.endsWith(QLatin1Char('+'))});
    }
    return out;
}

// ---- The Trash ----

// A text (see telamon_trash_text for `which`).
inline QString rustTrashText(uint32_t which, const QString &a = QString(), quint64 n = 0)
{
    const QByteArray ab = a.toUtf8();
    QByteArray buf(512, 0);
    auto call = [&] {
        return telamon_trash_text(which, reinterpret_cast<const uint8_t *>(ab.constData()), size_t(ab.size()), n, reinterpret_cast<uint8_t *>(buf.data()),
                                  size_t(buf.size()));
    };
    size_t len = call();
    if (len > size_t(buf.size())) {
        buf.resize(qsizetype(len));
        len = call();
    }
    return QString::fromUtf8(buf.constData(), qsizetype(qMin(len, size_t(buf.size()))));
}

// A text of "New" (see telamon_menu_text for `which`).
inline QString rustMenuText(uint32_t which, const QString &a = QString())
{
    const QByteArray ab = a.toUtf8();
    QByteArray buf(256, 0);
    auto call = [&] { return telamon_menu_text(which, reinterpret_cast<const uint8_t *>(ab.constData()), size_t(ab.size()), reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size())); };
    size_t len = call();
    if (len > size_t(buf.size())) {
        buf.resize(qsizetype(len));
        len = call();
    }
    return QString::fromUtf8(buf.constData(), qsizetype(qMin(len, size_t(buf.size()))));
}

// The `.hidden` text with `names` added or removed. rc 0: `text` is the new
// content; 1: nothing to change; 2: refused, `text` says why.
struct RustHidden {
    int rc;
    QByteArray text;
};
inline RustHidden rustMenuHidden(bool add, const QByteArray &content, const QStringList &names)
{
    QByteArray n;
    for (const QString &s : names) {
        n += s.toUtf8();
        n.append('\0');
    }
    QByteArray buf(qMax<qsizetype>(512, content.size() + n.size() + 16), 0);
    size_t len = 0;
    auto call = [&] {
        return telamon_menu_hidden(add, reinterpret_cast<const uint8_t *>(content.constData()), size_t(content.size()), reinterpret_cast<const uint8_t *>(n.constData()),
                                 size_t(n.size()), reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()), &len);
    };
    int rc = call();
    if (len > size_t(buf.size())) {
        buf.resize(qsizetype(len));
        rc = call();
    }
    return {rc, buf.left(qsizetype(qMin(len, size_t(buf.size()))))};
}

// The first name that `exists` (called with a name) says is free: `wanted`, "wanted (2)" and so on.
template<typename F>
inline QString rustFreeName(const QString &wanted, F exists)
{
    const QByteArray w = wanted.toUtf8();
    struct Ctx {
        F *fn;
    } ctx{&exists};
    auto trampoline = [](void *c, const uint8_t *name, size_t len) -> bool {
        return (*static_cast<Ctx *>(c)->fn)(QString::fromUtf8(reinterpret_cast<const char *>(name), qsizetype(len)));
    };
    QByteArray buf(256, 0);
    auto call = [&] { return telamon_menu_free_name(reinterpret_cast<const uint8_t *>(w.constData()), size_t(w.size()), trampoline, &ctx, reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size())); };
    size_t len = call();
    if (len > size_t(buf.size())) {
        buf.resize(qsizetype(len));
        len = call();
    }
    return QString::fromUtf8(buf.constData(), qsizetype(qMin(len, size_t(buf.size()))));
}

// ---- Batch Rename ----

// One item of a batch: the code of what is the matter with its new name
// (0 unchanged, 1 fine, 2 warning, 3 not a name, 4 twin, 5 taken by a selected
// item, 6 in the folder already), the new name and the reason in words.
struct RustBatchRow {
    int code = 0;
    QString name;
    QString text;
};
struct RustBatch {
    // The items were understood (else the rest is empty).
    bool valid = false;
    bool canApply = false;
    quint32 changed = 0;
    quint32 blocked = 0;
    // Why no names could be made (a bad pattern); empty when there is none.
    QString problem;
    QList<RustBatchRow> rows;
};

// Plans a batch rename of `names` (a folder's flag with each, in order);
// `exists(name)` says whether a name is already in the folder.
template<typename F>
inline RustBatch rustBatchPlan(const TelamonBatchSpec &spec, const QList<QPair<QString, bool>> &names, F exists)
{
    QByteArray items;
    for (const auto &n : names) {
        const QByteArray b = n.first.toUtf8();
        items.append(char(n.second ? 1 : 0));
        const quint32 len = quint32(b.size());
        const char le[4] = {char(len & 0xff), char((len >> 8) & 0xff), char((len >> 16) & 0xff), char((len >> 24) & 0xff)};
        items.append(le, 4);
        items.append(b);
    }
    struct Ctx {
        F *fn;
    } ctx{&exists};
    auto trampoline = [](void *c, const uint8_t *name, size_t len) -> bool {
        return (*static_cast<Ctx *>(c)->fn)(QString::fromUtf8(reinterpret_cast<const char *>(name), qsizetype(len)));
    };
    QByteArray buf(4096, 0);
    auto call = [&] {
        return telamon_batch_plan(&spec, reinterpret_cast<const uint8_t *>(items.constData()), size_t(items.size()), trampoline, &ctx,
                                  reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()));
    };
    size_t len = call();
    if (len > size_t(buf.size())) {
        buf.resize(qsizetype(len));
        len = call();
    }
    RustBatch out;
    if (len == 0 || len > size_t(buf.size())) {
        return out;
    }
    const auto *p = reinterpret_cast<const uint8_t *>(buf.constData());
    size_t at = 0;
    auto u32 = [&](quint32 &v) {
        if (at + 4 > len) {
            return false;
        }
        v = quint32(p[at]) | quint32(p[at + 1]) << 8 | quint32(p[at + 2]) << 16 | quint32(p[at + 3]) << 24;
        at += 4;
        return true;
    };
    auto text = [&](QString &s) {
        quint32 n = 0;
        if (!u32(n) || at + n > len) {
            return false;
        }
        s = QString::fromUtf8(buf.constData() + at, qsizetype(n));
        at += n;
        return true;
    };
    if (len < 1) {
        return out;
    }
    out.canApply = p[at++] != 0;
    if (!u32(out.changed) || !u32(out.blocked) || !text(out.problem)) {
        return out;
    }
    for (qsizetype i = 0; i < names.size(); ++i) {
        RustBatchRow row;
        if (at >= len) {
            return out;
        }
        row.code = p[at++];
        if (!text(row.name) || !text(row.text)) {
            return out;
        }
        out.rows.append(row);
    }
    out.valid = true;
    return out;
}
