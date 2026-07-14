#![no_main]

use libfuzzer_sys::fuzz_target;
use arborui_core::{CursorState, Point, Size, Style};
use arborui_render::Renderer;
use arborui_text::WidthPolicy;

const TEXT: [&str; 8] = [
    "",
    "ascii",
    "a\u{301}",
    "\u{754c}",
    "\u{1f469}\u{200d}\u{1f4bb}",
    "\u{1f1e6}\u{1f1e7}",
    "line one\nline two",
    "\u{2764}\u{fe0f}",
];

fuzz_target!(|data: &[u8]| {
    let mut renderer = Renderer::new(Size::ZERO, WidthPolicy::Unicode);

    for operation in data.chunks(5).take(256) {
        let size = Size::new(
            u16::from(operation.first().copied().unwrap_or_default() % 33),
            u16::from(operation.get(1).copied().unwrap_or_default() % 17),
        );
        let x = i32::from(i8::from_ne_bytes([operation
            .get(2)
            .copied()
            .unwrap_or_default()]));
        let y = i32::from(i8::from_ne_bytes([operation
            .get(3)
            .copied()
            .unwrap_or_default()]));
        let control = operation.get(4).copied().unwrap_or_default();
        let text = TEXT[usize::from(control) % TEXT.len()];
        let committed_before = renderer.current().clone();
        let prepared = renderer
            .prepare(size, CursorState::HIDDEN, |canvas| {
                canvas.draw_text(Point::new(x, y), text, Style::default(), None)?;
                Ok(())
            })
            .expect("bounded static input must render");

        let mut replay = committed_before.clone();
        prepared
            .patch()
            .apply_to(&mut replay)
            .expect("renderer patches must replay");
        assert_eq!(&replay, prepared.buffer());

        if control & 1 == 0 {
            let expected = prepared.buffer().clone();
            renderer
                .commit(prepared)
                .expect("fresh prepared frame must commit");
            assert_eq!(renderer.current(), &expected);
        } else {
            renderer.discard(prepared);
            assert_eq!(renderer.current(), &committed_before);
        }

        if control & 2 != 0 {
            renderer.invalidate();
        }
        if control & 4 != 0 {
            renderer.set_width_policy(match control % 3 {
                0 => WidthPolicy::Unicode,
                1 => WidthPolicy::Cjk,
                _ => WidthPolicy::WcWidth,
            });
        }
    }
});
