#!/usr/bin/env sh
set -eu
umask 022

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
TARGET=${TARGET:-x86_64-unknown-linux-gnu}
CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-"$ROOT_DIR/target"}
export CARGO_TARGET_DIR
PACKAGE_FORMATS=${PACKAGE_FORMATS:-auto}
if [ "$PACKAGE_FORMATS" = auto ]; then
  if command -v dpkg-deb >/dev/null 2>&1; then PACKAGE_FORMATS=deb
  elif command -v rpmbuild >/dev/null 2>&1; then PACKAGE_FORMATS=rpm
  else PACKAGE_FORMATS=binary
  fi
fi
case "$PACKAGE_FORMATS" in
  deb) BUILD_DEB=1; BUILD_RPM=0 ;;
  rpm) BUILD_DEB=0; BUILD_RPM=1 ;;
  binary) BUILD_DEB=0; BUILD_RPM=0 ;;
  *) printf 'PACKAGE_FORMATS must be auto, deb, rpm, or binary\n' >&2; exit 1 ;;
esac
if [ "$BUILD_DEB" -eq 1 ]; then
  command -v dpkg-deb >/dev/null 2>&1 && command -v dpkg-shlibdeps >/dev/null 2>&1 || {
    printf 'Debian packaging requires dpkg-deb and dpkg-shlibdeps (dpkg-dev)\n' >&2; exit 1
  }
fi
if [ "$BUILD_RPM" -eq 1 ]; then
  command -v rpmbuild >/dev/null 2>&1 || { printf 'Fedora packaging requires rpm-build\n' >&2; exit 1; }
  HOST_DISTRIBUTION=$(sed -n 's/^ID=//p' /etc/os-release 2>/dev/null | tr -d '"')
  [ "$HOST_DISTRIBUTION" = fedora ] || {
    printf 'Build the Fedora RPM inside Fedora so its ABI and dependency generators match the target system\n' >&2; exit 1
  }
fi
case "$TARGET" in
  x86_64-unknown-linux-gnu | aarch64-unknown-linux-gnu | i686-unknown-linux-gnu | i586-unknown-linux-gnu) ;;
  *) printf 'Unsupported GNU/Linux packaging target: %s\n' "$TARGET" >&2; exit 1 ;;
esac

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -n 1)
[ -n "$VERSION" ] || { printf 'Could not read the BExplorer version from Cargo.toml\n' >&2; exit 1; }
APPDIR="$ROOT_DIR/dist/bexplorer-linux-$TARGET"
BINARY="$ROOT_DIR/dist/bexplorer-$VERSION-linux-$TARGET"
DEBROOT="$ROOT_DIR/dist/bexplorer-deb"
RPMBUILD="$ROOT_DIR/dist/rpmbuild"

sh "$ROOT_DIR/scripts/linux/sync-icons.sh" --check

case "$TARGET" in
  x86_64-*) DEB_ARCH=amd64; RPM_ARCH=x86_64 ;;
  aarch64-*) DEB_ARCH=arm64; RPM_ARCH=aarch64 ;;
  i686-* | i586-*) DEB_ARCH=i386; RPM_ARCH=i686 ;;
  *) DEB_ARCH=; RPM_ARCH= ;;
esac

if [ "$BUILD_RPM" -eq 1 ] && [ "$(rpm --eval '%{_arch}')" != "$RPM_ARCH" ]; then
  printf 'Build the Fedora RPM natively for %s (cross-packaging is not supported)\n' "$RPM_ARCH" >&2
  exit 1
fi

DEB="$ROOT_DIR/dist/bexplorer_${VERSION}_${DEB_ARCH:-unsupported}.deb"

cargo build --locked --manifest-path "$ROOT_DIR/Cargo.toml" --release --target "$TARGET"

rm -rf "$APPDIR" "$DEBROOT" "$RPMBUILD"
mkdir -p \
  "$APPDIR/usr/bin" \
  "$APPDIR/usr/share/applications" \
  "$APPDIR/usr/share/icons/hicolor" \
  "$APPDIR/usr/share/pixmaps" \
  "$APPDIR/usr/share/metainfo" \
  "$APPDIR/usr/share/polkit-1/actions" \
  "$APPDIR/usr/share/doc/bexplorer"

