# Fuzz Corpus

The fuzz targets use bounded bytecode so corpus entries remain meaningful when
internal Rust types change.

## Targets

- `text_edit_sequences` interprets a length-prefixed initial string followed by
  insert, delete, movement, and selection operations. Its seeds cover ASCII,
  combining sequences, CJK text, ZWJ emoji, and regional indicators.
- `render_transactions` interprets five-byte frame operations containing size,
  signed paint coordinates, text selection, commit or discard, invalidation,
  and width-policy changes. Its seeds cover zero-area frames, resize, clipping,
  and Unicode-wide content.

When a failure is fixed, minimize it with `cargo fuzz tmin`, retain the input in
the matching corpus directory, and add a descriptive regression test to the
owning crate.

## Render Transactions

Each operation is `[width, height, x, y, control]`. Width and height are reduced
modulo 33 and 17; coordinates are signed bytes. Missing bytes default to zero,
including the final partial operation from a seed's trailing newline.

- `control % 8` selects empty, ASCII, combining, CJK, ZWJ emoji, flag, multiline,
  or variation-selector heart text, in that order.
- `control & 0x08 == 0` commits; otherwise the frame is discarded. This bit is
  independent of the three text-selection bits.
- The existing invalidation (`0x02`) and width-policy (`0x04`, then `control % 3`)
  controls are unchanged.

The intentional seeds are raw bytecode, not strings to draw:

- `resize` retains its original discard/commit/discard/commit/commit sequence,
  including zero-area resizes, offscreen paint coordinates, invalidation, and
  the WcWidth switch. Its first and third control bytes changed from `A`/`C` to
  `I`/`K` to preserve the old transaction decisions without changing text.
- `unicode` retains its original discard/discard/commit/discard/discard/commit
  sequence and all geometry, text, invalidation, and policy choices. The `s` in
  its ASCII suffix changed to `{` to preserve the fifth operation's discard.
  The UTF-8 bytes decode to offscreen coordinates, not visible Unicode text.
- `fixtures` adds visible coverage: for each of the eight fixtures, commit it,
  discard another frame with that fixture, then commit empty text to clean up.
  Every full operation starts with `AA` and two literal tabs, decoding to a
  32x14 frame at (9, 9). Controls `@` through `G` commit and `H` through `O`
  discard; `@` also supplies each cleanup frame. The trailing newline adds a
  final zero-height committed frame.

The target's unit tests exhaust all 256 control bytes, check the legacy seed
decisions and visible fixture sequences, and replay all intentional seeds.
`just test` (also part of `just ci`) runs them with Rust 1.85.0 and the separate
fuzz lockfile. Sanitizer-enabled fuzz campaigns still use the pinned nightly.
