use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

/// A UTF-8 byte offset into a [`TextBuffer`].
///
/// Values returned by `TextBuffer` always identify an extended-grapheme
/// boundary.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ByteOffset(usize);

impl ByteOffset {
    /// Returns the underlying UTF-8 byte offset.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// A non-empty selection represented by its fixed anchor and moving focus.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Selection {
    anchor: ByteOffset,
    focus: ByteOffset,
}

impl Selection {
    /// Returns the fixed endpoint of the selection.
    #[must_use]
    pub const fn anchor(self) -> ByteOffset {
        self.anchor
    }

    /// Returns the endpoint that follows cursor movement.
    #[must_use]
    pub const fn focus(self) -> ByteOffset {
        self.focus
    }

    /// Returns the earlier endpoint in the text.
    #[must_use]
    pub fn start(self) -> ByteOffset {
        self.anchor.min(self.focus)
    }

    /// Returns the later endpoint in the text.
    #[must_use]
    pub fn end(self) -> ByteOffset {
        self.anchor.max(self.focus)
    }

    /// Returns the selected UTF-8 byte range in document order.
    #[must_use]
    pub fn byte_range(self) -> Range<usize> {
        self.start().get()..self.end().get()
    }
}

/// A cursor destination within a single-line text buffer.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TextMovement {
    /// Move to the preceding extended-grapheme boundary.
    Left,
    /// Move to the following extended-grapheme boundary.
    Right,
    /// Move to the start of the text.
    Home,
    /// Move to the end of the text.
    End,
}

/// One editing command for a controlled single-line text input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextEdit<'a> {
    /// Insert text at the cursor, replacing the selection.
    ///
    /// Carriage returns and line feeds are omitted to preserve the single-line
    /// invariant.
    Insert(&'a str),
    /// Delete the selection or the grapheme preceding the cursor.
    Backspace,
    /// Delete the selection or the grapheme following the cursor.
    Delete,
    /// Move the cursor, optionally extending the selection from its anchor.
    Move {
        /// Cursor destination.
        movement: TextMovement,
        /// Whether to preserve the current anchor and extend the selection.
        extend_selection: bool,
    },
    /// Select all text.
    SelectAll,
}

/// Owned, grapheme-aware state for a controlled single-line text input.
///
/// Cursor and selection locations are UTF-8 byte offsets, but every exposed
/// endpoint is guaranteed to be an extended-grapheme boundary. The buffer is
/// intentionally limited to basic single-line editing and does not provide
/// history, word movement, or rich-text state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TextBuffer {
    text: String,
    cursor: usize,
    anchor: Option<usize>,
}