install -m 0755 "$CARGO_TARGET_DIR/$TARGET/release/bexplorer" "$APPDIR/usr/bin/bexplorer"
install -m 0644 "$ROOT_DIR/assets/linux/bexplorer.desktop" "$APPDIR/usr/share/applications/bexplorer.desktop"
install -m 0644 "$ROOT_DIR/assets/icons/appicon.png" "$APPDIR/usr/share/pixmaps/bexplorer.png"
for ICON in "$ROOT_DIR"/assets/linux/hicolor/*/apps/bexplorer.png; do
  [ -f "$ICON" ] || continue
  ICON_SIZE=$(basename "$(dirname "$(dirname "$ICON")")")
  mkdir -p "$APPDIR/usr/share/icons/hicolor/$ICON_SIZE/apps"
  install -m 0644 "$ICON" "$APPDIR/usr/share/icons/hicolor/$ICON_SIZE/apps/bexplorer.png"
done
install -m 0644 "$ROOT_DIR/assets/linux/io.github.BryamContreras.BExplorer.metainfo.xml" "$APPDIR/usr/share/metainfo/io.github.BryamContreras.BExplorer.metainfo.xml"
install -m 0644 "$ROOT_DIR/assets/linux/io.github.BryamContreras.BExplorer.policy" "$APPDIR/usr/share/polkit-1/actions/io.github.BryamContreras.BExplorer.policy"
install -m 0644 "$ROOT_DIR/README.md" "$APPDIR/usr/share/doc/bexplorer/README.md"
install -m 0644 "$ROOT_DIR/LICENSE" "$APPDIR/usr/share/doc/bexplorer/LICENSE"
install -m 0644 "$ROOT_DIR/THIRD_PARTY_NOTICES.md" "$APPDIR/usr/share/doc/bexplorer/THIRD_PARTY_NOTICES.md"
install -m 0644 "$ROOT_DIR/vendor/7zip-src/DOC/License.txt" "$APPDIR/usr/share/doc/bexplorer/License-7Zip.txt"
install -m 0644 "$ROOT_DIR/vendor/7zip-src/DOC/copying.txt" "$APPDIR/usr/share/doc/bexplorer/copying-7Zip.txt"
install -m 0644 "$ROOT_DIR/vendor/7zip-src/DOC/unRarLicense.txt" "$APPDIR/usr/share/doc/bexplorer/unRarLicense.txt"

mkdir -p "$ROOT_DIR/dist"
install -m 0755 "$APPDIR/usr/bin/bexplorer" "$BINARY"
(cd "$ROOT_DIR/dist" && sha256sum "$(basename "$BINARY")") > "$BINARY.sha256.txt"
printf 'Created %s and checksum\n' "$BINARY"

if [ "$BUILD_RPM" -eq 1 ]; then
  mkdir -p \
    "$RPMBUILD/BUILD" \
    "$RPMBUILD/BUILDROOT" \
    "$RPMBUILD/RPMS" \
    "$RPMBUILD/SOURCES" \
    "$RPMBUILD/SPECS" \
    "$RPMBUILD/SRPMS"
  cp -a "$APPDIR/usr" "$RPMBUILD/SOURCES/usr"

  cat > "$RPMBUILD/SPECS/bexplorer.spec" <<EOF
%global debug_package %{nil}

Name:           bexplorer
Version:        $VERSION
Release:        1%{?dist}
Summary:        Native Rust file explorer
License:        MIT
Vendor:         BExplorer Project
BuildArch:      $RPM_ARCH
AutoReqProv:    yes

# ELF Requires remain automatic; these also cover libraries loaded at runtime.
Requires:       glib2%{?_isa}
Requires:       fontconfig%{?_isa}
Requires:       libX11%{?_isa}
Requires:       libX11-xcb%{?_isa}
Requires:       libXext%{?_isa}
Requires:       libXcursor%{?_isa}
Requires:       libXi%{?_isa}
Requires:       libxcb%{?_isa}
Requires:       libxkbcommon%{?_isa}
Requires:       libxkbcommon-x11%{?_isa}
Requires:       libwayland-client%{?_isa}
Requires:       libwayland-cursor%{?_isa}
Requires:       libglvnd-egl%{?_isa}
Requires:       libglvnd-glx%{?_isa}
Requires:       hicolor-icon-theme
Requires:       polkit
Requires:       shared-mime-info
Requires:       udisks2
Requires:       xdg-desktop-portal
Requires:       (xdg-desktop-portal-gtk or xdg-desktop-portal-gnome or xdg-desktop-portal-kde)
Requires:       xdg-utils
Recommends:     desktop-file-utils
Recommends:     gvfs-fuse
Recommends:     gvfs-mtp
Recommends:     gvfs-smb
Recommends:     samba-client
Recommends:     avahi-tools
Recommends:     wl-clipboard
Recommends:     e2fsprogs
Recommends:     dosfstools
Recommends:     exfatprogs
Recommends:     ntfsprogs
Recommends:     btrfs-progs
Recommends:     xfsprogs
Recommends:     mesa-vulkan-drivers
Suggests:       kio-extras
Suggests:       kio-fuse
Suggests:       perl-File-MimeInfo

%description
BExplorer is a native Rust desktop file explorer with tabs, split-pane
workflows, archive handling, previews, and Linux desktop integration.

%prep

%build

%install
mkdir -p %{buildroot}
cp -a "%{_sourcedir}/usr" %{buildroot}/

%post
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || :
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || :
fi

%postun
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || :
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || :
fi

%files
%{_bindir}/bexplorer
%{_datadir}/applications/bexplorer.desktop
%{_datadir}/icons/hicolor/*/apps/bexplorer.png
%{_datadir}/metainfo/io.github.BryamContreras.BExplorer.metainfo.xml
%{_datadir}/pixmaps/bexplorer.png
%{_datadir}/polkit-1/actions/io.github.BryamContreras.BExplorer.policy
%doc %{_datadir}/doc/bexplorer/README.md
%doc %{_datadir}/doc/bexplorer/THIRD_PARTY_NOTICES.md
%doc %{_datadir}/doc/bexplorer/License-7Zip.txt
%doc %{_datadir}/doc/bexplorer/copying-7Zip.txt
%doc %{_datadir}/doc/bexplorer/unRarLicense.txt
%license %{_datadir}/doc/bexplorer/LICENSE

