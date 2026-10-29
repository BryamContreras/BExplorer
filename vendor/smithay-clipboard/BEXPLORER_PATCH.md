# BExplorer Wayland file-drop patch

The clipboard already owns Iced's inbound Wayland data device. This patch
forwards completed `text/uri-list` and GNOME file offers to BExplorer through
one process-wide queue and a bounded wake-up channel. The separate native drag
bridge supplies outbound drags only, avoiding a second inbound transfer queue.

File offers are flushed, made nonblocking, and read by calloop readiness
callbacks. A five-second timer removes an unfinished offer; an eight-MiB cap
rejects oversized URI data. Neither a stalled sender nor an oversized offer can
block the clipboard worker, its shutdown command, or subsequent clipboard
requests. Unix filename bytes are preserved when decoding URI escapes.

After fetching the application's locked dependencies, run
`sh scripts/linux/test-vendored.sh` for the clipboard notification, stalled-pipe,
size-limit, and URI-decoding regressions. It tests a temporary copy with the
application's resolved dependencies and leaves checkout lockfiles untouched.
