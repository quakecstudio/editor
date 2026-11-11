# SPDX-License-Identifier: GPL-3.0-or-later
#
# The rpm of QuakeC Studio (P4-18). packaging/linux/package.sh builds it with the version in
# `pkg_version` and the laid-out files in `tree`, both given with --define.
Name: quakec-studio
Version: %{pkg_version}
Release: 1
Summary: QuakeC IDE with its own compiler, live in-game preview and a built-in agent
License: GPL-3.0-or-later
URL: https://github.com/quakecstudio/editor
# The window's libraries, which winit and wgpu open at run time rather than link.
Requires: glibc, alsa-lib, libxkbcommon, libxkbcommon-x11, libX11, libX11-xcb, libXcursor, libXrandr, libXi, vulkan-loader
Recommends: libwayland-client, libwayland-cursor, libwayland-egl, mesa-vulkan-drivers, mesa-libGL, gnome-keyring
AutoReqProv: no

%description
QuakeC Studio is an IDE for QuakeC with its own compiler, a live in-game preview built on its
own QuakeC VM, a built-in agent and a map editor. Quake's game data is not included.

%install
cp -a %{tree}/. %{buildroot}/

%files
%license /usr/share/doc/quakec-studio/LICENSE
%doc /usr/share/doc/quakec-studio/NOTICE
/usr/bin/quakec-studio
/usr/bin/quakec-ide
/usr/bin/quakec-lsp
/usr/lib/quakec-studio
/usr/share/applications/com.quakecstudio.QuakeCStudio.desktop
/usr/share/icons/hicolor/scalable/apps/com.quakecstudio.QuakeCStudio.svg