impl TextBuffer {
    /// Creates a buffer with the cursor at the end of `text`.
    ///
    /// Carriage returns and line feeds are omitted, and tabs become spaces.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        let text = sanitize_single_line(text.into());
        let cursor = text.len();
        Self {
            text,
            cursor,
            anchor: None,
        }
    }

    /// Returns the buffer contents.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the cursor's UTF-8 byte offset.
    #[must_use]
    pub const fn cursor(&self) -> ByteOffset {
        ByteOffset(self.cursor)
    }

    /// Returns the current non-empty selection.
    #[must_use]
    pub fn selection(&self) -> Option<Selection> {
        let anchor = self.anchor?;
        if anchor == self.cursor {
            return None;
        }

        Some(Selection {
            anchor: ByteOffset(anchor),
            focus: ByteOffset(self.cursor),
        })
    }

    /// Applies one editing command.
    pub fn apply(&mut self, edit: TextEdit<'_>) {
        match edit {
            TextEdit::Insert(text) => self.insert(text),
            TextEdit::Backspace => self.backspace(),
            TextEdit::Delete => self.delete(),
            TextEdit::Move {
                movement,
                extend_selection,
            } => self.move_cursor(movement, extend_selection),
            TextEdit::SelectAll => {
                self.anchor = Some(0);
                self.cursor = self.text.len();
            }
        }
    }

    fn insert(&mut self, text: &str) {
        let inserted = sanitize_single_line(text.to_owned());
        let range = self
            .selection()
            .map_or(self.cursor..self.cursor, Selection::byte_range);
        if inserted.is_empty() && range.is_empty() {
            return;
        }
        let requested_cursor = range.start + inserted.len();
        self.text.replace_range(range, &inserted);
        self.cursor = boundary_at_or_after(&self.text, requested_cursor);
        self.anchor = None;
    }

    fn backspace(&mut self) {
        if let Some(selection) = self.selection() {
            self.delete_range(selection.byte_range());
        } else if let Some(start) = boundary_before(&self.text, self.cursor) {
            self.delete_range(start..self.cursor);
        }
    }

    fn delete(&mut self) {
        if let Some(selection) = self.selection() {
            self.delete_range(selection.byte_range());
        } else if let Some(end) = boundary_after(&self.text, self.cursor) {
            self.delete_range(self.cursor..end);
        }
    }

    fn delete_range(&mut self, range: Range<usize>) {
        let requested_cursor = range.start;
        self.text.replace_range(range, "");
        self.cursor = boundary_at_or_after(&self.text, requested_cursor);
        self.anchor = None;
    }

    fn move_cursor(&mut self, movement: TextMovement, extend_selection: bool) {
        let selection = self.selection();
        let destination = match movement {
            TextMovement::Left if !extend_selection && selection.is_some() => {
                selection.map_or(self.cursor, |selection| selection.start().get())
            }
            TextMovement::Right if !extend_selection && selection.is_some() => {
                selection.map_or(self.cursor, |selection| selection.end().get())
            }
            TextMovement::Left => {
                boundary_before(&self.text, self.cursor).map_or(self.cursor, |boundary| boundary)
            }
            TextMovement::Right => {
                boundary_after(&self.text, self.cursor).map_or(self.cursor, |boundary| boundary)
            }
            TextMovement::Home => 0,
            TextMovement::End => self.text.len(),
        };

        if extend_selection {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = destination;
    }
}

impl From<String> for TextBuffer {
    fn from(text: String) -> Self {
        Self::new(text)
    }
}

