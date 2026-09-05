#![cfg_attr(not(test), no_main)]

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

fn decode_control(control: u8) -> (usize, bool) {
    // The low three bits select text, so the transaction decision uses bit 3.
    (usize::from(control) % TEXT.len(), control & 0x08 == 0)
}

#[cfg(not(test))]
libfuzzer_sys::fuzz_target!(|data: &[u8]| render_transactions(data));

fn render_transactions(data: &[u8]) {
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
        let (text_index, commit) = decode_control(control);
        let text = TEXT[text_index];
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

        if commit {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_text_fixture_can_commit_and_discard() {
        let mut commits = [0; TEXT.len()];
        let mut discards = [0; TEXT.len()];
        for control in u8::MIN..=u8::MAX {
            let (text_index, commit) = decode_control(control);
            if commit {
                commits[text_index] += 1;
            } else {
                discards[text_index] += 1;
            }
        }
        println!("commits: {commits:?}; discards: {discards:?}");
        assert_eq!(commits, [16; TEXT.len()]);
        assert_eq!(discards, [16; TEXT.len()]);
    }

    #[test]
    fn legacy_seed_decisions_are_preserved() {
        for (seed, expected) in [
            (
                include_bytes!("../corpus/render_transactions/resize").as_slice(),
                [(1, false), (2, true), (3, false), (4, true), (0, true)].as_slice(),
            ),
            (
                include_bytes!("../corpus/render_transactions/unicode").as_slice(),
                [
                    (7, false),
                    (5, false),
                    (0, true),
                    (7, false),
                    (3, false),
                    (0, true),
                ]
                .as_slice(),
            ),
        ] {
            let decisions: Vec<_> = seed
                .chunks(5)
                .map(|operation| decode_control(operation.get(4).copied().unwrap_or_default()))
                .collect();
            assert_eq!(decisions, expected);
            render_transactions(seed);
        }
    }

    #[test]
    fn fixtures_seed_commits_discards_and_cleans_up_every_fixture() {
        let seed = include_bytes!("../corpus/render_transactions/fixtures");
        let mut sequences = seed.chunks_exact(15);
        assert_eq!(sequences.len(), TEXT.len());
        for (text_index, sequence) in sequences.by_ref().enumerate() {
            for (operation, expected) in
                sequence
                    .chunks_exact(5)
                    .zip([(text_index, true), (text_index, false), (0, true)])
            {
                // 32x14 at (9, 9) leaves every fixture visible, including both lines.
                assert_eq!(&operation[..4], b"AA\t\t");
                assert_eq!(decode_control(operation[4]), expected);
            }
        }
        assert_eq!(sequences.remainder(), b"\n");
        render_transactions(seed);
    }
}
