#!/usr/bin/env sh
set -eu
umask 022

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
TARGET=${TARGET:-x86_64-unknown-linux-gnu}
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -n 1)
[ -n "$VERSION" ] || { printf 'Could not read the BExplorer version from Cargo.toml\n' >&2; exit 1; }
APPDIR="$ROOT_DIR/dist/bexplorer-linux-$TARGET"
TARBALL="$ROOT_DIR/dist/bexplorer-$VERSION-linux-$TARGET.tar.gz"
DEBROOT="$ROOT_DIR/dist/bexplorer-deb"
RPMBUILD="$ROOT_DIR/dist/rpmbuild"

case "$TARGET" in
  x86_64-*) DEB_ARCH=amd64; RPM_ARCH=x86_64 ;;
  aarch64-*) DEB_ARCH=arm64; RPM_ARCH=aarch64 ;;
  i686-* | i586-*) DEB_ARCH=i386; RPM_ARCH=i686 ;;
  *) DEB_ARCH=; RPM_ARCH= ;;
esac

DEB="$ROOT_DIR/dist/bexplorer_${VERSION}_${DEB_ARCH:-unsupported}.deb"
RPM="$ROOT_DIR/dist/bexplorer-${VERSION}-1.${RPM_ARCH:-unsupported}.rpm"

cargo build --manifest-path "$ROOT_DIR/Cargo.toml" --release --target "$TARGET"

rm -rf "$APPDIR" "$DEBROOT" "$RPMBUILD"
mkdir -p \
  "$APPDIR/usr/bin" \
  "$APPDIR/usr/share/applications" \
  "$APPDIR/usr/share/icons/hicolor" \
  "$APPDIR/usr/share/pixmaps" \
  "$APPDIR/usr/share/metainfo" \
  "$APPDIR/usr/share/polkit-1/actions" \
  "$APPDIR/usr/share/doc/bexplorer"

install -m 0755 "$ROOT_DIR/target/$TARGET/release/bexplorer" "$APPDIR/usr/bin/bexplorer"
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
tar -C "$APPDIR" -czf "$TARBALL" .
printf 'Created %s\n' "$TARBALL"
sha256sum "$TARBALL" > "$TARBALL.sha256.txt"
printf 'Created %s\n' "$TARBALL.sha256.txt"

if ! command -v rpmbuild >/dev/null 2>&1; then
  printf 'Skipping .rpm: rpmbuild is not installed\n'
elif [ -z "$RPM_ARCH" ]; then
  printf 'Skipping .rpm: unsupported target architecture %s\n' "$TARGET"
else
  mkdir -p \
    "$RPMBUILD/BUILD" \
    "$RPMBUILD/BUILDROOT" \
    "$RPMBUILD/RPMS" \
    "$RPMBUILD/SOURCES/bexplorer-$VERSION" \
    "$RPMBUILD/SPECS" \
    "$RPMBUILD/SRPMS"
  cp -a "$APPDIR/usr" "$RPMBUILD/SOURCES/bexplorer-$VERSION/usr"
  tar -C "$RPMBUILD/SOURCES" \
    -czf "$RPMBUILD/SOURCES/bexplorer-$VERSION.tar.gz" \
    "bexplorer-$VERSION"
  rm -rf "$RPMBUILD/SOURCES/bexplorer-$VERSION"

  cat > "$RPMBUILD/SPECS/bexplorer.spec" <<EOF
Name:           bexplorer
Version:        $VERSION
Release:        1
Summary:        Native Rust file explorer
License:        MIT
Vendor:         BExplorer Project
Source0:        %{name}-%{version}.tar.gz
BuildArch:      $RPM_ARCH

Requires:       hicolor-icon-theme
Requires:       polkit
Requires:       shared-mime-info
Requires:       udisks2
Requires:       xdg-desktop-portal
Requires:       xdg-utils
Recommends:     desktop-file-utils
Recommends:     gvfs-fuse

%description
BExplorer is a native Rust desktop file explorer with tabs, split-pane
workflows, archive handling, previews, and Linux desktop integration.

%prep
%setup -q

%build

%install
mkdir -p %{buildroot}
cp -a usr %{buildroot}/

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
* Fri Jul 24 2026 BExplorer Project <noreply@github.com> - $VERSION-1
- Automated BExplorer package.
EOF

  rpmbuild --define "_topdir $RPMBUILD" \
    -bb "$RPMBUILD/SPECS/bexplorer.spec"
  RPM_CREATED=$(find "$RPMBUILD/RPMS" -type f \
    -name "bexplorer-$VERSION-1.$RPM_ARCH.rpm" -print | head -n 1)
  [ -n "$RPM_CREATED" ] || {
    printf 'rpmbuild completed but the package was not found\n' >&2
    exit 1
  }
  cp "$RPM_CREATED" "$RPM"
  printf 'Created %s\n' "$RPM"
  EXPECTED_VERSION=$VERSION EXPECTED_ARCH=$RPM_ARCH \
    sh "$ROOT_DIR/scripts/linux/validate-rpm.sh" "$RPM"
  sha256sum "$RPM" > "$RPM.sha256.txt"
  printf 'Created %s\n' "$RPM.sha256.txt"
fi

if ! command -v dpkg-deb >/dev/null 2>&1; then
  printf 'Skipping .deb: dpkg-deb is not installed\n'
  exit 0
fi

if [ -z "$DEB_ARCH" ]; then
  printf 'Skipping .deb: unsupported target architecture %s\n' "$TARGET"
  exit 0
fi

mkdir -p "$DEBROOT/DEBIAN"
cp -a "$APPDIR/usr" "$DEBROOT/usr"

# Keep the declared libc baseline aligned with the host used to build the
# distributable. Building in an older supported container therefore lowers
# the package requirement automatically instead of silently producing a .deb
# that APT can install but whose executable cannot start.
GLIBC_VERSION=$(LC_ALL=C grep -aoE 'GLIBC_[0-9]+(\.[0-9]+)+' \
  "$APPDIR/usr/bin/bexplorer" 2>/dev/null \
  | sed 's/^GLIBC_//' \
  | sort -V \
  | tail -n 1 || true)
case "$GLIBC_VERSION" in
  '') printf 'Could not determine the minimum GLIBC version required by BExplorer\n' >&2; exit 1 ;;
  *[!0-9.]*) printf 'Invalid GLIBC version detected: %s\n' "$GLIBC_VERSION" >&2; exit 1 ;;
  *)
    LIBC_DEPENDENCY="libc6 (>= $GLIBC_VERSION)"
    printf 'Detected GNU libc baseline: %s\n' "$GLIBC_VERSION"
    ;;
esac

cat > "$DEBROOT/DEBIAN/control" <<EOF
Package: bexplorer
Version: $VERSION
Section: utils
Priority: optional
Architecture: $DEB_ARCH
Maintainer: BExplorer Project <noreply@github.com>
Depends: $LIBC_DEPENDENCY,
 libgcc-s1,
 libstdc++6,
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
 kio-fuse
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
sha256sum "$DEB" > "$DEB.sha256.txt"
printf 'Created %s\n' "$DEB.sha256.txt"
