# Telamon Explorer (Files) for Telamon OS.

# No debuginfo subpackage: the Rust flags below keep symbols (debuginfo=2,
# strip=none) and the binary is shipped as built.
%global debug_package %{nil}

Name:           telamon-explorer
Version:        0.2.0
Release:        1%{?dist}
Summary:        Files, the file manager of Telamon OS
License:        MIT
URL:            https://github.com/EternalCoder454/atlasos-explorer
# Renamed from atlas-explorer: the image upgrades it in place, and what still
# says "atlas-explorer" (the commands, the desktop file ID, the D-Bus names of
# the file index) keeps working in this release.
Obsoletes:      atlas-explorer < 0.2.0
Provides:       atlas-explorer = %{version}-%{release}
Source0:        telamon-explorer-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  rust
# %%build_rustflags
BuildRequires:  rust-srpm-macros
BuildRequires:  gcc
BuildRequires:  gcc-c++
BuildRequires:  cmake
BuildRequires:  ninja-build
BuildRequires:  corrosion
# Cargo fetches the atlas-framework crates from GitHub.
BuildRequires:  git-core
BuildRequires:  desktop-file-utils
# %%{_userunitdir}
BuildRequires:  systemd-rpm-macros
BuildRequires:  libappstream-glib
BuildRequires:  cmake(Qt6Core)
BuildRequires:  cmake(Qt6Gui)
BuildRequires:  cmake(Qt6Qml)
BuildRequires:  cmake(Qt6Quick)
BuildRequires:  cmake(Qt6QuickControls2)
BuildRequires:  cmake(Qt6Widgets)
BuildRequires:  cmake(Qt6QmlTools)
BuildRequires:  qt6-qtbase-devel
BuildRequires:  cmake(KF6DBusAddons)
BuildRequires:  cmake(KF6WindowSystem)
BuildRequires:  cmake(KF6KIO) >= 6.30
BuildRequires:  cmake(KF6Solid)
BuildRequires:  cmake(KF6Service)
BuildRequires:  cmake(KF6CoreAddons)
BuildRequires:  cmake(KF6Config)
# QML modules qmlcachegen resolves at build time (not linked). telamon-ui comes
# from atlas-framework, which is in no repository: install its RPMs first
# (build-rpm.sh does, given ATLAS_LOCAL_RPMS).
BuildRequires:  kf6-kirigami-devel
BuildRequires:  telamon-ui >= 2.0.0

Requires:       kf6-kirigami
# Telamon.Ui, the shared look (atlas-framework); 1.4.0 for TelamonSidebar and
# TelamonBreadcrumb
Requires:       telamon-ui >= 2.0.0
Requires:       kf6-qqc2-desktop-style
Requires:       qt6-qtdeclarative
# the app icon and Breeze's icons are SVG
Requires:       qt6-qtsvg
# KIO's workers beyond file:/ and trash:/ (smb, sftp, mtp, network,
# recentlyused, thumbnail) and its thumbnailers
Requires:       kf6-kio-core >= 6.30
Recommends:     kio-extras
Recommends:     kdegraphics-thumbnailers
Recommends:     ffmpegthumbs

%description
Files is the file manager of Telamon OS. It has tabs, a sidebar of your places
and drives, icons, list, details, columns and gallery views, a preview pane,
split view and Quick Look. It works with local drives, phones, network shares,
SFTP servers and the Trash through KIO, runs copies and moves through one queue
that can pause, cancel and undo, and finds files across your home folder
through its own light index.

%prep
%autosetup -n telamon-explorer-%{version}

%build
# NETWORK: cargo (Corrosion runs it with --locked) fetches crates.io and the
# pinned atlas-framework crates during %%build. That works in podman and with
# `rpmbuild` on a networked machine, not in an offline mock/Koji build.
# CARGO_HOME from the environment keeps a crate cache between builds
# (CLAUDE.md mounts one); otherwise a fresh one in the build dir.
export CARGO_HOME=${CARGO_HOME:-%{_builddir}/cargo-home}
# Fedora's Rust flags (hardening, build-id, ...), also used by Corrosion's
# cargo. The remaps keep build paths (panic locations, assert file names) out
# of the package, as atlas-framework's DESIGN.md asks of apps using its crates.
# HOST_CXXFLAGS reaches only the C++ that cargo's build scripts compile (cc-rs
# reads HOST_ when not cross-compiling; CMake ignores it). CFLAGS and CXXFLAGS
# are Fedora's plus the same remap for the C++ CMake builds, so that two
# builds of one commit give the same build ID. These flags split on spaces,
# so _topdir must have none (build-rpm.sh's hasn't).
export RUSTFLAGS="%{build_rustflags} --remap-path-prefix=$PWD=. --remap-path-prefix=$CARGO_HOME=cargo"
export HOST_CXXFLAGS="-ffile-prefix-map=$PWD=. -ffile-prefix-map=$CARGO_HOME=cargo"
export CFLAGS="%{build_cflags} -ffile-prefix-map=$PWD=."
export CXXFLAGS="%{build_cxxflags} -ffile-prefix-map=$PWD=."
export CARGO_PROFILE_RELEASE_STRIP=none
# (%%cmake honours _vpath_srcdir, not __cmake_source_dir)
%global _vpath_srcdir apps/telamon-explorer
%cmake -G Ninja -DCMAKE_BUILD_TYPE=Release
%cmake_build