%changelog
* $(LC_ALL=C date '+%a %b %d %Y') BExplorer Project <noreply@github.com> - $VERSION-1
- Automated BExplorer package.
EOF

  rpmbuild --define "_topdir $RPMBUILD" \
    -bb "$RPMBUILD/SPECS/bexplorer.spec"
  RPM_CREATED=$(find "$RPMBUILD/RPMS" -type f \
    -name "bexplorer-$VERSION-1*.$RPM_ARCH.rpm" -print | head -n 1)
  [ -n "$RPM_CREATED" ] || {
    printf 'rpmbuild completed but the package was not found\n' >&2
    exit 1
  }
  RPM="$ROOT_DIR/dist/$(basename "$RPM_CREATED")"
  cp "$RPM_CREATED" "$RPM"
  printf 'Created %s\n' "$RPM"
  EXPECTED_VERSION=$VERSION EXPECTED_ARCH=$RPM_ARCH \
    sh "$ROOT_DIR/scripts/linux/validate-rpm.sh" "$RPM"
  (cd "$ROOT_DIR/dist" && sha256sum "$(basename "$RPM")") > "$RPM.sha256.txt"
  printf 'Created %s\n' "$RPM.sha256.txt"
fi

[ "$BUILD_DEB" -eq 1 ] || exit 0

mkdir -p "$DEBROOT/DEBIAN"
cp -a "$APPDIR/usr" "$DEBROOT/usr"

