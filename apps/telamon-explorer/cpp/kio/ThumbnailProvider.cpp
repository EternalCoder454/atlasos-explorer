#include "ThumbnailProvider.h"
#include "ServerLogic.h"

#include <KFileItem>
#include <KIO/PreviewJob>

#include <QCoreApplication>
#include <QFileInfo>
#include <algorithm>
#include <QPointer>
#include <QQuickTextureFactory>
#include <QTimer>

#include <sys/stat.h>

namespace
{
class ThumbResponse : public QQuickImageResponse
{
public:
    ThumbResponse(const QString &id, const QSize &requested)
    {
        // "<base64url url>/<mtime>"; the mtime only keys Qt's image cache.
        const QByteArray raw = QByteArray::fromBase64(id.section(QLatin1Char('/'), 0, 0).toLatin1(), QByteArray::Base64UrlEncoding);
        const QUrl url = QUrl::fromEncoded(raw);
        const int side = requested.isValid() ? std::clamp(std::max(requested.width(), requested.height()), 32, 1024) : 128;
        // Files on this computer; those on a server only if the Settings switch is on.
        const bool allowed = url.isLocalFile() || (ServerLogic::previewRemoteEnabled() && ServerLogic::isServerScheme(url.scheme()));
        if (!url.isValid() || !allowed) {
            QTimer::singleShot(0, this, [this] { finish(QImage()); });
            return;
        }
        // Qt asks from its image reader thread; KIO jobs belong on the GUI
        // thread, so the response moves there and starts the job from it.
        moveToThread(QCoreApplication::instance()->thread());
        QTimer::singleShot(0, this, [this, url, side] { start(url, side); });
    }

    void start(const QUrl &url, int side)
    {
        if (m_done) {
            return;
        }
        // A regular file; KIO's PreviewJob does the MIME check and size caps.
        const KFileItem item(url, QString(), S_IFREG);
        m_job = KIO::filePreview(KFileItemList{item}, QSize(side, side), &plugins());
        m_job->setAutoDelete(true);
        connect(m_job, &KIO::PreviewJob::gotPreview, this, [this](const KFileItem &, const QPixmap &p) { finish(p.toImage()); });
        connect(m_job, &KIO::PreviewJob::failed, this, [this](const KFileItem &) { finish(QImage()); });
        // A thumbnailer that hangs costs this file its thumbnail only.
        QTimer::singleShot(30000, this, [this] {
            if (!m_done) {
                if (m_job) {
                    m_job->kill();
                }
                finish(QImage());
            }
        });
    }

    QQuickTextureFactory *textureFactory() const override { return QQuickTextureFactory::textureFactoryForImage(m_image); }
    QString errorString() const override { return QString(); }

    // Called from Qt's reader thread: the kill runs on the GUI thread.
    void cancel() override
    {
        QMetaObject::invokeMethod(this, [this] {
            m_done = true;
            if (m_job) {
                m_job->kill();
            }
        });
    }

private:
    static const QStringList &plugins()
    {
        static const QStringList list = KIO::PreviewJob::defaultPlugins();
        return list;
    }

    void finish(QImage image)
    {
        if (m_done) {
            return;
        }
        m_done = true;
        const bool failed = image.isNull();
        if (failed) {
            image = QImage(1, 1, QImage::Format_ARGB32_Premultiplied);
            image.fill(Qt::transparent);
        }
        m_image = image;
        Q_EMIT finished();
    }

    QPointer<KIO::PreviewJob> m_job;
    QImage m_image;
    bool m_done = false;
};
}

QQuickImageResponse *ThumbnailProvider::requestImageResponse(const QString &id, const QSize &requestedSize)
{
    return new ThumbResponse(id, requestedSize);
}
