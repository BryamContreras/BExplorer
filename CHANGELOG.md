# Changelog

All notable user-facing changes to BExplorer are documented here.

## 1.0.2 - 2026-07-15

- Added a compact native Linux Properties window with General, Permissions,
  and Details tabs, including rename, recursive size, timestamps, filesystem
  information, owner/group selection, mode bits, advanced Unix permissions,
  application associations, and themed application icons.
- Added complete symbolic-link classification and navigation so links to
  folders open as folders, links to files open as files, and broken links keep
  their own safe metadata and error handling.
- Added application discovery and a functional Open with submenu on Linux.
  “Choose another application” now uses the XDG Desktop Portal `OpenFile`
  method with a real file descriptor and runs outside the UI thread.
- Added repeated-letter keyboard navigation to context menus and Linux
  owner/group/application selectors, matching the existing file-list
  typeahead behavior.
- Fixed the address bar so it returns to breadcrumb mode when focus moves
  elsewhere instead of remaining in text-edit mode.
- Extended Linux network discovery with saved KDE places, bounded KIO SMB
  discovery, and KIOFuse mount resolution while retaining GVfs, Samba, and
  Avahi as the primary cross-desktop providers.
- Improved Debian packaging with explicit runtime dependencies, an appropriate
  desktop-portal backend, themed hicolor icons, automatic glibc baseline
  detection, and accurate package-cache updates.
- Updated the Linux desktop entry to accept `%f`, open the requested folder,
  resolve files to their containing folder, and use the themed BExplorer icon.

## 1.0.1 - 2026-07-14

- Fixed the transient native Windows frame that could appear on the first
  external file drag into a non-maximized BExplorer window.
- Reworked the network-printer fallback icon with a clearer dimensional body,
  balanced paper trays, and better small-size rendering.
- Hardened local Windows packaging so application resources embed outside a
  Visual Studio shell, per-user Inno Setup installs are detected, and elevated
  shortcuts use the application directory as their working directory.

- Removed the redundant straight accent strip above the rounded file-drag
  card, leaving its border and shadow to define the floating surface cleanly.
- Added Windows-style cut feedback across every file view: pending cut items
  are subtly dimmed and recover immediately when copy replaces the clipboard.
- Fixed pointed KWin/Wayland window corners by constraining the native blur to
  a rounded region that follows every main and utility window resize.
- Reduced excessive transparency with KWin blur by raising the Linux surface
  opacity floor while preserving a clearly visible native blur effect.
- Made transfer, compression, and Defender windows use the same native
  transparency/blur surface as the main explorer while keeping their content
  cards lightly tinted and readable without masking that effect.
- Removed the native file-drag idle polling loop: outbound polling now runs
  only during an actual drag, while incoming Wayland drops wake the UI through
  an event-driven channel.
- Fixed Linux sidebar and bookmark-bar drive icons so secondary fixed disks
  mounted below `/media` keep their local-disk appearance instead of looking
  like removable USB storage.
- Fixed orderly shutdown on Linux Wayland by releasing native drag-and-drop
  and KWin blur resources before their borrowed window/display handles are
  destroyed, preventing normal closes from being reported as crashes.
- Added external and secondary local-drive formatting on Linux through UDisks2
  and Polkit, with safe unmount/remount handling and guards for the physical
  system disk, firmware, loop, layered, RAID, whole disks with child
  partitions, and multi-device Btrfs storage.
- Fixed Linux optical-media removal so CD/DVD drives use UDisks2 Eject instead
  of reporting a USB power-off error after a successful unmount.

## 1.0.0 - 2026-07-13

First stable release for Windows and Linux.

### Highlights

- Tabbed navigation, split panes, independent histories, and session restore.
- Details, list, icon, tile, grouping, sorting, filtering, and complete search views.
- Queued copy and move operations with progress, pause, cancel, undo, conflict handling,
  elevated retry, and synchronized staged replacement.
- ZIP and embedded 7-Zip browsing, compression, extraction, passwords, progress, and search.
- Image, text, source, SVG, and multi-page PDF previews.
- Windows Defender, WPD/MTP, network, disk-image, shell, clipboard, and drag-and-drop integration.
- Linux storage, GVfs/FUSE MTP, network discovery, UDisks2, Polkit, native clipboard,
  Wayland drag-and-drop, KWin blur, and optional Blur My Shell integration.
- Atomic configuration and session persistence with regression coverage for critical file operations.
- Debian packaging with `/usr/bin` and desktop integration, plus a bilingual Inno Setup installer
  with Start Menu, optional Desktop shortcut, and managed PATH integration.