impl From<&str> for TextBuffer {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

fn sanitize_single_line(text: String) -> String {
    if text.contains(['\r', '\n', '\t']) {
        text.chars()
            .filter_map(|character| match character {
                '\r' | '\n' => None,
                '\t' => Some(' '),
                character => Some(character),
            })
            .collect()
    } else {
        text
    }
}

fn boundary_before(text: &str, offset: usize) -> Option<usize> {
    text.grapheme_indices(true)
        .map(|(boundary, _)| boundary)
        .rfind(|boundary| *boundary < offset)
}

fn boundary_after(text: &str, offset: usize) -> Option<usize> {
    text.grapheme_indices(true)
        .map(|(boundary, _)| boundary)
        .find(|boundary| *boundary > offset)
        .or_else(|| (offset < text.len()).then_some(text.len()))
}

fn boundary_at_or_after(text: &str, offset: usize) -> usize {
    text.grapheme_indices(true)
        .map(|(boundary, _)| boundary)
        .find(|boundary| *boundary >= offset)
        .map_or(text.len(), |boundary| boundary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn move_edit(movement: TextMovement, extend_selection: bool) -> TextEdit<'static> {
        TextEdit::Move {
            movement,
            extend_selection,
        }
    }

    #[test]
    fn combining_sequence_is_one_editing_unit() {
        let mut buffer = TextBuffer::new("a\u{301}b");

        buffer.apply(move_edit(TextMovement::Home, false));
        buffer.apply(move_edit(TextMovement::Right, false));
        assert_eq!(buffer.cursor().get(), "a\u{301}".len());

        buffer.apply(TextEdit::Backspace);
        assert_eq!(buffer.text(), "b");
        assert_eq!(buffer.cursor().get(), 0);
    }

    #[test]
    fn zwj_and_wide_graphemes_have_single_boundaries() {
        let emoji = "👩‍💻";
        let mut buffer = TextBuffer::new(format!("x{emoji}界"));

        buffer.apply(move_edit(TextMovement::Home, false));
        buffer.apply(move_edit(TextMovement::Right, false));
        assert_eq!(buffer.cursor().get(), 1);
        buffer.apply(move_edit(TextMovement::Right, false));
        assert_eq!(buffer.cursor().get(), 1 + emoji.len());

        buffer.apply(TextEdit::Backspace);
        assert_eq!(buffer.text(), "x界");
        assert_eq!(buffer.cursor().get(), 1);
    }

    #[test]
    fn insertion_replaces_a_unicode_selection() {
        let emoji = "👩‍💻";
        let mut buffer = TextBuffer::new(format!("a{emoji}b"));

        buffer.apply(move_edit(TextMovement::Home, false));
        buffer.apply(move_edit(TextMovement::Right, false));
        buffer.apply(move_edit(TextMovement::Right, true));

        assert_eq!(
            buffer.selection().map(Selection::byte_range),
            Some(1..1 + emoji.len())
        );
        buffer.apply(TextEdit::Insert("界"));

        assert_eq!(buffer.text(), "a界b");
        assert_eq!(buffer.cursor().get(), 1 + "界".len());
        assert_eq!(buffer.selection(), None);
    }

    #[test]
    fn deletion_resnaps_when_neighbors_form_one_grapheme() {
        let mut buffer = TextBuffer::new("🇦x🇧");

        buffer.apply(move_edit(TextMovement::Home, false));
        buffer.apply(move_edit(TextMovement::Right, false));
        buffer.apply(move_edit(TextMovement::Right, true));
        buffer.apply(TextEdit::Delete);

        assert_eq!(buffer.text(), "🇦🇧");
        assert_eq!(buffer.cursor().get(), "🇦🇧".len());
        assert_eq!(buffer.selection(), None);
    }

    #[test]
    fn insertion_resnaps_when_it_joins_neighboring_graphemes() {
        let mut buffer = TextBuffer::new("👩💻");

        buffer.apply(move_edit(TextMovement::Home, false));
        buffer.apply(move_edit(TextMovement::Right, false));
        buffer.apply(TextEdit::Insert("\u{200d}"));

        assert_eq!(buffer.text(), "👩‍💻");
        assert_eq!(buffer.cursor().get(), buffer.text().len());
    }

    #[test]
    fn movement_extension_and_selection_collapse_follow_direction() {
        let mut buffer = TextBuffer::new("a界c");

        buffer.apply(move_edit(TextMovement::Home, true));
        assert_eq!(
            buffer
                .selection()
                .map(|selection| (selection.anchor().get(), selection.focus().get())),
            Some((buffer.text().len(), 0))
        );

        buffer.apply(move_edit(TextMovement::Right, false));
        assert_eq!(buffer.cursor().get(), buffer.text().len());
        assert_eq!(buffer.selection(), None);

        buffer.apply(TextEdit::SelectAll);
        buffer.apply(move_edit(TextMovement::Left, false));
        assert_eq!(buffer.cursor().get(), 0);
        assert_eq!(buffer.selection(), None);
    }

    #[test]
    fn boundary_commands_are_noops_at_text_edges() {
        let mut buffer = TextBuffer::new("");

        buffer.apply(TextEdit::Backspace);
        buffer.apply(TextEdit::Delete);
        buffer.apply(move_edit(TextMovement::Left, false));
        buffer.apply(move_edit(TextMovement::Right, true));

        assert_eq!(buffer.text(), "");
        assert_eq!(buffer.cursor().get(), 0);
        assert_eq!(buffer.selection(), None);
    }

    #[test]
    fn inserted_and_initial_line_breaks_are_omitted() {
        let mut buffer = TextBuffer::new("a\r\n\tb");
        assert_eq!(buffer.text(), "a b");

        buffer.apply(TextEdit::Insert("c\n\rd"));
        assert_eq!(buffer.text(), "a bcd");
        assert_eq!(buffer.cursor().get(), 5);
    }

    #[test]
    fn empty_insertion_deletes_the_selection() {
        let mut buffer = TextBuffer::new("abc");
        buffer.apply(TextEdit::SelectAll);
        buffer.apply(TextEdit::Insert(""));

        assert_eq!(buffer.text(), "");
        assert_eq!(buffer.selection(), None);
    }
}
