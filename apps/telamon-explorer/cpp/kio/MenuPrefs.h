// Which entries of the context menus are hidden (Settings > Context Menu and
// Actions): every built-in entry, every service-menu action and every plugin
// of KDE's file-item actions has a switch. The set is text in Files' settings
// file (telamon-explorerrc, [Menu], key Hidden) that the Rust core reads and
// filters the menu model by (atlas_explorer_core::menuprefs). The list of
// service menus and plugins is read from the disk on a worker when the
// Settings window asks.
#pragma once

#include <QObject>
#include <QQmlEngine>
#include <QStringList>
#include <QVariantList>

class MenuPrefs : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    // The keys of the built-in entries, in the order Settings lists them.
    Q_PROPERTY(QStringList builtinKeys READ builtinKeys CONSTANT)
    // [{key, text, detail, kind: "service" | "plugin"}] found on this computer.
    Q_PROPERTY(QVariantList services READ services NOTIFY servicesChanged)
    Q_PROPERTY(bool scanning READ scanning NOTIFY servicesChanged)
    // Bumped when the hidden set changes, so a binding on isHidden() reads again.
    Q_PROPERTY(int revision READ revision NOTIFY changed)

public:
    explicit MenuPrefs(QObject *parent = nullptr);

    QStringList builtinKeys() const { return m_builtin; }
    QVariantList services() const { return m_services; }
    bool scanning() const { return m_scanning; }
    int revision() const { return m_revision; }

    Q_INVOKABLE bool isHidden(const QString &key) const;
    Q_INVOKABLE void setHidden(const QString &key, bool hidden);
    // Looks for the service menus and plugins again (on a worker).
    Q_INVOKABLE void scan();

    // The hidden set as the core takes it, and the names for KFileItemActions'
    // exclude list. Read from the settings file when asked.
    static QByteArray hiddenText();
    static QStringList excludedServices();
    // The entries of a menu snapshot's state without the hidden ones is the core's job.

Q_SIGNALS:
    void changed();
    void servicesChanged();

private:
    QStringList m_builtin;
    QByteArray m_text;
    QVariantList m_services;
    bool m_scanning = false;
    int m_revision = 0;
    int m_scanSerial = 0;
};
