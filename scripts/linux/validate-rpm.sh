#!/usr/bin/env sh
set -eu

usage() {
  printf 'Usage: %s path-to-package.rpm\n' "$0"
}

case "$#" in
  1) ;;
  *) usage >&2; exit 2 ;;
esac

RPM=$1
if [ ! -f "$RPM" ]; then
  printf 'Package not found: %s\n' "$RPM" >&2
  exit 1
fi
if ! command -v rpm >/dev/null 2>&1; then
  printf 'rpm is required to validate %s\n' "$RPM" >&2
  exit 1
fi

PACKAGE=$(rpm -qp --queryformat '%{NAME}' "$RPM")
VERSION=$(rpm -qp --queryformat '%{VERSION}' "$RPM")
ARCH=$(rpm -qp --queryformat '%{ARCH}' "$RPM")
VENDOR=$(rpm -qp --queryformat '%{VENDOR}' "$RPM")

[ "$PACKAGE" = bexplorer ] || {
  printf 'Unexpected package name: %s\n' "$PACKAGE" >&2
  exit 1
}
if [ -n "${EXPECTED_VERSION:-}" ] && [ "$VERSION" != "$EXPECTED_VERSION" ]; then
  printf 'Unexpected package version: %s (expected %s)\n' \
    "$VERSION" "$EXPECTED_VERSION" >&2
  exit 1
fi
if [ -n "${EXPECTED_ARCH:-}" ] && [ "$ARCH" != "$EXPECTED_ARCH" ]; then
  printf 'Unexpected package architecture: %s (expected %s)\n' \
    "$ARCH" "$EXPECTED_ARCH" >&2
  exit 1
fi
[ "$VENDOR" = "BExplorer Project" ] || {
  printf 'Unexpected package vendor: %s\n' "$VENDOR" >&2
  exit 1
}

rpm -qpl "$RPM" | grep -Fqx '/usr/bin/bexplorer' || {
  printf 'Package does not contain executable /usr/bin/bexplorer\n' >&2
  exit 1
}

if rpm -qip "$RPM" | grep -Fqi 'Bryam Contreras'; then
  printf 'Package metadata exposes a personal maintainer name\n' >&2
  exit 1
fi

printf 'Validated %s %s (%s)\n' "$PACKAGE" "$VERSION" "$ARCH"
