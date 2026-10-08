// What Quick Look and the preview pane show for one file. QML gives it the
// file (`load`); a worker finds out what kind it is (MIME sniffing), its
// picture size and, for text, the first 256 KiB as safe plain text (the
// core's reader, which opens regular files only); the answer comes back to
// the GUI thread and an answer to an older file is dropped. Pictures are not
// made here: `thumbnailSource` is an image://thumb URL (ThumbnailProvider,
// KIO's thumbnailers in their own process). Local files only.
#pragma once

#include <QObject>
#include <QQmlEngine>
#include <QUrl>

#include <memory>

class PreviewLoader : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    // Same order as atlas_explorer_core::preview::Category.
    Q_PROPERTY(Category category READ category NOTIFY changed)
    // True from load() until the worker has answered.
    Q_PROPERTY(bool busy READ busy NOTIFY changed)
    Q_PROPERTY(QString mimeName READ mimeName NOTIFY changed)
    // The kind in words ("PNG image"), from the content when it is a local file.
    Q_PROPERTY(QString kindText READ kindText NOTIFY changed)
    // "4000 × 3000" for a picture whose size is known, else empty.
    Q_PROPERTY(QString dimensionsText READ dimensionsText NOTIFY changed)
    // Plain text for the Text category; never markup.
    Q_PROPERTY(QString text READ text NOTIFY changed)
    // A line to show instead of a preview, or under it ("Showing the first 256 KiB.").
    Q_PROPERTY(QString note READ note NOTIFY changed)
    // image://thumb/... for categories KIO thumbnails; empty for the rest.
    Q_PROPERTY(QString thumbnailSource READ thumbnailSource NOTIFY changed)
    // The file as a URL for the player (Audio and Video of this computer).
    Q_PROPERTY(QUrl playUrl READ playUrl NOTIFY changed)

public:
    enum Category { None, Folder, Image, Pdf, Video, Audio, Font, Document, Text, Other };
    Q_ENUM(Category)

    explicit PreviewLoader(QObject *parent = nullptr);
    ~PreviewLoader() override;

    Category category() const { return m_category; }
    bool busy() const { return m_busy; }
    QString mimeName() const { return m_mime; }
    QString kindText() const { return m_kind; }
    QString dimensionsText() const { return m_dimensions; }
    QString text() const { return m_text; }
    QString note() const { return m_note; }
    QString thumbnailSource() const { return m_thumb; }
    QUrl playUrl() const { return m_play; }

    // Starts finding out about a file. `localPath` is empty for a file that
    // is not on this computer (then nothing is read and only the name's
    // extension says what it is). `typeText` is the kind as the folder view
    // wrote it, used until the content says better.
    Q_INVOKABLE void load(const QString &localPath, bool isDir, const QString &fileName, const QString &typeText);
    Q_INVOKABLE void clear();

Q_SIGNALS:
    void changed();

private:
    struct Result;
    void apply(quint64 serial, const Result &r);
    static Result compute(const QString &path, bool isDir, const QString &fileName, const QString &typeText, const std::shared_ptr<std::atomic<quint64>> &latest,
                          quint64 serial);

    Category m_category = None;
    bool m_busy = false;
    QString m_mime, m_kind, m_dimensions, m_text, m_note, m_thumb;
    QUrl m_play;
    quint64 m_serial = 0;
    // The newest serial asked for, which a worker looks at before it reads anything.
    std::shared_ptr<std::atomic<quint64>> m_latest;
};
