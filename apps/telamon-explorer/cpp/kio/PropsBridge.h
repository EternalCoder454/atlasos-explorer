// Tags, ratings, permissions, checksums and folder sizes of the Rust core
// (src/props_ffi.rs), as C++ calls. No logic lives here, only buffers.
#pragma once

#include <QByteArray>
#include <QFile>
#include <QList>
#include <QString>
#include <QStringList>
#include <QUrl>

#include <cstddef>
#include <cstdint>

extern "C" {
struct TelamonTotals {
    uint64_t files;
    uint64_t folders;
    uint64_t bytes;
    uint64_t on_disk;
    uint64_t unreadable;
    uint64_t skipped;
};
struct TelamonOutcome;

size_t telamon_tags_read(const uint8_t *path, size_t len, uint8_t *out, size_t cap, uint32_t *status);
int32_t telamon_rating_read(const uint8_t *path, size_t len);
size_t telamon_tags_colour(const uint8_t *name, size_t len, uint8_t *out, size_t cap);
size_t telamon_tags_colours(uint8_t *out, size_t cap);
size_t telamon_tags_new_name(const uint8_t *name, size_t len, uint8_t *out, size_t cap, uint32_t *problem);
size_t telamon_tags_in_use(const uint8_t *seen, size_t seenLen, const uint8_t *indexed, size_t indexedLen, uint8_t *out, size_t cap);
uint32_t telamon_tags_have(const uint8_t *items, size_t itemsLen, const uint8_t *name, size_t nameLen);
size_t telamon_tags_status_text(uint32_t status, uint8_t *out, size_t cap);

TelamonOutcome *telamon_attrs_edit(uint32_t kind, const uint8_t *paths, size_t pathsLen, const uint8_t *add, size_t addLen, const uint8_t *remove,
                                   size_t removeLen, bool clear, uint32_t value, uint32_t setBits, uint32_t clearBits, const uint8_t *cancel);
TelamonOutcome *telamon_attrs_revert(const uint8_t *changes, size_t len, bool rev, const uint8_t *cancel);
uint32_t telamon_attrs_summary(const TelamonOutcome *h, uint64_t *items, uint64_t *failed);
size_t telamon_attrs_problem(const TelamonOutcome *h, uint8_t *out, size_t cap);
size_t telamon_attrs_changes(const TelamonOutcome *h, uint8_t *out, size_t cap);
void telamon_attrs_free(TelamonOutcome *h);
size_t telamon_attr_read(const uint8_t *path, size_t len, uint32_t key, uint8_t *out, size_t cap, bool *ok);

uint32_t telamon_perm_access(uint32_t mode, uint32_t who);
uint32_t telamon_perm_with(uint32_t mode, uint32_t who, uint32_t bits);
void telamon_perm_difference(uint32_t before, uint32_t after, uint32_t *set, uint32_t *clear);
size_t telamon_perm_text(uint32_t which, uint32_t mode, uint32_t who, bool isDir, uint8_t *out, size_t cap);
uint32_t telamon_current_uid();

int32_t telamon_sum_file(const uint8_t *path, size_t pathLen, uint32_t alg, const uint8_t *cancel, void (*progress)(void *, uint64_t), void *user, uint8_t *out,
                         size_t cap, size_t *len);
size_t telamon_sum_expected(const uint8_t *text, size_t len, uint32_t *alg, uint8_t *out, size_t cap);
size_t telamon_sum_name(uint32_t alg, uint8_t *out, size_t cap);
int32_t telamon_foldersize(const uint8_t *path, size_t len, const uint8_t *cancel, void (*progress)(void *, const TelamonTotals *), void *user,
                           TelamonTotals *totals);
}

namespace PropsBridge
{
// Calls `fn(uint8_t *out, size_t cap) -> size_t` with a buffer, again with a
// bigger one when the text did not fit.
template<typename F>
inline QByteArray bytesOf(F fn)
{
    QByteArray buf(256, 0);
    size_t n = fn(reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()));
    if (n > size_t(buf.size())) {
        buf.resize(qsizetype(n));
        n = fn(reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()));
    }
    buf.truncate(qsizetype(qMin(n, size_t(buf.size()))));
    return buf;
}

inline const uint8_t *p(const QByteArray &b)
{
    return reinterpret_cast<const uint8_t *>(b.constData());
}
inline size_t n(const QByteArray &b)
{
    return size_t(b.size());
}

// The tags of a local file, and how reading went: 0 read, 1 the file system
// keeps no attributes, 2 a link, 3 not readable, 4 not text.
struct TagsRead {
    QStringList names;
    uint32_t status = 3;
};

inline TagsRead readTags(const QString &path)
{
    TagsRead r;
    const QByteArray raw = QFile::encodeName(path);
    const QByteArray text = bytesOf([&](uint8_t *o, size_t c) { return telamon_tags_read(p(raw), n(raw), o, c, &r.status); });
    if (!text.isEmpty()) {
        r.names = QString::fromUtf8(text).split(QLatin1Char('\n'), Qt::SkipEmptyParts);
    }
    return r;
}

inline QString colourOf(const QString &name)
{
    const QByteArray b = name.toUtf8();
    return QString::fromUtf8(bytesOf([&](uint8_t *o, size_t c) { return telamon_tags_colour(p(b), n(b), o, c); }));
}

inline QString textOf(uint32_t which, uint32_t mode, uint32_t who, bool isDir)
{
    return QString::fromUtf8(bytesOf([&](uint8_t *o, size_t c) { return telamon_perm_text(which, mode, who, isDir, o, c); }));
}
}
