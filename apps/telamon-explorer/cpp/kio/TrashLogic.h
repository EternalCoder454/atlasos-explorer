// The Trash setting "Empty items older than N days" (off by default): where
// it is kept (telamon-explorerrc, [Trash]), when it runs (a few seconds after
// Files starts and once a day while it is open) and the question asked before
// it is turned on. Removing is the operation queue's (FileActions.emptyOldTrash);
// the rules for what counts as old are the core's (atlas_explorer_core::trash).
#pragma once

#include <QObject>
#include <QQmlEngine>
#include <QTimer>

class TrashLogic : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(bool autoEmpty READ autoEmpty NOTIFY autoEmptyChanged)
    Q_PROPERTY(int days READ days NOTIFY autoEmptyChanged)
    Q_PROPERTY(int minDays READ minDays CONSTANT)
    Q_PROPERTY(int maxDays READ maxDays CONSTANT)

public:
    explicit TrashLogic(QObject *parent = nullptr);

    bool autoEmpty() const { return m_on; }
    int days() const { return m_days; }
    int minDays() const;
    int maxDays() const;

    // Files started: when the switch is on, the first run comes shortly, and
    // then one every day.
    Q_INVOKABLE void begin();
    // The switch was moved. Turning it on asks first (`confirmRequested`); off is at once.
    Q_INVOKABLE void requestAutoEmpty(bool on);
    // The number of days was changed. A smaller number while on asks first,
    // when it would remove something now; anything else is kept at once and
    // is used at the next run.
    Q_INVOKABLE void requestDays(int days);
    // The answer to `confirmRequested`: Yes turns it on (or changes the days)
    // and runs now; No leaves everything as it was.
    Q_INVOKABLE void confirm();
    Q_INVOKABLE void cancel();

Q_SIGNALS:
    void autoEmptyChanged();
    // Asks the user: the question's text.
    void confirmRequested(const QString &text);
    // Time to empty what is old: FileActions.emptyOldTrash(days) does it.
    void emptyOldRequested(int days);

private:
    void ask(bool on, int days);
    void apply(bool on, int days);
    void schedule();

    bool m_on = false;
    int m_days = 30;
    bool m_askOn = false;
    int m_askDays = 30;
    quint64 m_askSerial = 0;
    QTimer m_daily;
};
