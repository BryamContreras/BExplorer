# BExplorer

![Rust 2024](https://img.shields.io/badge/Rust-2024-orange)
![Platform](https://img.shields.io/badge/platform-Windows-blue)
![Status](https://img.shields.io/badge/status-beta-yellow)
![License](https://img.shields.io/badge/license-MIT-green)

BExplorer is a Windows-first desktop file explorer written in Rust with
`eframe/egui`. It focuses on fast file management, split-pane workflows,
archive handling, preview support, and a modern compact interface without
turning into a heavy system shell replacement.

The project is designed so platform-specific code stays isolated. Windows is
currently the primary target, while Linux and macOS support are kept behind
separate platform modules for future work.

## Status

BExplorer is in active beta development.

It is already usable for day-to-day file management on Windows, but public
distribution still needs broader testing, a signed installer, and a wider
compatibility matrix across clean Windows installs, network environments, USB
drives, MTP devices, and permission-restricted folders.

## Highlights

- Native Rust desktop application using `eframe/egui`.
- Tabbed navigation with independent history and session restore.
- Split-pane mode with independent per-pane view state.
- Resizable and reorderable sidebar.
- Optional action bar and bookmark bar.
- Details, list, icons, large icons, extra-large icons, and tile views.
- Local drives, removable drives, mounted ISO images, UNC paths, network
  locations, and Windows MTP portable devices.
- Progressive network discovery with cached results.
- Windows Explorer-compatible file clipboard for regular file paths.
- Internal and external drag-and-drop support.
- Transfer queue with progress, pause, cancel, cleanup of partial files, and
  conflict handling.
- Elevated retry flow for operations that require administrator permission.
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
- extract ZIP, 7z, RAR, ISO, TAR, and other formats supported by 7-Zip;
- extract password-protected archives when a password is provided;
- browse common archive formats as folders;
- extract selected archive entries into normal folders;
- search inside supported archives during complete search.

On Windows, the 7-Zip engine is built from `vendor/7zip-src` and linked through
`vendor/7zip-ffi`. BExplorer does not ship or execute an external `7zr.exe`.

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
| Local file browsing | Supported | Planned | Planned |
| Tabs and split view | Supported | Planned | Planned |
| File transfers | Supported | Planned | Planned |
| Archive browsing/extraction | Supported | Planned | Planned |
| Native icons/thumbnails | Supported | Planned | Planned |
| MTP portable devices | Supported | Stub | Stub |
| Network discovery | Supported | Planned | Planned |
| ISO mount/eject | Supported | Planned | Planned |
| Acrylic/Mica/blur effects | Supported | N/A | N/A |
| Windows Defender scan | Supported | N/A | N/A |

The platform facade lives in `src/platform/mod.rs`. Windows-specific logic is
kept under `src/platform/windows/` and related Windows shell helpers, so future
Linux and macOS implementations can be added without mixing OS-specific logic
into the core application state.

## Known Limitations

- Linux and macOS support are not implemented yet.
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
- clipboard and paste shortcut behavior.

Run:

```powershell
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

- Linux file-management backend;
- macOS file-management backend;
- platform-native preview/icon integrations outside Windows;
- optional plugin or extension points.

## License

BExplorer's own Rust and application code is licensed under the MIT License.

See [LICENSE](LICENSE) for details.

The embedded 7-Zip source under `vendor/7zip-src/` is distributed under its own
licenses. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) and the original
license files in `vendor/7zip-src/DOC/`.
