// image://thumb/<base64url of the file's URL>/<mtime>: thumbnails from KIO's
// PreviewJob (which reads and writes the freedesktop cache itself and runs
// the thumbnailers out of process). Local files only. A file with no
// thumbnail gives a 1x1 transparent image, so the view keeps its icon.
#pragma once

#include <QQuickAsyncImageProvider>

class ThumbnailProvider : public QQuickAsyncImageProvider
{
public:
    QQuickImageResponse *requestImageResponse(const QString &id, const QSize &requestedSize) override;
};
