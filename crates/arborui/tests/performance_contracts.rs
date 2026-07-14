//! Deterministic output-shape contracts that complement timing benchmarks.

use arborui::{CursorState, Point, Size, Style, WidthPolicy, render::Renderer};

#[test]
fn full_repaint_has_one_complete_run_per_row() -> Result<(), Box<dyn std::error::Error>> {
    let size = Size::new(80, 24);
    let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
    let frame = renderer.prepare(size, CursorState::HIDDEN, |_| Ok(()))?;

    assert!(frame.patch().full_repaint);
    assert_eq!(frame.patch().runs.len(), usize::from(size.height));
    assert!(
        frame
            .patch()
            .runs
            .iter()
            .all(|run| run.cells.len() == usize::from(size.width))
    );
    Ok(())
}

#[test]
fn one_cell_change_emits_one_cell() -> Result<(), Box<dyn std::error::Error>> {
    let size = Size::new(80, 24);
    let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
    let initial = renderer.prepare(size, CursorState::HIDDEN, |_| Ok(()))?;
    renderer.commit(initial)?;

    let frame = renderer.prepare(size, CursorState::HIDDEN, |canvas| {
        canvas.draw_text(Point::new(40, 12), "x", Style::default(), None)?;
        Ok(())
    })?;

    assert!(!frame.patch().full_repaint);
    assert_eq!(frame.patch().runs.len(), 1);
    assert_eq!(frame.patch().runs[0].cells.len(), 1);
    Ok(())
}
