// Where the sidebar's places lead, and how a location is shown as text.
#pragma once

#include <QObject>
#include <QQmlEngine>
#include <QUrl>

class StandardPlaces : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON

public:
    explicit StandardPlaces(QObject *parent = nullptr);

    // home, desktop, documents, downloads, pictures, music, videos, recent,
    // network or trash; an unknown key gives the home folder.
    Q_INVOKABLE QUrl place(const QString &key) const;
    // The folder above url (url itself at the top).
    Q_INVOKABLE QUrl parentUrl(const QUrl &url) const;
    // A location as plain text safe to show: every path segment goes through
    // the core crate's display names.
    Q_INVOKABLE QString displayLocation(const QUrl &url) const;
};
