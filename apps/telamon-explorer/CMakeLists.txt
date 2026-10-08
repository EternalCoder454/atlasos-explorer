cmake_minimum_required(VERSION 3.24)
project(telamon-explorer VERSION 0.2.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

if(CMAKE_INSTALL_PREFIX_INITIALIZED_TO_DEFAULT)
    set(CMAKE_INSTALL_PREFIX /usr CACHE PATH "" FORCE)
endif()
include(GNUInstallDirs)
find_package(Qt6 6.11 REQUIRED COMPONENTS Core Gui Qml Quick QuickControls2 Widgets DBus Network)
qt_policy(SET QTP0001 NEW)
qt_policy(SET QTP0004 NEW)
# telamon-framework-ui's startup code (single instance, window activation)
find_package(KF6DBusAddons REQUIRED)
find_package(KF6WindowSystem REQUIRED)
# Files, jobs, thumbnails, Open With and service menus (KIO); places and
# drives (KIO's KFilePlacesModel, Solid); applications (KService).
# KF6 has no umbrella package: one find_package per framework.
find_package(KF6KIO 6.30 REQUIRED)
find_package(KF6Solid 6.30 REQUIRED)
find_package(KF6Service 6.30 REQUIRED)
find_package(KF6CoreAddons 6.30 REQUIRED)
find_package(KF6Config 6.30 REQUIRED)
find_package(KF6GuiAddons 6.30 REQUIRED)
# Properties and the Details columns: dimensions, duration, date taken, camera, pages.
find_package(KF6FileMetaData REQUIRED)
find_package(Corrosion REQUIRED)

set(WORKSPACE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/../..)

# Rust library (CXX-Qt QObjects, atlas-framework, atlas-explorer-core).
# cxx-qt-build finds Qt via $QMAKE.
find_program(QMAKE6 NAMES qmake6 qmake REQUIRED)
corrosion_import_crate(MANIFEST_PATH ${WORKSPACE_DIR}/Cargo.toml
    CRATES telamon-explorer telamon-explorer-indexd telamon-explorer-search LOCKED)
corrosion_set_env_vars(telamon_explorer QMAKE=${QMAKE6})

# The framework's C header (telamon/app.h) comes with the crate's sources,
# wherever cargo put them (a git checkout in ~/.cargo).
find_program(CARGO NAMES cargo REQUIRED)
execute_process(
    COMMAND ${CARGO} metadata --format-version 1 --locked --manifest-path ${WORKSPACE_DIR}/Cargo.toml
    OUTPUT_VARIABLE CARGO_METADATA
    RESULT_VARIABLE CARGO_METADATA_RESULT
    ERROR_VARIABLE CARGO_METADATA_ERROR
    TIMEOUT 300)
if(NOT CARGO_METADATA_RESULT EQUAL 0)
    message(FATAL_ERROR "cargo metadata failed (${CARGO_METADATA_RESULT}): ${CARGO_METADATA_ERROR}\n"
        "Run `cargo fetch --locked` first, or check the network.")
endif()
string(JSON NPACKAGES LENGTH "${CARGO_METADATA}" packages)
math(EXPR LAST_PACKAGE "${NPACKAGES} - 1")
set(TELAMON_UI_CRATE_DIR "")
foreach(i RANGE ${LAST_PACKAGE})
    string(JSON PKG_NAME GET "${CARGO_METADATA}" packages ${i} name)
    if(PKG_NAME STREQUAL "telamon-framework-ui")
        string(JSON PKG_MANIFEST GET "${CARGO_METADATA}" packages ${i} manifest_path)
        get_filename_component(TELAMON_UI_CRATE_DIR "${PKG_MANIFEST}" DIRECTORY)
    endif()
endforeach()
if(NOT EXISTS "${TELAMON_UI_CRATE_DIR}/include/telamon/app.h")
    message(FATAL_ERROR "telamon-framework-ui's include/telamon/app.h not found (looked in '${TELAMON_UI_CRATE_DIR}')")
endif()

# Telamon.Ui, the shared Telamon look, is installed like Kirigami (telamon-ui, from
# atlas-framework): QML imports it and nothing links it. qmlcachegen and
# qmllint read its types from the installed copy.
if(NOT EXISTS "${QT6_INSTALL_PREFIX}/${QT6_INSTALL_QML}/Telamon/Ui/qmldir")
    message(FATAL_ERROR "Telamon.Ui is not installed: install telamon-ui (atlas-framework)")
endif()

qt_add_executable(telamon-explorer-app cpp/main.cpp cpp/FileManager1.h cpp/FileManager1.cpp)
set_target_properties(telamon-explorer-app PROPERTIES OUTPUT_NAME telamon-explorer)
target_include_directories(telamon-explorer-app PRIVATE "${TELAMON_UI_CRATE_DIR}/include"
    "${CMAKE_CURRENT_SOURCE_DIR}/cpp/kio")

# qmlcachegen compiles every QML file to C++ at build time; the app reads no
# .qml from disk at run time.
qt_add_qml_module(telamon-explorer-app
    URI net.eterneon.telamon.explorer
    VERSION 1.0
    QML_FILES
        qml/Main.qml
        qml/PlaceholderPage.qml
        qml/FolderView.qml
        qml/FilesTabPage.qml
        qml/PathBar.qml
        qml/StatusLine.qml
        qml/DetailsView.qml
        qml/IconsView.qml
        qml/PlaceItem.qml
        qml/SearchBar.qml
        qml/QuickLook.qml
        qml/PreviewBody.qml
        qml/PreviewPane.qml
        qml/MediaControls.qml
        qml/OperationsButton.qml
        qml/ConflictDialog.qml
        qml/ProblemDialog.qml
        qml/ActionMenu.qml
        qml/IconRowItem.qml
        qml/FileMenu.qml
        qml/BackgroundMenu.qml
        qml/ViewMenu.qml
        qml/SortMenu.qml
        qml/NamePrompt.qml
        qml/InlineRename.qml
        qml/BatchRenameDialog.qml
        qml/IconCell.qml
        qml/ColumnsView.qml
        qml/ColumnList.qml
        qml/GalleryView.qml
        qml/GroupHeader.qml
        qml/HomePage.qml
        qml/HomeTile.qml
        qml/NetworkPage.qml
        qml/ConnectDialog.qml
        qml/TrashBar.qml
        qml/TrashAutoEmpty.qml
        qml/TagDots.qml
        qml/TagRowItem.qml
        qml/TagItem.qml
        qml/PropertiesDialog.qml
    SOURCES
        cpp/kio/FolderModel.h cpp/kio/FolderModel.cpp
        cpp/kio/StandardPlaces.h cpp/kio/StandardPlaces.cpp
        cpp/kio/ThumbnailProvider.h cpp/kio/ThumbnailProvider.cpp
        cpp/kio/FileActions.h cpp/kio/FileActions.cpp
        cpp/kio/OperationQueue.h cpp/kio/OperationQueue.cpp
        cpp/kio/OperationAsker.h cpp/kio/OperationAsker.cpp
        cpp/kio/ArchiveClient.h cpp/kio/ArchiveClient.cpp
        cpp/kio/ArchiveGuard.h cpp/kio/ArchiveGuard.cpp
        cpp/kio/OpsBridge.h
        cpp/kio/TabLogic.h cpp/kio/TabLogic.cpp
        cpp/kio/LocationLogic.h cpp/kio/LocationLogic.cpp
        cpp/kio/PlacesLogic.h cpp/kio/PlacesLogic.cpp
        cpp/kio/PlacesModel.h cpp/kio/PlacesModel.cpp
        cpp/kio/SearchController.h cpp/kio/SearchController.cpp
        cpp/kio/PreviewLogic.h cpp/kio/PreviewLogic.cpp
        cpp/kio/PreviewLoader.h cpp/kio/PreviewLoader.cpp
        cpp/kio/ViewMemory.h cpp/kio/ViewMemory.cpp
        cpp/kio/GroupLines.h cpp/kio/GroupLines.cpp
        cpp/kio/HomeLogic.h cpp/kio/HomeLogic.cpp
        cpp/kio/ServerLogic.h cpp/kio/ServerLogic.cpp
        cpp/kio/NetworkModel.h cpp/kio/NetworkModel.cpp
        cpp/kio/TrashLogic.h cpp/kio/TrashLogic.cpp
        cpp/kio/TagLogic.h cpp/kio/TagLogic.cpp
        cpp/kio/ColumnLogic.h cpp/kio/ColumnLogic.cpp
        cpp/kio/MetaReader.h cpp/kio/MetaReader.cpp
        cpp/kio/PropertiesLogic.h cpp/kio/PropertiesLogic.cpp
        cpp/kio/PropsBridge.h
        cpp/kio/RustBridge.h
)

target_link_libraries(telamon-explorer-app PRIVATE
    telamon_explorer
    Qt6::Core Qt6::Gui Qt6::Qml Qt6::Quick Qt6::QuickControls2 Qt6::Widgets Qt6::DBus Qt6::Network
    KF6::DBusAddons KF6::WindowSystem KF6::GuiAddons
    KF6::KIOCore KF6::KIOGui KF6::KIOWidgets KF6::KIOFileWidgets
    KF6::Solid KF6::Service KF6::CoreAddons KF6::ConfigCore KF6::FileMetaData)

install(TARGETS telamon-explorer-app RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR})
# The file index service and its command-line client are cargo binaries that
# Corrosion builds with the app (one cargo invocation, one lock file).
install(IMPORTED_RUNTIME_ARTIFACTS telamon-explorer-indexd telamon-explorer-search
        RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR})
install(FILES data/net.eterneon.telamon.explorer.desktop
        DESTINATION ${CMAKE_INSTALL_DATADIR}/applications)
# The names Files had before Telamon Explorer, for this release: its desktop
# file ID (default applications, the Launcher's pinned apps) and its commands.
install(FILES data/net.eterneon.atlas.explorer.desktop
        DESTINATION ${CMAKE_INSTALL_DATADIR}/applications)
foreach(program telamon-explorer telamon-explorer-indexd telamon-explorer-search)
    string(REPLACE "telamon-" "atlas-" legacy_program ${program})
    install(CODE "file(CREATE_LINK ${program} \$ENV{DESTDIR}${CMAKE_INSTALL_FULL_BINDIR}/${legacy_program} SYMBOLIC)")
endforeach()
install(FILES data/dbus/org.freedesktop.FileManager1.service
        DESTINATION ${CMAKE_INSTALL_DATADIR}/dbus-1/services)
install(FILES data/net.eterneon.telamon.explorer.metainfo.xml
        DESTINATION ${CMAKE_INSTALL_DATADIR}/metainfo)
install(FILES data/net.eterneon.telamon.explorer.svg
        DESTINATION ${CMAKE_INSTALL_DATADIR}/icons/hicolor/scalable/apps)
