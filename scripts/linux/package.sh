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

case "$TARGET" in
  x86_64-*) DEB_ARCH=amd64 ;;
  aarch64-*) DEB_ARCH=arm64 ;;
  i686-* | i586-*) DEB_ARCH=i386 ;;
  *) DEB_ARCH= ;;
esac

DEB="$ROOT_DIR/dist/bexplorer_${VERSION}_${DEB_ARCH:-unsupported}.deb"

cargo build --manifest-path "$ROOT_DIR/Cargo.toml" --release --target "$TARGET"

rm -rf "$APPDIR" "$DEBROOT"
mkdir -p \
  "$APPDIR/usr/bin" \
  "$APPDIR/usr/share/applications" \
  "$APPDIR/usr/share/icons/hicolor" \
  "$APPDIR/usr/share/metainfo" \
  "$APPDIR/usr/share/polkit-1/actions" \
  "$APPDIR/usr/share/doc/bexplorer"

install -m 0755 "$ROOT_DIR/target/$TARGET/release/bexplorer" "$APPDIR/usr/bin/bexplorer"
install -m 0644 "$ROOT_DIR/assets/linux/bexplorer.desktop" "$APPDIR/usr/share/applications/bexplorer.desktop"
for ICON in "$ROOT_DIR"/assets/linux/hicolor/*x*/apps/bexplorer.png; do
  ICON_SIZE=$(basename -- "$(dirname -- "$(dirname -- "$ICON")")")
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
cat > "$DEBROOT/DEBIAN/control" <<EOF
Package: bexplorer
Version: $VERSION
Section: utils
Priority: optional
Architecture: $DEB_ARCH
Maintainer: Bryam Contreras <noreply@github.com>
Depends: libc6,
 libgcc-s1,
 libstdc++6,
 libxcb1,
 libxkbcommon0,
 libwayland-client0,
 libegl1,
 libgl1,
 xdg-utils
Recommends: udisks2,
 pkexec | policykit-1,
 polkitd | policykit-1,
 gvfs,
 gvfs-fuse,
 gvfs-backends,
 gvfs-daemons,
 gvfs-mtp,
 wl-clipboard | xclip | xsel,
 smbclient,
 avahi-utils,
 shared-mime-info,
 hicolor-icon-theme
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
sha256sum "$DEB" > "$DEB.sha256.txt"
printf 'Created %s\n' "$DEB.sha256.txt"