# Resolve every ELF dependency and its versioned symbols, including C++ ABI
# requirements from embedded 7-Zip. A GLIBC-only string scan misses GLIBCXX /
# CXXABI requirements and can produce an installable but unusable package.
command -v dpkg-shlibdeps >/dev/null 2>&1 || {
  printf 'dpkg-dev (dpkg-shlibdeps) is required to determine Debian ABI dependencies\n' >&2
  exit 1
}
ABIROOT=$(mktemp -d "$ROOT_DIR/dist/.bexplorer-abi.XXXXXX")
trap 'rm -rf "$ABIROOT"' EXIT HUP INT TERM
mkdir -p "$ABIROOT/debian"
cat > "$ABIROOT/debian/control" <<EOF
Source: bexplorer
Section: utils
Priority: optional
Maintainer: BExplorer Project <noreply@github.com>

Package: bexplorer
Architecture: $DEB_ARCH
Description: Native Rust file explorer
EOF
ELF_DEPENDENCIES=$(cd "$ABIROOT" && dpkg-shlibdeps -O -e "$APPDIR/usr/bin/bexplorer" \
  | sed -n 's/^shlibs:Depends=//p')
[ -n "$ELF_DEPENDENCIES" ] || {
  printf 'Could not determine ELF runtime dependencies\n' >&2
  exit 1
}
rm -rf "$ABIROOT"
trap - EXIT HUP INT TERM
printf 'Detected ELF runtime dependencies: %s\n' "$ELF_DEPENDENCIES"

cat > "$DEBROOT/DEBIAN/control" <<EOF
Package: bexplorer
Version: $VERSION
Section: utils
Priority: optional
Architecture: $DEB_ARCH
Maintainer: BExplorer Project <noreply@github.com>
Depends: $ELF_DEPENDENCIES,
 libx11-6,
 libxext6,
 libxcb1,
 libxkbcommon0,
 libwayland-client0,
 libegl1,
 libgl1,
 libglib2.0-bin,
 xdg-utils,
 xdg-desktop-portal,
 xdg-desktop-portal-gtk | xdg-desktop-portal-backend,
 udisks2,
 pkexec | policykit-1,
 shared-mime-info,
 hicolor-icon-theme
Recommends: libx11-xcb1,
 libxcursor1,
 libxi6,
 libxkbcommon-x11-0,
 e2fsprogs,
 dosfstools,
 exfatprogs,
 ntfs-3g,
 btrfs-progs,
 xfsprogs,
 gvfs-fuse,
 gvfs-backends,
 smbclient,
 avahi-utils,
 desktop-file-utils,
 gtk-update-icon-cache,
 wl-clipboard | xclip | xsel
Suggests: libfile-mimeinfo-perl,
 kde-cli-tools,
 kio-extras,
 kio-fuse,
 tumbler,
 ffmpegthumbnailer | ffmpeg
Description: Native Rust file explorer
 BExplorer is a native Rust desktop file explorer with tabs, split-pane
 workflows, archive handling, previews, and Linux desktop integration.
EOF

cat > "$DEBROOT/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
fi
EOF

cat > "$DEBROOT/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
fi
EOF

chmod 0755 "$DEBROOT/DEBIAN/postinst" "$DEBROOT/DEBIAN/postrm"

if dpkg-deb --help 2>/dev/null | grep -q -- '--root-owner-group'; then
  dpkg-deb --root-owner-group --build "$DEBROOT" "$DEB"
else
  dpkg-deb --build "$DEBROOT" "$DEB"
fi
printf 'Created %s\n' "$DEB"
EXPECTED_VERSION=$VERSION EXPECTED_ARCH=$DEB_ARCH \
  "$ROOT_DIR/scripts/linux/validate-deb.sh" "$DEB"
(cd "$ROOT_DIR/dist" && sha256sum "$(basename "$DEB")") > "$DEB.sha256.txt"
printf 'Created %s\n' "$DEB.sha256.txt"
