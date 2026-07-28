# Changelog

All notable user-facing changes to BExplorer are documented here.

## Unreleased

- Visible local folders now refresh from native filesystem notifications
  instead of polling. BExplorer keeps one non-recursive watcher, observes at
  most the two visible panes, coalesces write bursts, and preserves selection
  and scroll position while updating.
- The sidebar recycle bin now uses the active Linux icon theme or the native
  Windows Shell icon, switches between empty and full states asynchronously,
  and keeps the embedded icon only as a fallback.
- Rename editors now prioritize text clipboard commands over file commands in
  every view. Copy, cut, paste, and select-all share one stateful editor,
  pasted text remains on one filename-safe line, and copying from rename or
  read-only text previews clears stale internal file-clipboard state.
- Creating or renaming items in large icon views now keeps short names
  centered beneath their icon while preserving wrapping for long names.
- The Windows installer icon has been regenerated from the current canonical
  application artwork, matching the icon embedded in the executable.

## 1.0.4 - 2026-07-27

- Fixed KDE and GNOME launchers continuing to display the previous application
  icon because the hicolor sizes had not been regenerated from the canonical
  icon. Linux packaging now validates all installed sizes before building.
- Added native Trash views on Windows and Linux with restore, delete, empty,
  grouping, preview, contextual actions, and localized status text.
- Added Send to submenus for removable storage and desktop-provided Bluetooth
  and mail destinations, using native themed icons and reliable submenu
  positioning.
- Added a setting to reveal normally hidden system units while filtering boot,
  home, reserved, and implementation-specific mounts by default. Removable
  media and portable devices now expose duplicate cleanup without offering it
  on protected system units.
- Made the interface scale its row, card, dialog, toolbar, sidebar, icon, and
  control dimensions with the configured font size. Properties and Settings
  gained responsive widths, clearer headers, aligned actions, and improved
  long-text handling.
- Made title-bar tabs single-line, ellipsized, and adaptively narrower as more
  tabs open. Split panes retain independent tab areas aligned with their
  content divider.
- Added recursive duplicate cleanup for local folders. The new context-menu
  action opens a live progress window with selectable, creation-date-sorted
  results grouped by extension and classified as Original, Exact match, or
  Possible match. Exact matches require the same name, extension, and size;
  possible matches require a similar name and the same extension. Selected
  candidates require confirmation and move to the native system trash, after
  which the folder is scanned again automatically.
- Multi-file transfers now publish cumulative byte progress every 8 MiB or
  80 ms while each current file is being written. Normal copies no longer
  force a blocking physical flush after every file, eliminating repeated
  multi-second stalls on USB media; atomic replacements and cross-filesystem
  moves retain the required durability before the original is discarded.
- Fixed context submenus so they align with the row that opened them at every
  configured font size. Send-to destinations now use the active desktop icon
  theme, and Linux removable volumes prefer UDisks/udev labels and partition
  names over UUID-based mount-directory names.
- Portable-device icons now follow the native Linux backend: KDE/KIO uses its
  `multimedia-player` theme icon, while GNOME/GVfs preserves the themed-icon
  priority advertised by GIO. MTP, camera, and iOS fallbacks remain compatible
  with Breeze, Adwaita, and other Freedesktop icon themes.
- Added native Linux MTP discovery and file operations for phones and tablets
  that are not already mounted as filesystem paths. KDE uses the stable
  `org.kde.kmtpd5` D-Bus interfaces supplied by KIO Extras, while GNOME and
  other desktops use GVfs/GIO and its FUSE mount. Connection changes refresh
  automatically, and navigation, image thumbnails, search, copy, move, and delete
  share the existing portable-device workflow used on Windows.
- File transfers, moves, deletions, archive jobs, and Defender scans now keep
  running when the main explorer window is closed. Their progress windows
  remain independent, the temporary operation host exits after the final job,
  and launching BExplorer again safely reopens the existing instance instead
  of abandoning or duplicating its active work.
- Added a scalable play icon and filmstrip side rails to video thumbnails so
  video frames remain distinguishable from image files without modifying the
  native or shared thumbnail cache. Compact views retain the play icon without
  crowded film perforations.
- Integrated Windows image and video thumbnails with the system-wide Shell
  thumbnail cache. BExplorer performs cache-only reads first, invokes installed
  native thumbnail providers only on a miss with bounded concurrency, and
  retains its internal image decoder as a portable fallback. Internally decoded
  images are persisted in a metadata-invalidated BExplorer cache with automatic
  age and size cleanup, avoiding repeated source decoding without adding an
  FFmpeg or desktop-specific dependency.
- Unified local image and video thumbnails on Linux around the shared
  Freedesktop XDG cache. BExplorer now generates and caches common image formats
  internally with Exif orientation and bounded memory, invokes registered
  `.thumbnailer` providers or XFCE Tumbler when useful, and falls back to
  bounded `ffmpegthumbnailer`/`ffmpeg` frame extraction for media formats the
  internal decoder cannot handle. Large image files are no longer rejected
  solely by their compressed byte size. File URIs match GLib/KDE encoding for
  names containing punctuation, and generated previews can be reused by other
  compliant file managers.
- Added direct TAR.GZ/TGZ browsing and layered extraction so BExplorer shows
  the TAR contents instead of an empty GZip container or an intermediate
  `.tar` file.
- Fixed Fedora 44/Plasma 6.7 window blur by supporting the standardized
  `ext-background-effect-v1` Wayland protocol while retaining the legacy KWin
  protocol for older Plasma releases.
- Fixed the abrupt opaque/translucent transition on newer KWin versions by
  preferring an 8-bit alpha surface instead of KWin's 2-bit-alpha
  `Rgb10a2Unorm` fallback, and by matching Iced's premultiplied renderer output
  with a premultiplied Wayland surface. The KDE transparency curve is
  continuous and redundant nested fills no longer mask the blur across the
  explorer's content.
- Transparency changes are previewed over the live KWin blur instead of a
  frozen backdrop captured when Settings was opened.
- Fixed dark system themes switching to the light fallback when file
  operations opened a transfer or archive window on Wayland.

## 1.0.3 - 2026-07-24

- Fixed the file view jumping to the top when Control was pressed while
  preserving Control+wheel view-size changes.
- Made rename editing select only the filename stem initially while keeping the
  extension editable in Details, Tiles, and every icon view.
- Added dedicated 48 px thumbnail and native-icon sources for compact views and
  the sidebar; Tiles continue using the large source.
- Added automated GitHub Actions packaging for the Windows Setup executable,
  Debian package, and RPM package.
- Refined Windows installer metadata and shortcuts: language is selected during
  setup, the Start Menu shortcut is created by default, the Desktop shortcut is
  optional, and the uninstaller is displayed simply as BExplorer.

## 1.0.2 - 2026-07-15

- Fixed Debian/Ubuntu installation failures by keeping optional clipboard,
  X11, filesystem, network, and desktop helpers out of strict dependencies;
  package generation now validates that classification automatically.
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
