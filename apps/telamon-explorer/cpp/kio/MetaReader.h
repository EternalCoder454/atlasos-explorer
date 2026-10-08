// What KFileMetaData knows about a file on this computer: the picture's or
// video's size, its length, when a photo was taken, the camera, the pages of
// a document. Reads the file, so it runs on a worker, never the GUI thread.
// Used by the Details view's optional columns and by Properties.
#pragma once

#include <QString>
#include <QVariantList>

namespace MetaReader
{
struct Info {
    // "4000 × 3000"
    QString dimensions;
    // "1:23"
    QString duration;
    // When a photo was taken, written in the user's locale.
    QString taken;
    // Everything known, for Properties: [{label, value}], in a fixed order.
    QVariantList rows;
};

// Never throws, never reads a file that is not a regular file; empty fields
// for what is not known.
Info read(const QString &path);
}
