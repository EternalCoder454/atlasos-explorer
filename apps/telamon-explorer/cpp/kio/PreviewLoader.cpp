#include "PreviewLoader.h"

#include "RustBridge.h"

#include <KIO/Global>

#include <QCoreApplication>
#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QImageReader>
#include <QMimeDatabase>
#include <QPointer>
#include <QThreadPool>

#include <atomic>

namespace
{
// Bytes of text kept: the core cuts markers' growth at this.
constexpr size_t MaxTextOut = 1024 * 1024;

QThreadPool &pool()
{
    // Two workers: stepping through a folder asks faster than files are read,
    // and an answer for a file that is no longer shown is not worth waiting for.
    static QThreadPool *p = [] {
        auto *q = new QThreadPool;
        q->setMaxThreadCount(2);
        return q;
    }();
    return *p;
}
}

struct PreviewLoader::Result {
    Category category = None;
    QString mime, kind, dimensions, text, note, thumb;
    QUrl play;
};

PreviewLoader::PreviewLoader(QObject *parent)
    : QObject(parent)
    , m_latest(std::make_shared<std::atomic<quint64>>(0))
{
}

PreviewLoader::~PreviewLoader()
{
    m_latest->store(~quint64(0));
}

void PreviewLoader::clear()
{
    m_latest->store(++m_serial);
    m_busy = false;
    m_category = None;
    m_mime.clear();
    m_kind.clear();
    m_dimensions.clear();
    m_text.clear();
    m_note.clear();
    m_thumb.clear();
    m_play.clear();
    Q_EMIT changed();
}

void PreviewLoader::load(const QString &localPath, bool isDir, const QString &fileName, const QString &typeText)
{
    const quint64 serial = ++m_serial;
    m_latest->store(serial);
    m_busy = true;
    m_category = None;
    m_mime.clear();
    m_kind = typeText;
    m_dimensions.clear();
    m_text.clear();
    m_note.clear();
    m_thumb.clear();
    m_play.clear();
    Q_EMIT changed();

    // Only an absolute path of this computer is ever opened.
    const QString path = QDir::isAbsolutePath(localPath) ? localPath : QString();
    pool().start([guard = QPointer<PreviewLoader>(this), latest = m_latest, serial, path, isDir, fileName, typeText] {
        const Result r = compute(path, isDir, fileName, typeText, latest, serial);
        // Delivered by the application object: the loader may be gone by now.
        QMetaObject::invokeMethod(
            qApp,
            [guard, serial, r] {
                if (guard) {
                    guard->apply(serial, r);
                }
            },
            Qt::QueuedConnection);
    });
}

void PreviewLoader::apply(quint64 serial, const Result &r)
{
    if (serial != m_serial) {
        return;
    }
    m_busy = false;
    m_category = r.category;
    m_mime = r.mime;
    if (!r.kind.isEmpty()) {
        m_kind = r.kind;
    }
    m_dimensions = r.dimensions;
    m_text = r.text;
    m_note = r.note;
    m_thumb = r.thumb;
    m_play = r.play;
    Q_EMIT changed();
}

// On a worker thread. Reads nothing once a newer file has been asked for.
PreviewLoader::Result PreviewLoader::compute(const QString &path, bool isDir, const QString &fileName, const QString &typeText,
                                             const std::shared_ptr<std::atomic<quint64>> &latest, quint64 serial)
{
    Result r;
    const auto stale = [&] { return latest->load() != serial; };
    QMimeDatabase db;
    if (isDir) {
        r.category = Folder;
        r.kind = typeText;
        return r;
    }
    if (path.isEmpty()) {
        // Not on this computer: no thumbnails and no reads (a server's file is
        // not fetched to be looked at). What it is comes from its name.
        const QMimeType mt = db.mimeTypeForFile(fileName, QMimeDatabase::MatchExtension);
        r.mime = mt.name();
        r.category = Category(rustPreviewClassify(mt.name()));
        r.note = QObject::tr("Files on other computers aren't previewed. Press Enter to open it.");
        return r;
    }
    const QFileInfo fi(path);
    if (!fi.exists()) {
        r.category = Other;
        r.note = QObject::tr("This file isn't there any more.");
        return r;
    }
    if (fi.isDir()) {
        r.category = Folder;
        r.kind = typeText;
        return r;
    }
    if (!fi.isFile()) {
        // A pipe, a device or a socket: never opened.
        r.category = Other;
        r.note = QObject::tr("This is a special file. There is nothing to preview.");
        return r;
    }
    const QMimeType mt = db.mimeTypeForFile(fi, QMimeDatabase::MatchDefault);
    r.mime = mt.name();
    if (!mt.isDefault() && !mt.comment().isEmpty()) {
        r.kind = mt.comment();
    }
    r.category = Category(rustPreviewClassify(mt.name()));
    if (r.category == Other && (mt.isDefault() || mt.inherits(QStringLiteral("text/plain")))) {
        r.category = Text;
    }
    if (stale()) {
        return r;
    }

    const auto thumbSource = [&] {
        const QByteArray id = QUrl::fromLocalFile(path).toEncoded().toBase64(QByteArray::Base64UrlEncoding | QByteArray::OmitTrailingEquals);
        return QStringLiteral("image://thumb/") + QString::fromLatin1(id) + QLatin1Char('/') + QString::number(fi.lastModified().toSecsSinceEpoch());
    };
    switch (r.category) {
    case Image: {
        // The header only; the picture itself is decoded by KIO's thumbnailer
        // in its own process. SVG is left alone (it is parsed as XML).
        if (!mt.inherits(QStringLiteral("image/svg+xml"))) {
            QImageReader reader(path);
            reader.setDecideFormatFromContent(true);
            const QSize s = reader.size();
            if (s.isValid()) {
                r.dimensions = rustPreviewText(1, (quint64(s.width()) << 32) | quint64(s.height()));
            }
        }
        r.thumb = thumbSource();
        break;
    }
    case Pdf:
    case Font:
    case Document:
    case Video:
    case Audio:
        r.thumb = thumbSource();
        if (r.category == Video || r.category == Audio) {
            r.play = QUrl::fromLocalFile(path);
        }
        break;
    case Text: {
        QByteArray out(int(MaxTextOut), 0);
        const QByteArray p = QFile::encodeName(path);
        uint32_t status = 3;
        bool truncated = false;
        const size_t n = telamon_preview_read_text(reinterpret_cast<const uint8_t *>(p.constData()), size_t(p.size()), reinterpret_cast<uint8_t *>(out.data()),
                                                   size_t(out.size()), &status, &truncated);
        if (status == 0) {
            r.text = QString::fromUtf8(out.constData(), qsizetype(qMin(n, size_t(out.size()))));
            if (truncated) {
                r.note = QObject::tr("Showing the first %1 of the file.").arg(KIO::convertSize(KIO::filesize_t(telamon_preview_text_cap())));
            }
        } else if (status == 1) {
            // The name or the type said text, the content did not.
            r.category = Other;
            r.note = QObject::tr("There is no preview for this kind of file.");
        } else if (status == 2) {
            r.category = Other;
            r.note = QObject::tr("This is a special file. There is nothing to preview.");
        } else {
            r.category = Other;
            r.note = QObject::tr("This file can't be read.");
        }
        break;
    }
    case Other:
        r.note = QObject::tr("There is no preview for this kind of file.");
        break;
    default:
        break;
    }
    return r;
}
