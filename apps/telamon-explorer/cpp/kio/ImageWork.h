// Picture work for the quick actions of More Actions (Rotate, Convert, Combine
// into PDF). Everything here runs on a worker thread and touches nothing but
// the files it is given and the folder it writes to: the originals are only
// read, the results are new files in a scratch folder (the queue then copies
// them into place, with the conflict dialog and Undo). What may be done, and
// what the new files are called, is decided by the core (src/image_ffi.rs).
#pragma once

#include <QList>
#include <QString>
#include <QStringList>

#include <atomic>

namespace ImageWork
{
// The numbers of the core's actions (imageops::Action).
enum Action { RotateLeft = 0, RotateRight = 1, ToPng = 2, ToJpeg = 3, ToWebp = 4, CombinePdf = 5 };
// The core's kinds (image_ffi.rs: telamon_image_kind).
enum Kind { NotAPicture = 0, Jpeg = 1, Png = 2, Webp = 3, Bmp = 4, OtherRaster = 5, Pdf = 6 };

struct Source {
    // The file's path on this computer.
    QString path;
    // Its Kind.
    int kind = NotAPicture;
};

// Whether Qt can write `format` ("png", "jpeg", "webp", "bmp") here.
bool canWrite(const QByteArray &format);

// Each returns "" when done, else why not in plain words. The message does
// not name the file (the caller does).
QString rotate(const Source &in, const QString &outPath, bool clockwise);
QString convert(const Source &in, const QString &outPath, Action to);
// One PDF made of the pictures and PDFs in `in`, in that order; `tempDir` is
// a folder of the caller's for the one-page PDFs made from pictures. `bad` is
// the position of the file that stopped it.
QString combine(const QList<Source> &in, const QString &outPath, const QString &tempDir, const std::atomic<bool> *cancel, int *bad);
}
