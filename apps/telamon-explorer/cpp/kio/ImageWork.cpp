#include "ImageWork.h"

#include <QCoreApplication>
#include <QUrl>

#include "RustBridge.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QImage>
#include <QImageReader>
#include <QImageWriter>
#include <QMarginsF>
#include <QPageSize>
#include <QPainter>
#include <QPdfWriter>
#include <QTransform>

#include <turbojpeg.h>

#include <climits>
#include <cmath>

namespace
{
QString tr(const char *text)
{
    return QCoreApplication::translate("ImageWork", text);
}

// Whether the core allows a file of this size (and picture of this many pixels).
QString checkInput(quint64 size, quint64 pixels)
{
    QByteArray why(128, 0);
    const size_t n = telamon_image_check_input(size, pixels, reinterpret_cast<uint8_t *>(why.data()), size_t(why.size()));
    if (n == 0) {
        return {};
    }
    return QCoreApplication::translate("ImageWork", "It can't be used because %1.").arg(QString::fromUtf8(why.constData(), qsizetype(qMin(n, size_t(why.size())))));
}

// A picture read as it is shown (the EXIF orientation applied), after the
// checks: a regular file, not too big, not too many pixels, Qt can read it.
QString readPicture(const QString &path, QImage *out)
{
    const QFileInfo info(path);
    if (!info.isFile()) {
        return tr("It isn't a file.");
    }
    if (const QString why = checkInput(quint64(info.size()), 0); !why.isEmpty()) {
        return why;
    }
    QImageReader reader(path);
    reader.setAutoTransform(true);
    reader.setAllocationLimit(1024);
    if (!reader.canRead()) {
        return tr("It can't be read as a picture.");
    }
    const QSize size = reader.size();
    if (size.isValid()) {
        if (const QString why = checkInput(0, quint64(size.width()) * quint64(size.height())); !why.isEmpty()) {
            return why;
        }
    }
    QImage img = reader.read();
    if (img.isNull()) {
        return tr("It can't be read as a picture.");
    }
    if (const QString why = checkInput(0, quint64(img.width()) * quint64(img.height())); !why.isEmpty()) {
        return why;
    }
    *out = img;
    return {};
}

// A picture with see-through parts, laid on white for a format that has none.
QImage onWhite(const QImage &img)
{
    if (!img.hasAlphaChannel()) {
        return img;
    }
    QImage flat(img.size(), QImage::Format_RGB32);
    flat.fill(Qt::white);
    QPainter p(&flat);
    p.drawImage(0, 0, img);
    return flat;
}

QString write(const QImage &img, const QString &outPath, const QByteArray &format, int quality)
{
    QImageWriter writer(outPath, format);
    if (quality >= 0) {
        writer.setQuality(quality);
    }
    QImage toWrite = format == "jpeg" ? onWhite(img) : img;
    if (!writer.write(toWrite)) {
        QFile::remove(outPath);
        return tr("The new picture couldn't be written: %1").arg(writer.errorString());
    }
    return {};
}

// A JPEG turned without decoding it: the pixels are not touched, and the
// orientation the camera wrote is made upright with it. "" when done; `*perfect`
// is false when the picture's size does not allow a lossless turn.
QString turnJpegLossless(const QString &path, const QString &outPath, bool clockwise, bool *perfect)
{
    *perfect = true;
    QFile in(path);
    if (!in.open(QIODevice::ReadOnly)) {
        return tr("It can't be read.");
    }
    const QByteArray data = in.readAll();
    if (data.isEmpty()) {
        return tr("It can't be read.");
    }
    const uint32_t orientation = telamon_jpeg_orientation(reinterpret_cast<const uint8_t *>(data.constData()), size_t(data.size()));
    tjhandle h = tj3Init(TJINIT_TRANSFORM);
    if (!h) {
        *perfect = false;
        return {};
    }
    tj3Set(h, TJPARAM_MAXPIXELS, int(qMin<quint64>(telamon_image_limit(2), INT_MAX)));
    tjtransform xform{};
    xform.op = int(telamon_jpeg_turn(orientation, clockwise));
    // Perfect: no edge is cut. Optimized: the Huffman tables are made for these coefficients (the default ones make the file bigger).
    xform.options = TJXOPT_PERFECT | TJXOPT_OPTIMIZE;
    unsigned char *dst = nullptr;
    size_t dstSize = 0;
    const int rc = tj3Transform(h, reinterpret_cast<const unsigned char *>(data.constData()), size_t(data.size()), 1, &dst, &dstSize, &xform);
    tj3Destroy(h);
    if (rc != 0 || !dst || dstSize == 0) {
        tj3Free(dst);
        *perfect = false;
        return {};
    }
    QByteArray result(reinterpret_cast<const char *>(dst), qsizetype(dstSize));
    tj3Free(dst);
    // The pixels are upright now: the EXIF says so too.
    telamon_jpeg_reset_orientation(reinterpret_cast<uint8_t *>(result.data()), size_t(result.size()));
    QFile out(outPath);
    if (!out.open(QIODevice::WriteOnly | QIODevice::Truncate) || out.write(result) != result.size() || !out.flush()) {
        out.close();
        QFile::remove(outPath);
        return tr("The new picture couldn't be written.");
    }
    return {};
}

// One page the size of the picture (150 dots to the inch, less for a huge
// one: a PDF page is at most 200 inches).
QString pictureToPdf(const QString &path, const QString &pdfPath)
{
    QImage img;
    if (const QString why = readPicture(path, &img); !why.isEmpty()) {
        return why;
    }
    double dpi = 150.0;
    const double longEdgeInches = qMax(img.width(), img.height()) / dpi;
    if (longEdgeInches > 190.0) {
        dpi = std::ceil(qMax(img.width(), img.height()) / 190.0);
    }
    QPdfWriter pdf(pdfPath);
    pdf.setResolution(int(dpi));
    pdf.setPageMargins(QMarginsF(0, 0, 0, 0));
    pdf.setPageSize(QPageSize(QSizeF(img.width() * 72.0 / dpi, img.height() * 72.0 / dpi), QPageSize::Point, QString(), QPageSize::ExactMatch));
    pdf.setCreator(QStringLiteral("Files"));
    QPainter painter(&pdf);
    if (!painter.isActive()) {
        return tr("The PDF couldn't be written.");
    }
    painter.drawImage(QRect(0, 0, img.width(), img.height()), img);
    painter.end();
    return QFileInfo(pdfPath).size() > 0 ? QString() : tr("The PDF couldn't be written.");
}
}

