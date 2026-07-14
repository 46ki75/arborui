#![no_main]

use libfuzzer_sys::fuzz_target;
use unicode_segmentation::UnicodeSegmentation;
use arborui_text::{TextBuffer, TextEdit, TextMovement, WidthPolicy, measure};

fuzz_target!(|data: &[u8]| {
    let mut input = Input::new(data);
    let initial = input.string(64);
    let mut buffer = TextBuffer::new(initial);

    for _ in 0..256 {
        let Some(operation) = input.byte() else {
            break;
        };
        match operation % 8 {
            0 => {
                let inserted = input.string(64);
                buffer.apply(TextEdit::Insert(&inserted));
            }
            1 => buffer.apply(TextEdit::Backspace),
            2 => buffer.apply(TextEdit::Delete),
            3 => buffer.apply(TextEdit::SelectAll),
            movement => buffer.apply(TextEdit::Move {
                movement: match movement % 4 {
                    0 => TextMovement::Left,
                    1 => TextMovement::Right,
                    2 => TextMovement::Home,
                    _ => TextMovement::End,
                },
                extend_selection: operation & 0x80 != 0,
            }),
        }
        assert_invariants(&buffer);
    }
});

fn assert_invariants(buffer: &TextBuffer) {
    let text = buffer.text();
    let cursor = buffer.cursor().get();
    assert!(cursor <= text.len());
    assert!(text.is_char_boundary(cursor));
    assert!(is_grapheme_boundary(text, cursor));
    assert!(!text.contains(['\r', '\n', '\t']));

    if let Some(selection) = buffer.selection() {
        for endpoint in [selection.anchor().get(), selection.focus().get()] {
            assert!(endpoint <= text.len());
            assert!(text.is_char_boundary(endpoint));
            assert!(is_grapheme_boundary(text, endpoint));
        }
        assert!(!selection.byte_range().is_empty());
    }

    for policy in [WidthPolicy::Unicode, WidthPolicy::Cjk, WidthPolicy::WcWidth] {
        let _ = measure(text, policy);
    }
}

fn is_grapheme_boundary(text: &str, offset: usize) -> bool {
    offset == text.len()
        || UnicodeSegmentation::grapheme_indices(text, true).any(|(boundary, _)| boundary == offset)
}

struct Input<'a> {
    remaining: &'a [u8],
}

impl<'a> Input<'a> {
    fn new(remaining: &'a [u8]) -> Self {
        Self { remaining }
    }

    fn byte(&mut self) -> Option<u8> {
        let (&byte, remaining) = self.remaining.split_first()?;
        self.remaining = remaining;
        Some(byte)
    }

    fn string(&mut self, limit: usize) -> String {
        let Some(length) = self.byte() else {
            return String::new();
        };
        let length = (usize::from(length) % (limit + 1)).min(self.remaining.len());
        let (value, remaining) = self.remaining.split_at(length);
        self.remaining = remaining;
        String::from_utf8_lossy(value).into_owned()
    }
}
