#!/usr/bin/env sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -n 1)
[ -n "$VERSION" ] || { printf 'Could not read the BExplorer version from Cargo.toml\n' >&2; exit 1; }

run_as_root() {
  if [ "$(id -u)" -eq 0 ]; then
    "$@"
  elif command -v sudo >/dev/null 2>&1; then
    sudo "$@"
  else
    printf 'Root privileges are required, but sudo is not installed\n' >&2
    return 1
  fi
}

case "${1:-}" in
  -h | --help)
    printf 'Usage: %s [path-to-package.deb]\n' "$0"
    exit 0
    ;;
esac

if [ "$#" -gt 1 ]; then
  printf 'Usage: %s [path-to-package.deb]\n' "$0" >&2
  exit 2
fi

if [ "$#" -eq 1 ]; then
  DEB=$1
else
  if command -v dpkg >/dev/null 2>&1; then
    ARCH=$(dpkg --print-architecture)
  else
    printf 'dpkg is required to detect the Debian architecture\n' >&2
    exit 1
  fi
  DEB="$ROOT_DIR/dist/bexplorer_${VERSION}_${ARCH}.deb"
fi

if [ ! -f "$DEB" ]; then
  printf 'Package not found: %s\nBuild it first with scripts/linux/package.sh\n' "$DEB" >&2
  exit 1
fi

DEB=$(CDPATH= cd -- "$(dirname -- "$DEB")" && pwd)/$(basename -- "$DEB")

if command -v apt-get >/dev/null 2>&1; then
  run_as_root apt-get install -y "$DEB"
elif command -v apt >/dev/null 2>&1; then
  run_as_root apt install -y "$DEB"
else
  run_as_root dpkg -i "$DEB"
fi

if [ ! -x /usr/bin/bexplorer ]; then
  printf 'Installation completed but /usr/bin/bexplorer was not created\n' >&2
  exit 1
fi

printf 'Installed BExplorer %s at /usr/bin/bexplorer\n' "$VERSION"
