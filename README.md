# BExplorer

![Rust 2024](https://img.shields.io/badge/Rust-2024-orange)
![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux-blue)
![Status](https://img.shields.io/badge/status-beta-yellow)
![License](https://img.shields.io/badge/license-MIT-green)

BExplorer is a Windows-first desktop file explorer written in Rust with
`iced`. It focuses on fast file management, split-pane workflows,
archive handling, preview support, and a modern compact interface without
turning into a heavy system shell replacement.

Spanish documentation is available in [docs/es/README.md](docs/es/README.md).

The project is designed so platform-specific code stays isolated. Windows is
currently the primary target, Linux has an initial desktop-neutral backend, and
macOS support is kept behind separate platform modules for future work.

## Status

BExplorer is in active beta development. Its desktop interface has been migrated
from `egui` to `iced` and the superseded UI implementation has been removed.

The current `iced` interface covers local browsing, tabs, split panes, view
modes, filtering, grouping and column sorting, rename, background file deletion,
and queued copy/move operations. Session changes are persisted as they happen
and large directories are rendered incrementally instead of being silently
truncated. Complete search, previews, archive jobs, Microsoft Defender actions,
MTP transfers, disk-image mounting, network discovery, and native drag-and-drop
are connected to the `iced` interface.

Linux builds and covers core local browsing/file operations with
Freedesktop/XDG-style integration, but still needs broader runtime testing
across distributions, Wayland/X11 sessions, portals, network mounts, USB
drives, and desktop clipboard implementations.

## Highlights

- Native Rust desktop application using `iced`.
- Tabbed navigation with independent history and session restore.
- Split-pane mode with independent per-pane view state.
- Incremental rendering for directories with thousands of entries.
- Background rename, create-folder, trash, and permanent-delete operations so
  slow disks and network paths do not block the UI thread.
- Resizable and reorderable sidebar.
- Optional action bar and bookmark bar.
- Details, list, icons, large icons, extra-large icons, and tile views.
- Local drives, removable drives, mounted ISO images, UNC paths, network
  locations, Linux mount points, and Windows MTP portable devices.
- Progressive network discovery with cached results.
- Windows Explorer-compatible file clipboard for regular file paths, plus
  native MIME clipboard helpers and a text/URI-list fallback for Linux.
- Internal and external drag-and-drop support.
- Transfer queue with progress, pause, cancel, cleanup of partial files, and
  conflict handling.
- Concurrent archive jobs with a dedicated progress window that is restored
  when a new compression or transfer starts.
- Elevated Microsoft Defender remediation and exclusion actions on Windows.
- Quick search and complete search, including search inside supported archives.
- Resizable preview panel for images, text files, SVG, and PDF.
- Native file icons, local thumbnails, and MTP thumbnails when exposed by the
  device.
- Windows Defender scan integration.
- Configurable theme, accent color, icon borders, window effects, shortcuts,
  and sidebar layout.

## File Operations

BExplorer supports the common file-management operations expected from a modern
explorer:

- copy, cut, paste, move, rename, delete, and permanent delete;
- create folders and text documents;
- drag files inside BExplorer and between BExplorer and Windows;
- copy from and to Windows Explorer through the system clipboard;
- copy, paste, and delete files on supported MTP devices;
- copy and move across local drives, removable drives, and UNC shares;
- retry supported operations through UAC when Windows denies access.

Conflict resolution is available for copy and move operations:

- `Replace`: overwrite the existing destination item.
- `Skip`: leave the destination item untouched.
- `Keep both`: copy the new item using a numbered name such as
  `report (2).txt`.

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
| Local file browsing | Supported | Initial | Planned |
| Tabs and split view | Supported | Initial | Planned |
| File transfers | Supported | Initial | Planned |
| Archive browsing/extraction | Supported | Initial | Planned |
| Native icons/thumbnails | Supported | Initial | Planned |
| MTP portable devices | Supported | Mounted GVfs/FUSE devices | Stub |
| Network discovery | Supported | Best effort | Planned |
| ISO mount/eject | Supported | Initial via UDisks2 | Planned |
| Acrylic/Mica/blur effects | Supported | N/A | N/A |
| Windows Defender scan | Supported | N/A | N/A |

The platform facade lives in `src/platform/mod.rs`. Windows-specific logic is
kept under `src/platform/windows/` and related Windows shell helpers. Linux
support uses `/proc/self/mountinfo`, sysfs, `xdg-terminal-exec`/common terminal
fallbacks, and a Freedesktop `.desktop` entry without binding the application to
one desktop environment. Linux windowing is handled by `iced`/`winit`, so the
application can run under Wayland or X11 when the required runtime libraries are
available.

## Interface Architecture

The `iced` interface is split by responsibility under `src/iced_ui`:

- `mod.rs` owns application state, messages, subscriptions, and window setup;
- `update.rs` routes messages and asynchronous operation results;
- `interaction.rs`, `navigation.rs`, and `search_state.rs` own input, movement,
  selection, drag-and-drop, and search state;
- `view.rs` and `view/` build window chrome, file surfaces, and dialogs;
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
Disk image mount/eject uses UDisks2 through `udisksctl` when available, elevated
retry uses Polkit through `pkexec`, and network discovery uses available
Freedesktop/GVfs, Avahi, and Samba command-line helpers.

KDE Plasma/Wayland uses KWin's optional native blur protocol. GNOME/Mutter does
not expose its Shell blur actor as a Wayland client protocol, so BExplorer uses
the optional [Blur My Shell](https://extensions.gnome.org/extension/3193/blur-my-shell/)
extension for GNOME application blur. Selecting Blur registers the `bexplorer`
application ID with that extension; disabling it removes BExplorer's entry
again. If the extension is unavailable, BExplorer keeps an opaque readable
background.

## Known Limitations

- Linux support is initial and not yet at Windows feature parity.
- Direct WPD/MTP sessions and Microsoft Defender are Windows-only. Linux exposes
  portable devices mounted by GVfs as regular storage paths.
- macOS has initial storage, disk-image, eject, and mounted-SMB adapters but
  still needs broader runtime testing and native drag-and-drop integration.
- Browsing an unmounted authenticated SMB share on Linux or macOS may still
  require connecting it through the desktop environment first.
- Linux file clipboard interoperability uses native MIME helpers when
  `wl-copy`/`wl-paste`, `xclip`, or `xsel` are installed, with a text fallback.
- Linux icon theme lookup is implemented in-process and may not yet cover every
  desktop-specific theme extension.
- Linux drag-out uses Wayland-compatible helper applications such as `ripdrag`,
  `dragon-drag-and-drop`, `dragon`, or `dragon-drop` when available.
  A custom helper can be selected with `BEXPLORER_DRAG_HELPER`.
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
- Public distribution still needs installer polish, signing, and clean-machine
  testing.

## Build Requirements

Windows requirements:

- Rust stable installed with `rustup`.
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

Linux requirements:

- Rust stable installed with `rustup`.
- A C/C++ toolchain usable by the `cc` crate, such as GCC, Clang, or Zig
  wrappers for `cc`, `c++`, `ar`, and `ranlib`.
- Usual desktop runtime libraries required by `iced`/`winit` on your
  distribution.

Optional Linux integrations:

- Blur My Shell for application blur on GNOME Wayland.
- `wl-clipboard`, `xclip`, or `xsel` for file clipboard MIME interoperability.
- `ripdrag`, `dragon-drag-and-drop`, `dragon`, or `dragon-drop` for native
  drag-out to other applications on Wayland.
- `udisks2` for ISO/USB mount and eject actions.
- `polkit` with `pkexec` for elevated retry.
- `xdg-utils` and GLib/GVfs (`gio`) for default app opening and mounted devices.
- Samba tools such as `smbclient`/`smbtree`, and optionally Avahi, for network
  discovery.
- GVfs MTP/FUSE support for mounted phone/camera devices.

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

For a simple internal beta installer, the installer can copy:

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

Linux packages can be staged with:

```bash
tools/package_linux.sh
```

The script creates a tarball under `dist/` and, when `dpkg-deb` is installed, a
basic `.deb` package with the desktop entry, app icon, metainfo, license, and
third-party notices.

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

## Project Layout

```text
src/
  main.rs
  app/
    state.rs
    state/
    config.rs
    session.rs
    commands.rs
  fs/
    archive.rs
    archive/
    archive_listing.rs
    explorer.rs
    operations.rs
    portable.rs
    search.rs
    transfer_queue.rs
  platform/
    mod.rs
    windows.rs
    windows/
    linux.rs
    macos.rs
    shell/
  preview/
  ui/
    action_bar.rs
    bookmarks_bar.rs
    dialogs.rs
    file_table.rs
    sidebar.rs
    status_bar.rs
    tabs.rs
    theme.rs
    window_frame.rs
  utils/
    errors.rs
    log.rs
    paths.rs
vendor/
  7zip-src/
  7zip-ffi/
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
- clipboard and paste shortcut behavior;
- Linux mountinfo parsing, MIME/file URI clipboard helpers, UDisks parsing,
  network helper parsing, and XDG thumbnail/icon metadata.

Run:

```bash
cargo test
```

## Roadmap

Near-term focus:

- internal beta hardening on Windows;
- installer and update flow;
- code signing;
- broader clean-machine testing;
- stronger network and MTP edge-case coverage;
- additional preview formats;
- continued separation of Windows/Linux/macOS platform layers.

Longer-term:

- Linux runtime testing, drag-out, deeper portal integration, and richer MTP;
- macOS file-management backend;
- platform-native preview/icon integrations outside Windows;
- optional plugin or extension points.

## License

BExplorer's own Rust and application code is licensed under the MIT License.

See [LICENSE](LICENSE) for details.

The embedded 7-Zip source under `vendor/7zip-src/` is distributed under its own
licenses. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) and the original
license files in `vendor/7zip-src/DOC/`.
