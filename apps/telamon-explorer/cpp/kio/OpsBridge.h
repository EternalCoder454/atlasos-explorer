// The operation queue, undo history, conflict rules and pre-flight checks of
// the Rust core (src/ops_ffi.rs), as C++ calls. No logic lives here, only
// buffers.
#pragma once

#include <QByteArray>
#include <QString>
#include <QStringList>
#include <QUrl>

#include <cstddef>
#include <cstdint>

extern "C" {
struct TelamonOpInfo {
    uint64_t id;
    uint32_t kind;
    uint32_t state;
    uint64_t bytes_done;
    uint64_t bytes_total;
    uint64_t items_done;
    uint64_t items_total;
    double speed;
    int64_t time_left_ms;
    uint32_t pending;
};
void *telamon_ops_new();
void telamon_ops_free(void *h);
uint64_t telamon_ops_add(void *h, uint32_t kind, const uint8_t *label, size_t len);
bool telamon_ops_next_action(void *h, uint32_t *kind, uint64_t *id);
void telamon_ops_started(void *h, uint64_t id);
void telamon_ops_progress(void *h, uint64_t id, uint64_t bytesDone, uint64_t bytesTotal, uint64_t itemsDone, uint64_t itemsTotal);
void telamon_ops_event(void *h, uint64_t id, uint32_t what);
void telamon_ops_failed(void *h, uint64_t id, const uint8_t *reason, size_t len);
uint32_t telamon_ops_conflict(void *h, uint64_t id, uint32_t kind);
bool telamon_ops_answer(void *h, uint64_t id, uint32_t answer, bool applyToAll);
bool telamon_ops_info(void *h, uint64_t id, TelamonOpInfo *out);
size_t telamon_ops_ids(void *h, uint64_t *out, size_t cap);
size_t telamon_ops_text(void *h, uint64_t id, uint32_t which, uint8_t *out, size_t cap);
size_t telamon_ops_dismiss(void *h, uint64_t id);
bool telamon_hist_record(void *h, const uint8_t *title, size_t titleLen, const uint8_t *rec, size_t recLen);
void telamon_hist_barrier(void *h);
size_t telamon_hist_titles(void *h, uint32_t side, uint8_t *out, size_t cap);
size_t telamon_hist_paths(void *h, uint32_t side, bool after, uint8_t *out, size_t cap);
int32_t telamon_hist_plan(void *h, uint32_t side, const uint8_t *states, size_t statesLen, uint8_t *out, size_t cap, size_t *len);
size_t telamon_hist_complete(void *h, uint32_t side, const uint8_t *states, size_t statesLen, const uint8_t *trashes, size_t trashesLen, uint8_t *out,
                           size_t cap);
void telamon_hist_drop(void *h, uint32_t side);
size_t telamon_op_text(uint32_t which, uint32_t kind, const uint8_t *names, size_t namesLen, size_t count, const uint8_t *to, size_t toLen,
                     const uint8_t *newName, size_t newLen, uint8_t *out, size_t cap);
size_t telamon_format_size(uint64_t bytes, uint8_t *out, size_t cap);
uint32_t telamon_conflict_choices(bool sourceIsDir, bool destIsDir, bool sameFile);
bool telamon_conflict_allowed(bool sourceIsDir, bool destIsDir, bool sameFile, uint32_t answer);
uint32_t telamon_conflict_newer(bool hasSource, int64_t source, bool hasDest, int64_t dest);
bool telamon_conflict_endangers(uint32_t answer);
size_t telamon_keep_both(const uint8_t *dir, size_t dirLen, const uint8_t *name, size_t nameLen, uint8_t *out, size_t cap);
bool telamon_is_inside(const uint8_t *source, size_t sourceLen, const uint8_t *dest, size_t destLen);
size_t telamon_into_itself_text(uint32_t transfer, const uint8_t *folder, size_t folderLen, const uint8_t *dest, size_t destLen, bool same, uint8_t *out,
                              size_t cap);
int32_t telamon_room_check(uint64_t needed, const uint8_t *dest, size_t destLen, const uint8_t *what, size_t whatLen, int64_t freeOverride, uint8_t *out,
                         size_t cap, size_t *len);
int32_t telamon_preflight(uint32_t transfer, const uint8_t *sources, size_t sourcesLen, const uint8_t *dest, size_t destLen, int64_t freeOverride, uint8_t *out,
                        size_t cap, size_t *len);
}

namespace OpsBridge
{
// Calls `fn(uint8_t *out, size_t cap) -> size_t` with a buffer, again with a
// bigger one when the text did not fit, and returns what it wrote.
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

template<typename F>
inline QString textOf(F fn)
{
    return QString::fromUtf8(bytesOf(fn));
}

inline const uint8_t *p(const QByteArray &b)
{
    return reinterpret_cast<const uint8_t *>(b.constData());
}
inline size_t n(const QByteArray &b)
{
    return size_t(b.size());
}
}
