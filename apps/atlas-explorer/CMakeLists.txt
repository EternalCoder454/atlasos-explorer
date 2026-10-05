cmake_minimum_required(VERSION 3.24)
project(atlas-explorer VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

if(CMAKE_INSTALL_PREFIX_INITIALIZED_TO_DEFAULT)
    set(CMAKE_INSTALL_PREFIX /usr CACHE PATH "" FORCE)
endif()
include(GNUInstallDirs)
find_package(Qt6 6.11 REQUIRED COMPONENTS Core Gui Qml Quick QuickControls2 Widgets DBus)
qt_policy(SET QTP0001 NEW)
qt_policy(SET QTP0004 NEW)
# atlas-framework-ui's startup code (single instance, window activation)
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
find_package(Corrosion REQUIRED)

set(WORKSPACE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/../..)

# Rust library (CXX-Qt QObjects, atlas-framework, atlas-explorer-core).
# cxx-qt-build finds Qt via $QMAKE.
find_program(QMAKE6 NAMES qmake6 qmake REQUIRED)
corrosion_import_crate(MANIFEST_PATH ${WORKSPACE_DIR}/Cargo.toml
    CRATES atlas-explorer atlas-explorer-indexd atlas-explorer-search LOCKED)
corrosion_set_env_vars(atlas_explorer QMAKE=${QMAKE6})

# The framework's C header (atlas/app.h) comes with the crate's sources,
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
set(ATLAS_UI_CRATE_DIR "")
foreach(i RANGE ${LAST_PACKAGE})
    string(JSON PKG_NAME GET "${CARGO_METADATA}" packages ${i} name)
    if(PKG_NAME STREQUAL "atlas-framework-ui")
        string(JSON PKG_MANIFEST GET "${CARGO_METADATA}" packages ${i} manifest_path)
        get_filename_component(ATLAS_UI_CRATE_DIR "${PKG_MANIFEST}" DIRECTORY)
    endif()
endforeach()
if(NOT EXISTS "${ATLAS_UI_CRATE_DIR}/include/atlas/app.h")
    message(FATAL_ERROR "atlas-framework-ui's include/atlas/app.h not found (looked in '${ATLAS_UI_CRATE_DIR}')")
endif()

# Atlas.Ui, the shared Atlas look, is installed like Kirigami (atlas-ui, from
# atlas-framework): QML imports it and nothing links it. qmlcachegen and
# qmllint read its types from the installed copy.
if(NOT EXISTS "${QT6_INSTALL_PREFIX}/${QT6_INSTALL_QML}/Atlas/Ui/qmldir")
    message(FATAL_ERROR "Atlas.Ui is not installed: install atlas-ui (atlas-framework)")
endif()

qt_add_executable(atlas-explorer-app cpp/main.cpp cpp/FileManager1.h cpp/FileManager1.cpp)
set_target_properties(atlas-explorer-app PROPERTIES OUTPUT_NAME atlas-explorer)
target_include_directories(atlas-explorer-app PRIVATE "${ATLAS_UI_CRATE_DIR}/include"
    "${CMAKE_CURRENT_SOURCE_DIR}/cpp/kio")

# qmlcachegen compiles every QML file to C++ at build time; the app reads no
# .qml from disk at run time.
qt_add_qml_module(atlas-explorer-app
    URI net.eterneon.atlas.explorer
    VERSION 1.0
    QML_FILES
        qml/Main.qml
        qml/PlaceholderPage.qml
        qml/FolderView.qml
        qml/DetailsView.qml
        qml/IconsView.qml
    SOURCES
        cpp/kio/FolderModel.h cpp/kio/FolderModel.cpp
        cpp/kio/StandardPlaces.h cpp/kio/StandardPlaces.cpp
        cpp/kio/ThumbnailProvider.h cpp/kio/ThumbnailProvider.cpp
        cpp/kio/FileActions.h cpp/kio/FileActions.cpp
        cpp/kio/RustBridge.h
)

target_link_libraries(atlas-explorer-app PRIVATE
    atlas_explorer
    Qt6::Core Qt6::Gui Qt6::Qml Qt6::Quick Qt6::QuickControls2 Qt6::Widgets Qt6::DBus
    KF6::DBusAddons KF6::WindowSystem KF6::GuiAddons
    KF6::KIOCore KF6::KIOGui KF6::KIOWidgets KF6::KIOFileWidgets
    KF6::Solid KF6::Service KF6::CoreAddons KF6::ConfigCore)

install(TARGETS atlas-explorer-app RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR})
# The file index service and its command-line client are cargo binaries that
# Corrosion builds with the app (one cargo invocation, one lock file).
install(IMPORTED_RUNTIME_ARTIFACTS atlas-explorer-indexd atlas-explorer-search
        RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR})
install(FILES data/net.eterneon.atlas.explorer.desktop
        DESTINATION ${CMAKE_INSTALL_DATADIR}/applications)
install(FILES data/dbus/org.freedesktop.FileManager1.service
        DESTINATION ${CMAKE_INSTALL_DATADIR}/dbus-1/services)
install(FILES data/net.eterneon.atlas.explorer.metainfo.xml
        DESTINATION ${CMAKE_INSTALL_DATADIR}/metainfo)
install(FILES data/net.eterneon.atlas.explorer.svg
        DESTINATION ${CMAKE_INSTALL_DATADIR}/icons/hicolor/scalable/apps)
