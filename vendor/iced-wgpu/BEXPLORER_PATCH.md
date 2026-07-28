# BExplorer iced_wgpu patch

BExplorer keeps two focused Wayland transparency changes on top of
`iced_wgpu` 0.14.0:

- Prefer an 8-bit RGBA/BGRA surface when no sRGB format is advertised instead
  of accepting the compositor's first format. Newer KWin can advertise
  `Rgb10a2Unorm` first; its 2-bit alpha channel reduces transparency to four
  visible levels and causes abrupt slider jumps.
- Prefer `CompositeAlphaMode::PreMultiplied` when a surface advertises
  premultiplied and postmultiplied alpha. Iced's shaders and render pipelines
  already produce a premultiplied framebuffer, while newer KWin versions expose
  different alpha capabilities. Matching the surface interpretation prevents
  washed-out translucency on Wayland.

Remove the `[patch.crates-io]` entry and this vendored crate once upstream
selects a surface format with adequate alpha precision and an alpha mode that
matches the renderer output.