namespace ImageWork
{
bool canWrite(const QByteArray &format)
{
    return QImageWriter::supportedImageFormats().contains(format);
}

QString rotate(const Source &in, const QString &outPath, bool clockwise)
{
    if (in.kind == Jpeg) {
        const QFileInfo info(in.path);
        if (!info.isFile()) {
            return tr("It isn't a file.");
        }
        if (const QString why = checkInput(quint64(info.size()), 0); !why.isEmpty()) {
            return why;
        }
        bool perfect = true;
        const QString why = turnJpegLossless(in.path, outPath, clockwise, &perfect);
        if (!why.isEmpty()) {
            return why;
        }
        if (perfect) {
            return {};
        }
        // The picture's size is not a multiple of a JPEG block, so a turn
        // without decoding would cut its edge: it is decoded and written again.
    }
    QImage img;
    if (const QString why = readPicture(in.path, &img); !why.isEmpty()) {
        return why;
    }
    const QImage turned = img.transformed(QTransform().rotate(clockwise ? 90 : -90));
    switch (in.kind) {
    case Jpeg:
        return write(turned, outPath, "jpeg", 95);
    case Webp:
        // 100 is WebP's lossless mode: turning must not lose anything.
        return write(turned, outPath, "webp", 100);
    case Bmp:
        return write(turned, outPath, "bmp", -1);
    default:
        return write(turned, outPath, "png", -1);
    }
}

QString convert(const Source &in, const QString &outPath, Action to)
{
    QImage img;
    if (const QString why = readPicture(in.path, &img); !why.isEmpty()) {
        return why;
    }
    switch (to) {
    case ToJpeg:
        return write(img, outPath, "jpeg", 92);
    case ToWebp:
        return write(img, outPath, "webp", 90);
    default:
        return write(img, outPath, "png", -1);
    }
}

QString combine(const QList<Source> &in, const QString &outPath, const QString &tempDir, const std::atomic<bool> *cancel, int *bad)
{
    *bad = -1;
    QByteArray paths;
    int n = 0;
    for (const Source &s : in) {
        if (cancel && cancel->load()) {
            return QStringLiteral("cancelled");
        }
        QString pdf = s.path;
        if (s.kind != Pdf) {
            pdf = QDir(tempDir).filePath(QStringLiteral("page-%1.pdf").arg(n));
            if (const QString why = pictureToPdf(s.path, pdf); !why.isEmpty()) {
                *bad = n;
                return why;
            }
        }
        paths += QFile::encodeName(pdf);
        paths.append('\0');
        ++n;
    }
    const QByteArray out = QFile::encodeName(outPath);
    uint32_t pages = 0, badIndex = UINT32_MAX;
    const int rc = telamon_pdf_merge(reinterpret_cast<const uint8_t *>(paths.constData()), size_t(paths.size()), reinterpret_cast<const uint8_t *>(out.constData()),
                                   size_t(out.size()), reinterpret_cast<const uint8_t *>(cancel), &pages, &badIndex);
    if (badIndex != UINT32_MAX) {
        *bad = int(badIndex);
    }
    if (rc != 0) {
        QFile::remove(outPath);
    }
    switch (rc) {
    case 0:
        return {};
    case 1:
        return tr("It can't be read as a PDF.");
    case 2:
        return tr("It is protected with a password, and Files doesn't take protection off.");
    case 3:
        return tr("It has no pages.");
    case 4:
        return tr("It is too big to be joined.");
    case 5:
        return tr("The result would have more than 5,000 pages.");
    case 6:
        return tr("The new PDF couldn't be written.");
    case 7:
        return QStringLiteral("cancelled");
    default:
        return tr("Nothing to join.");
    }
}
}
