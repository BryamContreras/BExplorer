#!/usr/bin/env sh
set -eu

[ "$#" -eq 1 ] && [ -f "$1" ] || {
  printf 'Usage: %s package.rpm (inside a fresh Fedora container)\n' "$0" >&2
  exit 2
}
[ "$(id -u)" -eq 0 ] || { printf 'Run this installation smoke test as root inside a fresh container\n' >&2; exit 1; }
DISTRIBUTION=$(sed -n 's/^ID=//p' /etc/os-release | tr -d '"')
[ "$DISTRIBUTION" = fedora ] || { printf 'This smoke test requires Fedora\n' >&2; exit 1; }
RPM=$(readlink -f "$1")
VERSION=$(rpm -qp --queryformat '%{VERSION}' "$RPM")

# No development packages or optional integrations may mask a missing Depends.
dnf --assumeyes --setopt=install_weak_deps=False install "$RPM"
rpm --verify bexplorer
LIBRARIES=$(ldd /usr/bin/bexplorer)
printf '%s\n' "$LIBRARIES"
if printf '%s\n' "$LIBRARIES" | grep -q 'not found'; then
  printf 'Installed binary has unresolved ELF libraries\n' >&2
  exit 1
fi
for HELPER in gio gsettings xdg-open pkexec udisksctl; do
  command -v "$HELPER" >/dev/null 2>&1 || { printf 'Missing core desktop helper: %s\n' "$HELPER" >&2; exit 1; }
done
[ "$(/usr/bin/bexplorer --version)" = "BExplorer $VERSION" ] || {
  printf 'Installed executable does not report package version %s\n' "$VERSION" >&2
  exit 1
}
printf 'Fedora RPM installation, ELF dependencies, and version %s passed\n' "$VERSION"
