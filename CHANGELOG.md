# Changelog

All notable user-facing changes to BExplorer are documented here.

## Unreleased

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
