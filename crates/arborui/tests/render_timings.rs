//! Facade-level contracts for opt-in application render timings.

use arborui::{
    AppRunner, Application, Capabilities, Command, Element, HeadlessRenderOutcome, Renderer, Size,
    TimedRender, UpdateContext, widgets::text,
};

struct TimingApp;

impl Application for TimingApp {
    type Message = ();

    fn update(
        &mut self,
        (): Self::Message,
        _context: &mut UpdateContext<Self::Message>,
    ) -> Command<Self::Message> {
        Command::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        text("timed")
    }
}

#[test]
fn facade_exposes_opt_in_render_timings() -> Result<(), Box<dyn std::error::Error>> {
    let size = Size::new(12, 2);
    let renderer = Renderer::new(size, Capabilities::default().width_policy);
    let mut runner = AppRunner::new(TimingApp, size, renderer);

    let TimedRender { outcome, timings } = runner.render_headless_timed()?;

    assert_eq!(outcome, HeadlessRenderOutcome::Committed);
    let timings = timings.expect("committed render must have timings");
    assert_eq!(timings.terminal_serialization_and_write, None);
    assert!(timings.commit.is_some());
    assert!(timings.post_commit.is_some());
    assert_eq!(timings.repaint_regions, 1);
    assert_eq!(timings.repaint_cells, size.area());
    assert_eq!(
        runner.render_headless_timed()?,
        TimedRender {
            outcome: HeadlessRenderOutcome::Idle,
            timings: None,
        }
    );
    Ok(())
}
