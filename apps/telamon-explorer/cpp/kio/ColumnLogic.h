// Which optional columns the Details view shows: Tags, Dimensions, Duration
// and Date Taken. Kept in telamon-explorerrc, [View], for every folder.
// All four are off until switched on. Dimensions, Duration and Date Taken open
// each file (on a worker, for the rows on screen only); Tags reads one
// attribute of each row, which the colour dots need anyway.
#pragma once

#include <QObject>
#include <QQmlEngine>

class ColumnLogic : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(bool tags READ tags WRITE setTags NOTIFY changed)
    Q_PROPERTY(bool dimensions READ dimensions WRITE setDimensions NOTIFY changed)
    Q_PROPERTY(bool duration READ duration WRITE setDuration NOTIFY changed)
    Q_PROPERTY(bool taken READ taken WRITE setTaken NOTIFY changed)
    // Any of the three that need the files read.
    Q_PROPERTY(bool anyMeta READ anyMeta NOTIFY changed)

public:
    explicit ColumnLogic(QObject *parent = nullptr);

    bool tags() const { return m_tags; }
    bool dimensions() const { return m_dimensions; }
    bool duration() const { return m_duration; }
    bool taken() const { return m_taken; }
    bool anyMeta() const { return m_dimensions || m_duration || m_taken; }
    void setTags(bool on);
    void setDimensions(bool on);
    void setDuration(bool on);
    void setTaken(bool on);

Q_SIGNALS:
    void changed();

private:
    void set(bool &field, bool on, const char *key);

    bool m_tags = false;
    bool m_dimensions = false;
    bool m_duration = false;
    bool m_taken = false;
};
