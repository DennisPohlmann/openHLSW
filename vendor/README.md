# Patched egui crates

openHLSW uses modified copies of two crates of [egui](https://github.com/emilk/egui) 0.37.0
for sharper text: sub-pixel ("ClearType") text rendering, which uses the red, green and blue
stripes of every screen pixel. They replace the crates.io versions through
`[patch.crates-io]` in `Cargo.toml`. Both are dual-licensed under MIT or Apache-2.0
(`LICENSE-MIT`, `LICENSE-APACHE` in each folder), copyright Emil Ernerfeldt and the egui
contributors. The examples and benchmarks of the published crates are left out.

All changes are marked with `openHLSW` in the code.

## epaint 0.37.0

- `src/text/font_tweak.rs`: `SubpixelOrder` (None / RGB / BGR) and the global switch
  `set_subpixel_order` / `subpixel_order` (exported from `epaint::text`).
- `src/text/font_face.rs`: `rasterize_lcd` draws a glyph outline at three times the
  horizontal resolution and spreads it over the three color stripes with FreeType's default
  LCD filter (weights 8, 77, 86, 77, 8). The atlas texel stores the coverage of each stripe
  in red, green and blue and the smallest of them as alpha.
- `src/image.rs`: `FontColorTransferFunction::to_atlas_color` applies the coverage curve to
  each channel of such texels.

## egui_glow 0.37.0

- `src/painter.rs`: on desktop OpenGL 3.3+ the shaders are compiled as GLSL 3.30 with
  dual-source blending (`ONE_MINUS_SRC1_COLOR`), so every color channel of the background
  is covered by its own amount. `SUBPIXEL_TEXT_SUPPORTED` (in `src/lib.rs`) tells the app
  whether this is available.
- `src/shader/fragment.glsl`: the second output is the per-channel blend factor. A texel
  whose color is above its alpha is sub-pixel coverage; everything else egui draws is
  premultiplied (color at most alpha) and blends exactly as before. Without dual-source
  blending (older drivers, OpenGL ES), sub-pixel texels are drawn as gray coverage.

## Updating egui

When egui is updated, copy the new versions of both crates from crates.io (or the egui
repository) into this folder, apply the changes above again and bump the versions in
`Cargo.toml`. Remove the `[patch.crates-io]` section to go back to the stock crates.
