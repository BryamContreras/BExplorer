# BExplorer

![Rust 2024](https://img.shields.io/badge/Rust-2024-orange)
![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux-blue)
![Version](https://img.shields.io/badge/version-1.0.2-brightgreen)
![Status](https://img.shields.io/badge/status-stable-brightgreen)
![License](https://img.shields.io/badge/license-MIT-green)

BExplorer is a stable desktop file explorer for Windows and Linux written in
Rust with `iced`. It focuses on fast file management, split-pane workflows,
archive handling, preview support, and a modern compact interface without
turning into a heavy system shell replacement.

Spanish documentation is available in [docs/es/README.md](docs/es/README.md).

The project keeps platform-specific code isolated behind a shared facade.
Windows and Linux are supported targets with native integrations appropriate to
each system. macOS remains an experimental future target behind separate
platform modules.

## Status

BExplorer 1.0.2 is the current stable Windows and Linux version. Its desktop
interface, file-operation engine, archive workflows, platform integrations,
configuration format, and session format form the supported 1.x baseline.
Development now prioritizes compatibility fixes, reliability, and focused
improvements over broad rewrites.

The current `iced` interface covers local browsing, tabs, split panes, view
modes, filtering, grouping and column sorting, rename, background file deletion,
and queued copy/move operations. Session changes are persisted as they happen
and large directories are rendered incrementally instead of being silently
truncated. Complete search, previews, archive jobs, Microsoft Defender actions,
MTP transfers, disk-image mounting, network discovery, native drag-and-drop,
symbolic links, native properties, and application selection are connected to
the `iced` interface.

Linux provides local and removable storage, file operations, archives,
previews, search, GVfs/FUSE portable devices, GVfs/Samba/Avahi network
discovery with optional KIO enrichment for SMB, UDisks2, Polkit, XDG portals,
Freedesktop icons and thumbnails, clipboard interoperability, and Wayland/X11
support. The Debian package declares the services needed for these integrations
as dependencies; source builds still keep readable fallbacks when an optional
desktop-specific component is unavailable.

## Highlights

- Native Rust desktop application using `iced`.
- Tabbed navigation with independent history and session restore.
- Split-pane mode with independent per-pane view state.
- Freedesktop directory launch integration: `bexplorer %f` opens a requested
  directory, or the containing directory when an application passes a file.
- Incremental rendering for directories with thousands of entries.
- Background rename, create-folder, trash, and permanent-delete operations so
  slow disks and network paths do not block the UI thread.
- Background storage monitoring refreshes disks, USB media, optical drives, and
  mounted portable devices on Windows and Linux without blocking the UI.
- Resizable and reorderable sidebar.
- Optional action bar and bookmark bar.
- Details, list, icons, large icons, extra-large icons, and tile views.
- Local drives, removable drives, mounted ISO images, UNC paths, network
  locations, Linux mount points, and Windows MTP portable devices.
- Symbolic-link awareness on Linux: links to directories navigate as folders,
  links to files open as files, and broken links remain visible and identifiable.
- Non-system drive formatting with native Windows elevation or UDisks2/Polkit
  on Linux. Linux permits external drives and secondary local disks while
  blocking the physical system disk, firmware, loop, layered, and RAID
  devices, then unmounts and remounts the selected filesystem safely.
- Progressive network discovery with cached results.
- Windows Explorer-compatible file clipboard for regular file paths, plus
  native MIME clipboard helpers and a text/URI-list fallback for Linux.
- Internal and external drag-and-drop support.
- Drag polling sleeps until a drag is prepared or active instead of waking the
  application continuously while idle.
- Transfer queue with progress, pause, cancel, cleanup of partial files, and
  conflict handling.
- Per-pane search and transfer progress in split view; closing one pane keeps
  its operations running and transfers progress ownership to the remaining pane.
- One-level undo for completed copies, moves, and moves to trash.
- Transactional local replacements: the complete new file or directory is
  copied and synchronized beside the destination before the existing item is
  replaced. A failed preparation leaves the previous destination untouched.
- Concurrent archive jobs with a dedicated progress window that is restored
  when a new compression or transfer starts.
- Elevated Microsoft Defender remediation and exclusion actions on Windows.
- Quick search and complete search, including search inside supported archives.
- Resizable preview panel for images, text files, SVG, and PDF.
- Double-click ISO mounting followed by navigation to the mounted image in a
  new tab, plus contextual eject for supported mounted media.
- Native file icons, local thumbnails, and MTP thumbnails when exposed by the
  device.
- Native Windows property sheets and a compact BExplorer properties window on
  Linux with General, Permissions, and Details pages.
- Application-aware **Open with** menus with application names and icons;
  Linux uses the XDG desktop portal for the full application chooser.
- Windows Defender scan integration.
- Configurable theme, accent color, icon borders, window effects, shortcuts,
  and sidebar layout.
- An About dialog with the application icon, version, description, and project
  link.

## File Operations

BExplorer supports the common file-management operations expected from a modern
explorer:

- copy, cut, paste, move, rename, delete, and permanent delete;
- create folders and text documents;
- drag files inside BExplorer, accept desktop file drops on Windows/Linux, and
  drag out through the documented platform integrations;
- copy from and to Windows Explorer through the system clipboard;
- exchange Linux file clipboard MIME data through Wayland/X11 helpers;
- show pending cut entries with reduced opacity until the clipboard operation
  changes or completes;
- copy, paste, and delete files on supported MTP devices;
- copy and move across local drives, removable drives, and UNC shares;
- undo the most recent completed copy, move, or move-to-trash operation;
- retry supported operations through UAC on Windows or Polkit on Linux when
  access is denied.

Conflict resolution is available for copy and move operations:

- `Replace`: prepare and synchronize the complete replacement before changing
  the existing destination item.
- `Skip`: leave the destination item untouched.
- `Keep both`: copy the new item using a numbered name such as
  `report (2).txt`.

## Symbolic Links, Properties, and Applications

Linux symbolic links are classified without losing the identity of the link
itself. A valid directory link can be browsed, a valid file link is opened by
its target type, and a broken link produces an explicit error rather than being
treated as an empty file. Properties show both the stored target and resolved
target. BExplorer does not silently apply link permission changes to the target.

Windows continues to use the native Shell property sheet. Linux uses a native
BExplorer properties window that supports:

- one or multiple local files and directories;
- rename, logical size, allocated size, timestamps, MIME type, mount point,
  filesystem, backing device, free space, UID/GID, inode, and hard-link count;
- a background directory-size calculation that keeps the UI responsive;
- owner and group selection through the system identity database;
- read, write, and execute permissions for owner, group, and others;
- setuid, setgid, and sticky bits, with optional recursive application;
- elevated permission/ownership changes through Polkit when required;
- viewing installed applications with their desktop-entry names and icons, and
  changing the default application for a MIME type through `xdg-mime`.

The Linux **Choose another application** action calls the XDG Desktop Portal
`OpenFile` method with a file descriptor and `ask=true`. It falls back to the
real `mimeopen --ask` chooser when available; it never silently launches the
current default application while presenting that action as a chooser. The
context-menu submenu can also launch a specific compatible desktop application
directly.

## Archive Support

BExplorer includes archive browsing, compression, extraction, and password
support.

Supported workflows:

- create ZIP archives natively from Rust;
- create 7z archives through the embedded 7-Zip engine;
- create password-protected ZIP and 7z archives;
- choose archive name, ZIP/7z format, and fast, normal, or high compression
  from the action bar; context menus also offer one-click normal ZIP or 7z;
- run multiple compression jobs at once with individual progress and cancel
  controls;
- extract ZIP, 7z, RAR, ISO, TAR, and other formats supported by 7-Zip;
- extract password-protected archives when a password is provided;
- browse common archive formats as folders;
- extract selected archive entries into normal folders;
- search inside supported archives during complete search.

On Windows and Linux, the 7-Zip engine is built from `vendor/7zip-src` and
linked through `vendor/7zip-ffi`. BExplorer does not ship or execute an external
`7zr.exe`.

## Preview Panel

The preview panel is resizable and can be toggled from the action bar.

Currently supported:

- images;
- plain text and source-like text files;
- SVG;
- PDF, including multi-page preview.

Unsupported file types show a clear "no preview available" message instead of
blocking the UI.

## Platform Support

| Feature | Windows | Linux | macOS |
| --- | --- | --- | --- |
| Local file browsing | Supported | Supported | Experimental |
| Tabs and split view | Supported | Supported | Experimental |
| File transfers | Supported | Supported | Experimental |
| Archive browsing/extraction | Supported | Supported | Experimental |
| Native icons/thumbnails | Supported | Supported | Experimental |
| Symbolic links | Shell behavior | File/folder/broken-link aware | Experimental |
| Properties | Windows Shell sheet | Native BExplorer dialog | Not supported |
| Open with / app chooser | Windows Shell | XDG portal + desktop entries | Not supported |
| MTP portable devices | WPD/MTP | Mounted GVfs/FUSE devices | Not supported |
| Network discovery | Native providers | GVfs/Samba/Avahi + optional KIO SMB enrichment | Mounted SMB only |
| ISO mount/eject | Supported | UDisks2 | Experimental |
| Non-system drive format | Format-Volume | UDisks2 D-Bus | Experimental |
| Window blur | Native Windows effects | KWin / Blur My Shell | Experimental |
| Windows Defender scan | Supported | N/A | N/A |

The platform facade lives in `src/platform/mod.rs`. Windows-specific logic is
kept under `src/platform/windows/` and related Windows shell helpers. Linux
support uses `/proc/self/mountinfo`, sysfs, `xdg-terminal-exec`/common terminal
fallbacks, and a Freedesktop `.desktop` entry without binding the application to
one desktop environment. Linux windowing is handled by `iced`/`winit`, so the
application can run under Wayland or X11 when the required runtime libraries are
available.

### Linux distribution compatibility

The source tree targets GNU/Linux rather than a single desktop environment.
GNOME and KDE Plasma are the primary integration targets; XDG, GVfs, UDisks2,
and Polkit keep most functionality desktop-neutral. Distribution compatibility
must distinguish the source code from a prebuilt binary:

| Distribution/environment | Project status | Current locally built `.deb` |
| --- | --- | --- |
| Debian 13, GNOME | Manually tested | Compatible |
| Debian 13, KDE Plasma | Manually tested | Compatible; KIO enrichment available when installed |
| Ubuntu 26.04, GNOME | Manually tested | Compatible |
| Ubuntu 24.04 LTS and derivatives with the same or newer ABI | Supported package baseline; clean-machine testing remains recommended | Compatible when the declared packages are available |
| Debian 12 | Functionally tested from a compatible local build | **Not compatible** with the current artifact (`libc6` is too old) |
| Ubuntu 22.04 LTS and derivatives | Older baseline; rebuild and test from source there | **Not compatible** with the current artifact (`libc6` is too old) |
| Other Debian/Ubuntu derivatives | Expected to run when they provide the declared runtime services and a compatible ABI; not all desktop combinations are tested | Depends on architecture, `libc6`, and package availability |
| Fedora, Arch, openSUSE, and other families | Source builds only; not currently a packaged/tested release target | The Debian package is not supported |

The `.deb` produced in the current build environment is GNU/Linux `amd64` and
declares `libc6 (>= 2.39)`. That covers the ABI baseline of Ubuntu 24.04 or
newer and Debian 13, but not Debian 12 or Ubuntu 22.04. This is a property of
the distributed binary, not an intentional source-code restriction: the
packaging script detects the highest GLIBC symbol required by the executable.
To support an older distribution, compile and test on that oldest baseline and
publish the resulting package; do not only weaken the dependency metadata.

## Interface Architecture

The `iced` interface is split by responsibility under `src/iced_ui`:

- `mod.rs` owns application state, messages, subscriptions, and window setup;
- `update.rs` routes messages and asynchronous operation results;
- `interaction.rs`, `navigation.rs`, and `search_state.rs` own input, movement,
  selection, drag-and-drop, and search state;
- `view.rs` and `view/` build window chrome, file surfaces, and dialogs;
- `properties.rs` and `view/dialogs/properties.rs` connect the native Linux
  properties backend to its General, Permissions, and Details pages;
- `file_actions.rs` owns clipboard, transfers, rename, delete, compression, and
  shell actions;
- `advanced.rs` connects Defender, MTP, disk images, and removable drives;
- `helpers.rs` and `helpers/` contain formatting, layout, icon, theme, and
  persistence helpers.

Directory views initially build 500 entries and automatically append additional
batches near the end of the scroll area. All matching entries remain available;
the batching only limits how many widgets are constructed at once.

On Linux, file icons are resolved through the Freedesktop icon theme layout and
Shared MIME Info database. Image thumbnails first try the standard XDG
thumbnail cache and then fall back to BExplorer's internal thumbnail generation.
Disk image mount/eject uses UDisks2 through `udisksctl` when available.
Non-system drive formatting uses the stable UDisks2 D-Bus API so authorization
is handled by the distribution's Polkit policy. Elevated file-operation retry
uses Polkit through `pkexec`, and network discovery uses available
Freedesktop/GVfs, Avahi, and Samba helpers. On KDE, cached Dolphin places,
active KIOFuse mounts, and short non-interactive `kioclient` probes complement
those providers without replacing them or leaving an unbounded network scan on
the UI thread. The XDG application chooser also runs away from the UI thread.

KDE Plasma/Wayland uses KWin's optional native blur protocol. GNOME/Mutter does
not expose its Shell blur actor as a Wayland client protocol, so BExplorer uses
the optional [Blur My Shell](https://extensions.gnome.org/extension/3193/blur-my-shell/)
extension for GNOME application blur. Selecting Blur registers the `bexplorer`
application ID with that extension and disables its focused-window opacity
override so the active explorer remains blurred; disabling the effect removes
BExplorer's entry again. If the extension is unavailable, BExplorer keeps an
opaque readable background.

## Known Limitations

- Windows and Linux use different native facilities, so a few integrations are
  intentionally platform-specific rather than identical.
- Direct WPD/MTP sessions and Microsoft Defender are Windows-only. Linux exposes
  portable devices mounted by GVfs as regular storage paths.
- macOS has initial storage, disk-image, eject, and mounted-SMB adapters but
  still needs broader runtime testing and native drag-and-drop integration.
- Browsing an unmounted authenticated SMB share on Linux or macOS may still
  require connecting it through the desktop environment first.
- KIO support enriches SMB discovery and resolves existing KIOFuse mounts; it
  is optional and is not a replacement for credentials or mounting a remote
  share.
- The built-in Linux properties dialog applies to real local or mounted paths,
  not virtual roots such as the top-level Network or portable-device view.
- Setting BExplorer as the default directory handler does not replace the
  toolkit-owned file picker embedded in GTK or Qt applications. Those dialogs
  continue to be provided by the toolkit or the desktop portal.
- Linux file clipboard interoperability uses native MIME helpers when
  `wl-copy`/`wl-paste`, `xclip`, or `xsel` are installed, with a text fallback.
- Linux icon theme lookup is implemented in-process and may not yet cover every
  desktop-specific theme extension.
- Native Linux drag-out is implemented for Wayland. On X11, select a helper
  explicitly with `BEXPLORER_DRAG_HELPER`; setting
  `BEXPLORER_DRAG_HELPER_FALLBACK=1` also permits a known helper such as
  `ripdrag`, `dragon-drag-and-drop`, `dragon`, or `dragon-drop` when the native
  Wayland path is unavailable.
- Linux MTP support currently covers devices already mounted by GVfs/FUSE.
- Copying directly between two folders on the same MTP device is not supported.
- Extracting selected archive entries directly into an MTP device is blocked;
  extract to a normal folder first.
- Virtual roots such as `Network` or a portable-device root are not transfer
  destinations. Enter a real folder, UNC share, or MTP folder first.
- Administrator retry does not permanently change folder ACLs. It only retries
  the requested operation with elevated permissions.
- Archive extraction conflicts currently keep both files automatically with
  numbered names.
- Network discovery depends on Windows services, providers, credentials, and
  the local network. Devices may appear progressively.
- Distributed Windows builds may show a SmartScreen warning until a signed
  installer is provided; the portable archive includes a SHA-256 checksum.

## Build Requirements

Windows requirements:

- Rust 1.92 or newer installed with `rustup`.
- Visual Studio Build Tools with C++ support.

Recommended commands:

```powershell
cargo check
cargo test
cargo run
```

Optimized build:

```powershell
cargo build --release
```

The release executable is generated at:

```text
target/release/bexplorer.exe
```

Linux source-build requirements:

- Rust 1.92 or newer installed with `rustup`.
- A C/C++ toolchain usable by the `cc` crate, such as GCC, Clang, or Zig
  wrappers for `cc`, `c++`, `ar`, and `ranlib`.
- Development headers and runtime libraries required by `iced`/`winit` for
  Wayland/X11 and OpenGL on the build distribution.

The generated Debian package makes the following integration groups mandatory
so a normal installation does not silently lose advertised features:

- Wayland/X11/OpenGL runtime libraries;
- GLib/GIO, `xdg-utils`, the XDG Desktop Portal, and a portal backend;
- UDisks2, `pkexec`, and filesystem tools for ext, FAT, exFAT, NTFS, Btrfs, and
  XFS operations;
- GVfs backends/FUSE, Samba, and Avahi for mounted portable devices and network
  discovery;
- Wayland and X11 clipboard helpers;
- Shared MIME information, desktop database, and the hicolor icon theme.

Optional desktop-specific enhancements remain:

- Blur My Shell for application blur on GNOME Wayland; the fallback is opaque.
- `kde-cli-tools`, `kio-extras`, and `kio-fuse` for additional KDE discovery and
  already-mounted KIO paths. They remain suggested rather than pulling KDE into
  a GNOME installation.
- `xsel` or `libfile-mimeinfo-perl` as additional clipboard/application-chooser
  fallbacks.
- `ripdrag`, `dragon-drag-and-drop`, `dragon`, or `dragon-drop` for drag-out on
  X11 or as a Wayland fallback. `BEXPLORER_DRAG_HELPER` selects a custom helper,
  while `BEXPLORER_DRAG_HELPER_FALLBACK=1` enables automatic fallback to a
  known installed helper.

Recommended commands:

```bash
cargo check
cargo test
cargo run
```

Optimized build:

```bash
cargo build --release
```

## Packaging Notes

BExplorer currently builds as a mostly self-contained executable.

Windows portable packages and the graphical installer can be built with:

```powershell
scripts/windows/package.ps1
```

This requires Inno Setup 6 or 7 to be installed. The script creates a versioned
portable ZIP, a bilingual Inno Setup installer, and SHA-256 checksums under
`dist/`. The installer:

- lets the user choose English or Spanish;
- installs BExplorer under Program Files;
- creates its Start Menu entry by default;
- offers an optional Desktop shortcut;
- offers adding BExplorer to the system `PATH`, enabled by default;
- removes only its own `PATH` entry during uninstall.

Use `-SkipInstaller` to create only the portable ZIP. A manual portable
installation can copy:

```text
BExplorer.exe
README.md
LICENSE
THIRD_PARTY_NOTICES.md
```

to a stable folder such as:

```text
C:\Program Files\BExplorer\
```

and create Start Menu/Desktop shortcuts.

Configuration and session data are stored in the user's application-data
directory, not next to the executable.

Because release builds include the embedded 7-Zip engine, binary distributions
should also include the 7-Zip license information from
`vendor/7zip-src/DOC/License.txt`, `vendor/7zip-src/DOC/copying.txt`, and
`vendor/7zip-src/DOC/unRarLicense.txt`, or otherwise provide equivalent
third-party notices.

Linux packages can be built with:

```bash
scripts/linux/package.sh
```

The script creates a versioned tarball and SHA-256 checksum under `dist/` and,
when `dpkg-deb` is installed, a `.deb` package and checksum. The package
contains the desktop entry, metainfo, Polkit policy, hicolor icons from 16 to
512 pixels, license, third-party notices, and the original 7-Zip license texts.
It installs the executable as `/usr/bin/bexplorer`, registers BExplorer as an
available `inode/directory` handler, and uses `Exec=bexplorer %f` so a desktop
invocation navigates to the requested folder. This makes BExplorer selectable
as the default file manager without silently changing the user's current
association. If a caller passes a file, BExplorer opens its containing folder.

The control file declares the runtime integrations described under Build
Requirements. It also derives the minimum `libc6` version from the built
executable instead of hard-coding a misleading baseline. The package generated
in the current environment requires `libc6 >= 2.39`; Debian 12 and Ubuntu 22.04
need a package rebuilt and tested on an older compatible build base.

Install the generated package and its dependencies with:

```bash
scripts/linux/install-deb.sh
```

Both legacy commands under `tools/` remain as compatibility wrappers.

## Local Data

BExplorer uses the `directories` crate for user configuration paths.

Typical locations:

- Windows: `%APPDATA%\BExplorer\BExplorer\config`
- Linux: `~/.config/bexplorer`
- macOS: `~/Library/Application Support/dev.BExplorer.BExplorer`

Important files:

- `config.json`: theme, language, favorites, recents, view preferences, sidebar
  layout, preview settings, shortcuts, and visual preferences.
- `session.json`: open tabs, history, active paths, and split-pane state.
- `bexplorer.log`: non-fatal errors and diagnostic events.

Configuration and session JSON are written through synchronized sibling files
and atomically replaced, so an interrupted save does not expose a partially
written document.

## Project Layout

```text
src/
  main.rs
  app/
    config.rs
    session.rs
    thumbnail_data.rs
  fs/
    archive.rs
    archive/                 # ZIP, 7-Zip and shared archive types
    archive_listing.rs
    explorer.rs
    explorer/                # Platform storage enumeration
    operations.rs
    portable.rs
    properties.rs            # Native Linux metadata and permission backend
    search.rs
    transfer_queue.rs
  iced_ui/
    mod.rs
    state.rs
    update.rs                # Exhaustive Message dispatcher
    interaction/             # Context, selection, drag and layout input
    properties.rs            # Properties state and asynchronous operations
    view/                    # Chrome, menus, dialogs and file presentations
    helpers/
  platform/
    mod.rs
    windows.rs
    windows/
    linux.rs
    linux/                   # Icons, storage watching, blur, KIO and Wayland drag
    macos.rs
    shell/
  utils/
    atomic_file.rs
    errors.rs
    log.rs
    paths.rs
vendor/
  7zip-src/
  7zip-ffi/
scripts/
  linux/                     # Debian/tar packaging and local .deb install
  windows/                   # Portable ZIP and Inno Setup installer
```

## Tests

The current test suite covers:

- archive listing and archive-root behavior;
- ZIP creation and extraction;
- 7z creation and extraction through FFI;
- password-protected ZIP and 7z extraction;
- archive progress reporting;
- concurrent 7z compression jobs;
- extension search;
- complete search inside ZIP archives;
- UNC path handling;
- duplicate filtering for USB/MTP devices;
- basic create-folder and create-file operations;
- transfer conflict policies;
- synchronized staged replacement and preservation of an existing destination
  when replacement preparation fails;
- atomic configuration/session file replacement;
- clipboard and paste shortcut behavior;
- symbolic-link classification for file, directory, and broken targets;
- Linux properties metadata, desktop-entry discovery, ownership/permission
  validation, recursive changes, and elevated-helper request safety;
- application chooser portal request construction and command-line launch-path
  normalization;
- Linux mountinfo parsing, MIME/file URI clipboard helpers, UDisks parsing,
  GVfs/Samba/Avahi/KIO network helper parsing, and XDG thumbnail/icon metadata;
- Linux elevated-operation response handling with `fs.protected_regular`;
- GNOME/KDE blur selection and readable fallbacks.

Run:

```bash
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

## Post-1.0 roadmap

Planned 1.x improvements:

- signed Windows installer and update flow;
- broader distribution and clean-machine compatibility coverage;
- stronger network, MTP, and portal edge-case handling;
- additional preview formats and further properties refinements;
- continued separation and testing of platform layers.

Longer-term exploration:

- broader Linux desktop testing, self-contained drag-out, and richer MTP;
- macOS file-management backend;
- platform-native preview/icon integrations outside Windows;
- optional plugin or extension points.

Release history is recorded in [CHANGELOG.md](CHANGELOG.md). Security issues
should be reported according to [SECURITY.md](SECURITY.md).

## License

BExplorer's own Rust and application code is licensed under the MIT License.

See [LICENSE](LICENSE) for details.

The embedded 7-Zip source under `vendor/7zip-src/` is distributed under its own
licenses. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) and the original
license files in `vendor/7zip-src/DOC/`.
