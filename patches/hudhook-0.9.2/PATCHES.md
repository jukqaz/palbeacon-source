# Local hudhook 0.9.2 patch

This directory contains the minimum source, build files, and license material
needed to patch the crates.io `hudhook` 0.9.2 release.

## PalBeacon patch

The DX12 texture uploader originally created a texture in `COPY_DEST`, moved it
to `PIXEL_SHADER_RESOURCE` after the first upload, and then copied into it again
without moving it back to `COPY_DEST` on subsequent `replace_texture` calls.

The local change explicitly tracks the stable texture state and records this
transition sequence for every upload:

1. `PIXEL_SHADER_RESOURCE -> COPY_DEST` when replacing an existing texture.
2. Copy the upload buffer into the texture.
3. `COPY_DEST -> PIXEL_SHADER_RESOURCE` before rendering.

The initial upload starts in `COPY_DEST`, so it skips only the first transition.
Uploads use the renderer's own direct command queue instead of an independent
queue. This orders a replacement after any in-flight frame that still samples
the texture and before the next frame consumes the updated contents. The
texture API and all non-DX12 rendering backends are unchanged.

The patch also adds an opt-in `InputKeyboardExceptEscape` message-filter bit.
PalBeacon uses it only while the expanded map is interactive: search keystrokes
stay in the overlay, while Escape still reaches Palworld so the game remains in
control of its menu and cursor state.

Upstream release commit: `f5ea4079415e3720d342b5d97a3454cad893f3e2`.
