# BExplorer iced_winit patch

This directory contains `iced_winit` 0.14.0 with one focused Linux fix.

Iced currently creates winit windows without reading the startup activation
token supplied by XDG-compliant launchers. On GNOME/Wayland this leaves startup
feedback active until the compositor times out, even though the application is
already responsive.

Before creating the first window, the patch:

1. reads the backend-appropriate activation token from the environment;
2. removes both activation variables so child processes cannot inherit them;
3. attaches the token to the winit window attributes.

The token is then consumed by winit using `xdg_activation_v1` on Wayland or the
startup-notification protocol on X11.

Upstream issue: <https://github.com/iced-rs/iced/issues/3317>

Remove this crate patch once an Iced release includes equivalent behavior.
