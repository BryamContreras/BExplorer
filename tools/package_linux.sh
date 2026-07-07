#!/usr/bin/env sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
TARGET=${TARGET:-x86_64-unknown-linux-gnu}
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -n 1)
APPDIR="$ROOT_DIR/dist/bexplorer-linux-$TARGET"
TARBALL="$ROOT_DIR/dist/bexplorer-$VERSION-linux-$TARGET.tar.gz"
DEBROOT="$ROOT_DIR/dist/bexplorer-deb"
DEB="$ROOT_DIR/dist/bexplorer_${VERSION}_amd64.deb"

cargo build --release --target "$TARGET"

rm -rf "$APPDIR" "$DEBROOT"
mkdir -p \
  "$APPDIR/usr/bin" \
  "$APPDIR/usr/share/applications" \
  "$APPDIR/usr/share/icons/hicolor/256x256/apps" \
  "$APPDIR/usr/share/metainfo" \
  "$APPDIR/usr/share/polkit-1/actions" \
  "$APPDIR/usr/share/doc/bexplorer"

install -m 0755 "$ROOT_DIR/target/$TARGET/release/bexplorer" "$APPDIR/usr/bin/bexplorer"
install -m 0644 "$ROOT_DIR/assets/linux/bexplorer.desktop" "$APPDIR/usr/share/applications/bexplorer.desktop"
install -m 0644 "$ROOT_DIR/assets/icons/appicon.png" "$APPDIR/usr/share/icons/hicolor/256x256/apps/bexplorer.png"
install -m 0644 "$ROOT_DIR/assets/linux/io.github.BryamContreras.BExplorer.metainfo.xml" "$APPDIR/usr/share/metainfo/io.github.BryamContreras.BExplorer.metainfo.xml"
install -m 0644 "$ROOT_DIR/assets/linux/io.github.BryamContreras.BExplorer.policy" "$APPDIR/usr/share/polkit-1/actions/io.github.BryamContreras.BExplorer.policy"
install -m 0644 "$ROOT_DIR/README.md" "$APPDIR/usr/share/doc/bexplorer/README.md"
install -m 0644 "$ROOT_DIR/LICENSE" "$APPDIR/usr/share/doc/bexplorer/LICENSE"
install -m 0644 "$ROOT_DIR/THIRD_PARTY_NOTICES.md" "$APPDIR/usr/share/doc/bexplorer/THIRD_PARTY_NOTICES.md"

mkdir -p "$ROOT_DIR/dist"
tar -C "$APPDIR" -czf "$TARBALL" .
printf 'Created %s\n' "$TARBALL"

if command -v dpkg-deb >/dev/null 2>&1; then
  mkdir -p "$DEBROOT/DEBIAN"
  cp -a "$APPDIR/usr" "$DEBROOT/usr"
  cat > "$DEBROOT/DEBIAN/control" <<EOF
Package: bexplorer
Version: $VERSION
Section: utils
Priority: optional
Architecture: amd64
Maintainer: Bryam Contreras <noreply@github.com>
Depends: libc6,
 libgcc-s1,
 libstdc++6,
 libxcb1,
 libxkbcommon0,
 libwayland-client0,
 libegl1,
 libgl1,
 udisks2,
 pkexec | policykit-1,
 polkitd | policykit-1,
 xdg-utils
Recommends: gvfs,
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
  if dpkg-deb --help 2>/dev/null | grep -q -- '--root-owner-group'; then
    dpkg-deb --root-owner-group --build "$DEBROOT" "$DEB"
  else
    dpkg-deb --build "$DEBROOT" "$DEB"
  fi
  printf 'Created %s\n' "$DEB"
fi
