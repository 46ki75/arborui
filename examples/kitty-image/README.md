# Kitty Image Example

This facade-only example is a full-terminal image viewer with a selectable image
list and an aspect-fitted preview that expands to the available terminal area.
It exercises native image replacement, resizing, movement, deletion, and
fallback behavior. The optional transparent overlay is composited into the
source before transfer because VS Code does not alpha-composite overlapping
native images.

Run it in a directly connected Kitty, Ghostty, WezTerm, or VS Code integrated
terminal session. VS Code requires `terminal.integrated.enableImages`:

With no path argument, the application loads images from `./images` relative to
the current working directory:

```console
cargo run -p arborui-example-kitty-image -- --auto
```

Pass a BMP, GIF, ICO, JPEG, PNG, PNM, QOI, TGA, TIFF, or WebP path to exercise
the optional decoder and display the first frame:

```console
cargo run -p arborui-example-kitty-image -- --auto photo.webp
```

Pass a directory to load its immediate supported image files in lexical path
order. Select an image with the list, arrow keys, or `p` and `n`:

```console
cargo run -p arborui-example-kitty-image -- --auto ./photos
```

Directory loading is bounded to 256 images and 256 MiB of decoded RGBA pixels.
Subdirectories and files with unsupported extensions are ignored; a supported
file that fails to decode stops startup with its path in the error.

Input is detected from its contents rather than its extension. Decoded images
are normalized to oriented sRGB RGBA and retain the renderer's 64 MiB decoded
payload limit. Sources retain their decoded pixel resolution for terminal
transfer; only the cell placement is fitted to the preview area. SVG and other
vector formats are not supported. When the terminal or PTY reports drawable
pixel dimensions, the demo uses the measured cell aspect ratio to avoid forced
stretching. Otherwise, it falls back to cells twice as tall as they are wide.

If conservative environment detection does not recognize a compatible
terminal, explicitly enable output:

```console
cargo run -p arborui-example-kitty-image -- --kitty
```

Only use `--kitty` with a terminal known to implement the Kitty graphics
protocol. Use `--no-kitty` to confirm the text fallback.

Use Up, Down, Left, Right, `p`, or `n` to change the selected image; Home and End
jump to the first and last image. Click a visible list row to select it. Press
`o` to remove or restore the transparent overlay, `m` to move the placement,
`h` to delete or restore the placement, and `q` or Escape to quit. The controls
are also keyboard- and mouse-operable.

Resizing the terminal refits and centers the preview while preserving the
source aspect ratio. The selected row remains visible as the list window moves
through large directories.
