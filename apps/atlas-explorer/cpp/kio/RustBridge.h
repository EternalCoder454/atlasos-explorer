// The Rust core's display names, keys and sorting (src/ffi.rs), as C++ calls.
// No logic lives here, only buffers.
#pragma once

#include <QByteArray>
#include <QString>

#include <cstddef>
#include <cstdint>

extern "C" {
struct AtlasSortRow {
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
size_t atlas_display_name(const uint8_t *name, size_t len, uint8_t *out, size_t cap);
size_t atlas_name_key(const uint8_t *name, size_t len, uint8_t *out, size_t cap);
bool atlas_sort_permutation(const AtlasSortRow *rows, size_t n, uint32_t column, bool descending, bool foldersFirst, uint32_t *out);
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
    return QString::fromUtf8(rustBytes(atlas_display_name, name));
}

inline QByteArray rustNameKey(const QByteArray &name)
{
    return rustBytes(atlas_name_key, name);
}
