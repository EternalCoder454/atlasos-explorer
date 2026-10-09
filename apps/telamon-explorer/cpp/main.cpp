// Starts Qt, makes Explorer single-instance and loads the window. A second
// launch (the launcher icon, a folder opened from another app,
// `telamon-explorer --select <file>`) hands its arguments to this one and
// exits; they are read in Rust (src/backend.rs), never here.
#include "FileManager1.h"
#include "kio/ThumbnailProvider.h"

#include <telamon/app.h>

#include <KDBusService>
#include <KWindowSystem>

#include <QApplication>
#include <QCommandLineParser>
#include <QDBusConnection>
#include <QDir>
#include <QQmlApplicationEngine>
#include <QQuickWindow>
#include <QSGRendererInterface>
#include <QUrl>

#include <memory>

// Defined in src/lib.rs.
extern "C" void *telamon_backend_new();
extern "C" void telamon_adopt_legacy();

// Hands a launch's arguments (without the program name) to the backend.
static void activate(QObject *backend, const QStringList &arguments, const QString &cwd)
{
    if (!QMetaObject::invokeMethod(backend, "activate", Q_ARG(QStringList, arguments), Q_ARG(QString, cwd))) {
        qWarning("telamon-explorer: the backend did not take the launch arguments");
    }
}

static void raise(QQmlApplicationEngine *engine)
{
    auto *window = qobject_cast<QQuickWindow *>(engine->rootObjects().value(0));
    if (!window) {
        return;
    }
    if (window->visibility() == QWindow::Minimized) {
        window->showNormal();
    } else {
        window->show();
    }
    // The launcher's activation token: without it Wayland keeps the window
    // down.
    KWindowSystem::updateStartupId(window);
    KWindowSystem::activateWindow(window);
}

int main(int argc, char *argv[])
{
    telamon_app_init();
    // Drawn on the CPU like the other Telamon apps unless QT_QUICK_BACKEND says
    // otherwise (the P phase measures whether thumbnail grids and 100k-row
    // scrolling are better on the GPU).
    if (qEnvironmentVariableIsEmpty("QT_QUICK_BACKEND")) {
        QQuickWindow::setGraphicsApi(QSGRendererInterface::Software);
    }

    QApplication app(argc, argv);
    telamon_app_ready();
    // What Explorer kept under its old name (atlas-explorer) comes over to the
    // new one before anything reads a setting.
    telamon_adopt_legacy();

    QCommandLineParser parser;
    parser.setApplicationDescription(QStringLiteral("The file manager of Telamon OS."));
    parser.addHelpOption();
    parser.addVersionOption();
    parser.addOption({QStringLiteral("new-window"), QStringLiteral("Open the locations in a new window.")});
    parser.addOption({QStringLiteral("select"), QStringLiteral("Open each location's folder with it selected.")});
    parser.addOption({QStringLiteral("split"), QStringLiteral("Open the first two locations side by side.")});
    parser.addPositionalArgument(QStringLiteral("location"), QStringLiteral("Folders, files or URLs (smb://, sftp://, trash:/ ...)."),
                                 QStringLiteral("[location...]"));
    // Only --help and --version are acted on here. Every other argument goes
    // to the backend, which alone decides what is valid and says what it
    // refused in the window, so the two never disagree.
    parser.parse(QCoreApplication::arguments());
    if (parser.isSet(QStringLiteral("help"))) {
        parser.showHelp();
    }
    if (parser.isSet(QStringLiteral("version"))) {
        parser.showVersion();
    }

    // One instance per session. A second launch's arguments come here through
    // activateRequested; without a session bus each launch runs on its own.
    KDBusService service(KDBusService::Unique | KDBusService::NoExitOnFailure);
    // KDBusService also exports the whole application object at
    // /MainApplication, with its slots and properties: any process on the bus
    // could call quit() and closeAllWindows() (ending a copy half way) or set
    // the style sheet. Nothing needs that path: org.freedesktop.Application
    // is served at the application's own path.
    QDBusConnection::sessionBus().unregisterObject(QStringLiteral("/MainApplication"));

    // The backend outlives the engine: the window's bindings read it until
    // the engine is gone.
    std::unique_ptr<QObject> backend(static_cast<QObject *>(telamon_backend_new()));
    auto engine = std::make_unique<QQmlApplicationEngine>();
    QObject::connect(engine.get(), &QQmlApplicationEngine::objectCreationFailed, &app, [] { QCoreApplication::exit(1); }, Qt::QueuedConnection);
    engine->addImageProvider(QStringLiteral("thumb"), new ThumbnailProvider);
    engine->setInitialProperties({{QStringLiteral("backend"), QVariant::fromValue(backend.get())}});
    engine->loadFromModule("net.eterneon.telamon.explorer", "Main");
    if (engine->rootObjects().isEmpty()) {
        return 1;
    }

    // A second launch. With nothing but the program name (the launcher
    // icon, the taskbar) the backend is told too: it opens a new window.
    QObject::connect(&service, &KDBusService::activateRequested, backend.get(),
                     [e = engine.get(), b = backend.get()](const QStringList &arguments, const QString &cwd) {
                         raise(e);
                         // The Rust side reads 64; one more lets it say some were left out.
                         activate(b, arguments.mid(1, 65), cwd);
                     });
    // org.freedesktop.Application.Open from any process in the session: URLs
    // only, and no folder, so relative paths are refused.
    QObject::connect(&service, &KDBusService::openRequested, backend.get(), [e = engine.get(), b = backend.get()](const QList<QUrl> &urls) {
        raise(e);
        if (urls.isEmpty()) {
            return;
        }
        // `--` first: whatever the caller sent is never read as an option.
        QStringList arguments{QStringLiteral("--bus"), QStringLiteral("--")};
        // The Rust side looks at 64 arguments (`--bus`, this `--` and 62 URLs) and
        // counts the rest; one more is enough for it to say some were left
        // out.
        for (const QUrl &url : urls.mid(0, 64)) {
            arguments << url.toString(QUrl::FullyEncoded);
        }
        activate(b, arguments, QString());
    });
    // org.freedesktop.FileManager1: "show this folder / item / its properties"
    // from any app in the session. Everything goes through the backend's launch
    // parsing; `--` first so nothing sent is read as an option.
    auto viaBackend = [e = engine.get(), b = backend.get()](const QStringList &uris, const QString &, const QString &option) {
        raise(e);
        // `--bus`: another program asked, so no server or device is opened for it.
        QStringList arguments{QStringLiteral("--bus")};
        if (!option.isEmpty()) {
            arguments << option;
        }
        arguments << QStringLiteral("--") << uris;
        activate(b, arguments, QString());
    };
    FileManager1::registerOn(
        backend.get(), [viaBackend](const QStringList &u, const QString &id) { viaBackend(u, id, QString()); },
        [viaBackend](const QStringList &u, const QString &id) { viaBackend(u, id, QStringLiteral("--select")); },
        [e = engine.get(), b = backend.get()](const QStringList &uris, const QString &) {
            raise(e);
            QStringList arguments{QStringLiteral("--bus"), QStringLiteral("--")};
            arguments << uris;
            QMetaObject::invokeMethod(b, "inspect", Q_ARG(QStringList, arguments));
        });
    activate(backend.get(), QCoreApplication::arguments().mid(1), QDir::currentPath());

    const int code = app.exec();
    engine.reset();
    return code;
}
