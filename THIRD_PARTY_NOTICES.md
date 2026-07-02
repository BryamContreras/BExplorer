# Third-Party Notices

This file summarizes third-party components that are bundled with, built into,
or materially used by BExplorer releases.

It is not a replacement for the original license files. When distributing
BExplorer binaries, include this file together with the relevant original
license texts.

## 7-Zip

BExplorer includes an embedded 7-Zip engine used for archive browsing,
compression, extraction, and password-protected archive workflows.

Local source path:

```text
vendor/7zip-src/
vendor/7zip-ffi/
```

Original project:

```text
https://www.7-zip.org/
```

License summary:

- Most 7-Zip source files are licensed under the GNU LGPL.
- Some files are licensed under BSD 2-clause or BSD 3-clause terms.
- RAR decompression support includes the unRAR license restriction.

Important local license files:

```text
vendor/7zip-src/DOC/License.txt
vendor/7zip-src/DOC/copying.txt
vendor/7zip-src/DOC/unRarLicense.txt
vendor/7zip-src/DOC/readme.txt
```

The unRAR restriction applies to code used for RAR decompression. In practical
terms, the unRAR sources must not be used to recreate the proprietary RAR
compression algorithm.

For exact terms, rely on the original files listed above.

## BExplorer 7-Zip FFI glue

The C++ glue code in `vendor/7zip-ffi/` was written for BExplorer to call the
embedded 7-Zip engine from Rust. Unless a file says otherwise, that glue code is
licensed under the same MIT license as BExplorer's own application code.
