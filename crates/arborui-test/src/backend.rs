use std::{collections::VecDeque, error::Error, fmt, time::Duration};

use arborui_core::Size;
use arborui_render::FramePatch;
use arborui_terminal::{Capabilities, TerminalBackend, TerminalEvent, TerminalState, WriteOutcome};

use crate::TestFrame;

#[derive(Clone, Copy, Debug)]
pub(crate) enum ScriptedWrite {
    Outcome(WriteOutcome),
    Fail,
}

/// In-memory terminal output failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TestBackendError;

impl fmt::Display for TestBackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("test terminal output failure")
    }
}

impl Error for TestBackendError {}

pub(crate) struct MemoryBackend {
    size: Size,
    capabilities: Capabilities,
    frame: TestFrame,
    patches: Vec<FramePatch>,
    writes: VecDeque<ScriptedWrite>,
}

impl MemoryBackend {
    pub(crate) fn new(size: Size, capabilities: Capabilities) -> Self {
        Self {
            size,
            capabilities,
            frame: TestFrame::new(size),
            patches: Vec::new(),
            writes: VecDeque::new(),
        }
    }

    pub(crate) const fn frame(&self) -> &TestFrame {
        &self.frame
    }

    pub(crate) fn patches(&self) -> &[FramePatch] {
        &self.patches
    }

    pub(crate) fn set_size(&mut self, size: Size) {
        self.size = size;
    }

    pub(crate) fn sync_committed_size(&mut self) {
        if self.frame.size() != self.size {
            self.frame = TestFrame::new(self.size);
        }
    }

    pub(crate) fn script(&mut self, write: ScriptedWrite) {
        self.writes.push_back(write);
    }
}

impl TerminalBackend for MemoryBackend {
    type Error = TestBackendError;

    fn size(&self) -> Result<Size, Self::Error> {
        Ok(self.size)
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn poll_event(&mut self, _timeout: Duration) -> Result<Option<TerminalEvent>, Self::Error> {
        Ok(None)
    }

    fn apply_state(&mut self, _desired: &TerminalState) -> Result<(), Self::Error> {
        Ok(())
    }

    fn write_patch(&mut self, patch: &FramePatch) -> Result<WriteOutcome, Self::Error> {
        patch
            .validate_for_width_policy(self.capabilities.width_policy)
            .map_err(|_| TestBackendError)?;
        self.patches.push(patch.clone());
        match self
            .writes
            .pop_front()
            .unwrap_or(ScriptedWrite::Outcome(WriteOutcome::Applied))
        {
            ScriptedWrite::Outcome(WriteOutcome::Applied) => {
                self.frame.apply(patch);
                Ok(WriteOutcome::Applied)
            }
            ScriptedWrite::Outcome(outcome) => Ok(outcome),
            ScriptedWrite::Fail => Err(TestBackendError),
        }
    }

    fn restore(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arborui_core::{CursorState, Point, Style};
    use arborui_render::{PatchCellContent, Renderer};
    use arborui_text::WidthPolicy;

    use super::*;

    #[test]
    fn rejects_invalid_text_without_changing_the_committed_frame()
    -> Result<(), Box<dyn std::error::Error>> {
        let size = Size::new(1, 1);
        let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
        let prepared = renderer.prepare(size, CursorState::HIDDEN, |canvas| {
            canvas.draw_text(Point::ORIGIN, "x", Style::default(), None)?;
            Ok(())
        })?;
        let mut patch = prepared.patch().clone();
        let PatchCellContent::Grapheme { text, .. } = &mut patch.runs[0].cells[0].content else {
            return Err("test patch must contain a grapheme".into());
        };
        *text = Arc::from("\u{2028}");
        let capabilities = Capabilities {
            width_policy: WidthPolicy::Unicode,
            ..Capabilities::default()
        };
        let mut backend = MemoryBackend::new(size, capabilities);
        let committed = backend.frame().clone();

        assert_eq!(backend.write_patch(&patch), Err(TestBackendError));
        assert_eq!(backend.frame(), &committed);
        Ok(())
    }
}