%install
%cmake_install
# Telamon OS needs its file manager: dnf refuses to remove it.
install -Dpm0644 apps/telamon-explorer/data/dnf/protected.d/telamon-explorer.conf \
    %{buildroot}%{_sysconfdir}/dnf/protected.d/telamon-explorer.conf
# The file index service: D-Bus activation file and interface XML, the user
# unit.
install -Dpm0644 data/dbus/net.eterneon.telamon.explorer.Search.service \
    %{buildroot}%{_datadir}/dbus-1/services/net.eterneon.telamon.explorer.Search.service
install -Dpm0644 data/dbus/net.eterneon.telamon.explorer.Search1.xml \
    %{buildroot}%{_datadir}/dbus-1/interfaces/net.eterneon.telamon.explorer.Search1.xml
install -Dpm0644 data/systemd/telamon-explorer-indexd.service \
    %{buildroot}%{_userunitdir}/telamon-explorer-indexd.service
# The index service under its old names too, for this release: the old bus
# name and interface (the Launcher), and the old unit name, a link to the new.
install -Dpm0644 data/dbus/net.eterneon.atlas.explorer.Search.service \
    %{buildroot}%{_datadir}/dbus-1/services/net.eterneon.atlas.explorer.Search.service
install -Dpm0644 data/dbus/net.eterneon.atlas.explorer.Search1.xml \
    %{buildroot}%{_datadir}/dbus-1/interfaces/net.eterneon.atlas.explorer.Search1.xml
ln -s telamon-explorer-indexd.service %{buildroot}%{_userunitdir}/atlas-explorer-indexd.service

%check
# No path into the build tree (checked as well as set: see %%build).
# grep: 0 = found, 1 = not found, anything else (no binary) fails too.
rc=0
grep -qF "%{_builddir}" %{buildroot}%{_bindir}/telamon-explorer || rc=$?
if [ "$rc" != 1 ]; then
    echo "telamon-explorer holds the build path %{_builddir} (grep status $rc)" >&2
    exit 1
fi
desktop-file-validate %{buildroot}%{_datadir}/applications/net.eterneon.telamon.explorer.desktop
desktop-file-validate %{buildroot}%{_datadir}/applications/net.eterneon.atlas.explorer.desktop
# The old commands are links to the new ones.
for p in explorer explorer-indexd explorer-search; do
    test "$(readlink %{buildroot}%{_bindir}/atlas-$p)" = "telamon-$p"
done
# Both bus names start the same service.
grep -q '^SystemdService=telamon-explorer-indexd.service$' \
    %{buildroot}%{_datadir}/dbus-1/services/net.eterneon.atlas.explorer.Search.service
appstream-util validate-relax --nonet \
    %{buildroot}%{_datadir}/metainfo/net.eterneon.telamon.explorer.metainfo.xml

%files
%license LICENSE
%{_bindir}/telamon-explorer
%{_bindir}/telamon-explorer-indexd
%{_bindir}/telamon-explorer-search
%{_bindir}/atlas-explorer
%{_bindir}/atlas-explorer-indexd
%{_bindir}/atlas-explorer-search
%{_datadir}/dbus-1/services/net.eterneon.atlas.explorer.Search.service
%{_datadir}/dbus-1/interfaces/net.eterneon.atlas.explorer.Search1.xml
%{_userunitdir}/atlas-explorer-indexd.service
%{_datadir}/applications/net.eterneon.atlas.explorer.desktop
%{_datadir}/dbus-1/services/net.eterneon.telamon.explorer.Search.service
%{_datadir}/dbus-1/services/org.freedesktop.FileManager1.service
%{_datadir}/dbus-1/interfaces/net.eterneon.telamon.explorer.Search1.xml
%{_userunitdir}/telamon-explorer-indexd.service
%{_datadir}/applications/net.eterneon.telamon.explorer.desktop
%{_datadir}/metainfo/net.eterneon.telamon.explorer.metainfo.xml
%{_datadir}/icons/hicolor/scalable/apps/net.eterneon.telamon.explorer.svg
%config(noreplace) %{_sysconfdir}/dnf/protected.d/telamon-explorer.conf

%changelog
* Wed Oct 07 2026 EternalHell <77252745+EternalCoder454@users.noreply.github.com> - 0.2.0-1
- Renamed to Telamon Explorer: telamon-explorer, telamon-explorer-indexd,
  telamon-explorer-search, net.eterneon.telamon.explorer, built on telamon-ui
  2.0.0. atlas-explorer is obsoleted and provided; the old commands, desktop
  file ID, unit name and the file index's old bus name
  (net.eterneon.atlas.explorer.Search1) are kept for this release
- Settings (atlas-explorerrc), the index settings (atlas-explorer/indexrc) and
  the index snapshot move to the new names once, on first start

* Mon Oct 05 2026 EternalHell <77252745+EternalCoder454@users.noreply.github.com> - 0.1.0-1
- First package
