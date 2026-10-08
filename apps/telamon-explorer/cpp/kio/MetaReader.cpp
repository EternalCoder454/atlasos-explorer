#include "MetaReader.h"

#include <QUrl>

#include "RustBridge.h"

#include <KFileMetaData/ExtractorCollection>
#include <KFileMetaData/PropertyInfo>
#include <KFileMetaData/SimpleExtractionResult>

#include <QFileInfo>
#include <QImageReader>
#include <QLocale>
#include <QMimeDatabase>

#include <algorithm>

using namespace KFileMetaData;

namespace
{
QString cleanText(const QString &s)
{
    // Tags inside files are untrusted text: made visible, never markup.
    return rustDisplayName(s.toUtf8());
}

QString whenText(const QVariant &v)
{
    const QDateTime t = v.toDateTime();
    return t.isValid() ? QLocale().toString(t, QLocale::ShortFormat) : cleanText(v.toString());
}
}

MetaReader::Info MetaReader::read(const QString &path)
{
    Info info;
    const QFileInfo fi(path);
    if (!fi.isFile()) {
        return info;
    }
    QMimeDatabase db;
    const QString mime = db.mimeTypeForFile(fi).name();
    // One collection a thread: plugins are loaded once for it.
    thread_local ExtractorCollection collection;
    SimpleExtractionResult result(path, mime, ExtractionResult::ExtractMetaData);
    for (Extractor *ex : collection.fetchExtractors(mime)) {
        ex->extract(&result);
    }
    const PropertyMultiMap props = result.properties();

    int width = props.value(Property::Width).toInt();
    int height = props.value(Property::Height).toInt();
    if ((width <= 0 || height <= 0) && mime.startsWith(QLatin1String("image/")) && !mime.contains(QLatin1String("svg"))) {
        // The header only.
        QImageReader reader(path);
        reader.setDecideFormatFromContent(true);
        const QSize s = reader.size();
        if (s.isValid()) {
            width = s.width();
            height = s.height();
        }
    }
    if (width > 0 && height > 0) {
        info.dimensions = rustPreviewText(1, (quint64(width) << 32) | quint64(height));
    }
    const qint64 seconds = props.value(Property::Duration).toLongLong();
    if (seconds > 0) {
        info.duration = rustPreviewText(0, quint64(seconds) * 1000);
    }
    for (Property::Property p : {Property::PhotoDateTimeOriginal, Property::ImageDateTime}) {
        if (props.contains(p)) {
            info.taken = whenText(props.value(p));
            break;
        }
    }

    auto add = [&](const QString &label, const QString &value) {
        if (!value.isEmpty()) {
            info.rows.append(QVariantMap{{QStringLiteral("label"), label}, {QStringLiteral("value"), value}});
        }
    };
    auto shown = [&](Property::Property p) {
        QStringList parts;
        const PropertyInfo pi(p);
        const auto values = props.values(p);
        // values() lists the newest first; the file's order is the reverse.
        for (auto it = values.crbegin(); it != values.crend(); ++it) {
            const QString t = cleanText(pi.formatAsDisplayString(*it));
            if (!t.isEmpty() && !parts.contains(t)) {
                parts << t;
            }
            if (parts.size() >= 8) {
                break;
            }
        }
        return parts.join(QStringLiteral(", "));
    };
    auto labelled = [&](Property::Property p, const QString &label = QString()) { add(label.isEmpty() ? PropertyInfo(p).displayName() : label, shown(p)); };

    labelled(Property::Title);
    labelled(Property::Artist);
    labelled(Property::Album);
    labelled(Property::AlbumArtist);
    labelled(Property::Genre);
    labelled(Property::TrackNumber);
    labelled(Property::ReleaseYear);
    add(QObject::tr("Dimensions"), info.dimensions);
    add(QObject::tr("Duration"), info.duration);
    labelled(Property::FrameRate);
    labelled(Property::VideoCodec);
    labelled(Property::AudioCodec);
    labelled(Property::BitRate);
    labelled(Property::SampleRate);
    labelled(Property::Channels);
    add(QObject::tr("Date Taken"), info.taken);
    const QString make = shown(Property::Manufacturer);
    const QString model = shown(Property::Model);
    add(QObject::tr("Camera"), (model.startsWith(make, Qt::CaseInsensitive) ? model : (make + QLatin1Char(' ') + model)).trimmed());
    labelled(Property::PhotoFNumber);
    labelled(Property::PhotoExposureTime);
    labelled(Property::PhotoISOSpeedRatings);
    labelled(Property::PhotoFocalLength);
    labelled(Property::PageCount, QObject::tr("Pages"));
    labelled(Property::WordCount);
    labelled(Property::Author);
    labelled(Property::Subject);
    labelled(Property::Generator);
    if (!props.contains(Property::PhotoDateTimeOriginal) && props.contains(Property::CreationDate)) {
        add(QObject::tr("Created"), whenText(props.value(Property::CreationDate)));
    }
    labelled(Property::Language);
    labelled(Property::Copyright);
    labelled(Property::Description);
    if (info.rows.size() > 40) {
        info.rows = info.rows.mid(0, 40);
    }
    return info;
}
