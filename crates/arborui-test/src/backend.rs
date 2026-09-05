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
    record_patches: bool,
    writes: VecDeque<ScriptedWrite>,
}

impl MemoryBackend {
    pub(crate) fn new(size: Size, capabilities: Capabilities, record_patches: bool) -> Self {
        Self {
            size,
            capabilities,
            frame: TestFrame::new(size),
            patches: Vec::new(),
            record_patches,
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
        if self.record_patches {
            self.patches.push(patch.clone());
        }
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
    fn nonrecording_writes_do_not_retain_patch_history() -> Result<(), Box<dyn std::error::Error>> {
        let size = Size::new(1, 1);
        let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
        let prepared = renderer.prepare(size, CursorState::HIDDEN, |canvas| {
            canvas.draw_text(Point::ORIGIN, "x", Style::default(), None)?;
            Ok(())
        })?;
        let mut backend = MemoryBackend::new(size, Capabilities::default(), false);
        assert_eq!((backend.patches.len(), backend.patches.capacity()), (0, 0));
        for _ in 0..800 {
            assert_eq!(
                backend.write_patch(prepared.patch()),
                Ok(WriteOutcome::Applied)
            );
            assert_eq!((backend.patches.len(), backend.patches.capacity()), (0, 0));
        }
        assert_eq!(backend.frame().characters(), "x");
        assert_eq!((backend.patches.len(), backend.patches.capacity()), (0, 0));
        Ok(())
    }

    #[test]
    fn recording_only_changes_history_not_frames_or_write_outcomes()
    -> Result<(), Box<dyn std::error::Error>> {
        let size = Size::new(1, 1);
        let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
        let prepared = renderer.prepare(size, CursorState::HIDDEN, |canvas| {
            canvas.draw_text(Point::ORIGIN, "x", Style::default(), None)?;
            Ok(())
        })?;
        let mut recording = MemoryBackend::new(size, Capabilities::default(), true);
        let mut nonrecording = MemoryBackend::new(size, Capabilities::default(), false);
        for (index, (script, expected)) in [
            (
                ScriptedWrite::Outcome(WriteOutcome::Deferred),
                Ok(WriteOutcome::Deferred),
            ),
            (
                ScriptedWrite::Outcome(WriteOutcome::StateUnknown),
                Ok(WriteOutcome::StateUnknown),
            ),
            (ScriptedWrite::Fail, Err(TestBackendError)),
            (
                ScriptedWrite::Outcome(WriteOutcome::Applied),
                Ok(WriteOutcome::Applied),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            for backend in [&mut recording, &mut nonrecording] {
                backend.script(script);
                assert_eq!(backend.write_patch(prepared.patch()), expected);
                assert!(backend.writes.is_empty());
                assert_eq!(
                    backend.frame().characters(),
                    if index == 3 { "x" } else { " " }
                );
            }
            assert_eq!(recording.frame(), nonrecording.frame());
            assert_eq!(recording.patches.len(), index + 1);
            assert_eq!(recording.patches.last(), Some(prepared.patch()));
            assert_eq!(
                (nonrecording.patches.len(), nonrecording.patches.capacity()),
                (0, 0)
            );
        }
        Ok(())
    }

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
        for record_patches in [true, false] {
            let mut backend = MemoryBackend::new(size, capabilities, record_patches);
            let committed = backend.frame().clone();
            backend.script(ScriptedWrite::Outcome(WriteOutcome::Deferred));

            assert_eq!(backend.write_patch(&patch), Err(TestBackendError));
            assert_eq!(backend.frame(), &committed);
            assert_eq!((backend.patches.len(), backend.patches.capacity()), (0, 0));
            assert_eq!(backend.writes.len(), 1);
            assert_eq!(
                backend.write_patch(prepared.patch()),
                Ok(WriteOutcome::Deferred)
            );
            assert!(backend.writes.is_empty());
            assert_eq!(backend.frame(), &committed);
        }
        Ok(())
    }
}
