# Kitty Image Example

This facade-only example accepts an optional encoded raster image for its base
layer and otherwise generates three decoded RGBA sources in memory. It
exercises native image replacement, transparent layering, movement, deletion,
and fallback behavior.

Run it in a directly connected Kitty, Ghostty, WezTerm, or VS Code integrated
terminal session. VS Code requires `terminal.integrated.enableImages`:

```console
cargo run -p arborui-example-kitty-image -- --auto
```

Pass a BMP, GIF, ICO, JPEG, PNG, PNM, QOI, TGA, TIFF, or WebP path to exercise
the optional decoder and display the first frame:

```console
cargo run -p arborui-example-kitty-image -- --auto photo.webp
```

Input is detected from its contents rather than its extension. Decoded images
are normalized to oriented sRGB RGBA and retain the renderer's 64 MiB decoded
payload limit. SVG and other vector formats are not supported.

If conservative environment detection does not recognize a compatible
terminal, explicitly enable output:

```console
cargo run -p arborui-example-kitty-image -- --kitty
```

Only use `--kitty` with a terminal known to implement the Kitty graphics
protocol. Use `--no-kitty` to confirm the text fallback.

Press `p` to replace the base image, `o` to remove or restore the transparent
overlay, `m` to move the placements, `h` to delete or restore all placements,
and `q` or Escape to quit. The controls are also keyboard- and mouse-operable.

Shrink the terminal below the complete image rectangle to check clipping. A
partially clipped image intentionally falls back to text; restoring the window
should restore native output.
