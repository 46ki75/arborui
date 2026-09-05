//! Production ANSI serialization measurements for matched application frames.

use std::{
    io::{self, Write},
    sync::{Arc, Mutex},
};

use arborui::{
    Application, Capabilities, CrosstermBackend as ArboruiCrosstermBackend, FramePatch,
    TerminalBackend,
};
use arborui_comparison_collection_lab_ratatui::{
    ComparisonAction, OVERLAY_RESIZE_STORM, RatatuiCollectionLab, RatatuiLogLab, RatatuiOverlayLab,
    RatatuiTableLab, RatatuiUnicodeLab, STANDARD_RESIZE_STORM, UNICODE_RESIZE_STORM,
    UNICODE_RESIZE_STORM_OFFSET, draw_test_log_terminal, draw_test_overlay_terminal,
    draw_test_table_terminal, draw_test_terminal,
};
use arborui_example_collection_lab::{
    CollectionLab, CollectionMode, LogAction, LogLab, Message, OverlayAction, OverlayLab,
    TableAction, TableLab, UnicodeAction, UnicodeLab,
};
use arborui_test::{KeyCode, Size as ArboruiSize, TestApp};
use ratatui::{
    Terminal,
    backend::{Backend, CrosstermBackend as RatatuiCrosstermBackend, TestBackend},
    buffer::Buffer,
    layout::Rect,
};

const ITEM_COUNT: usize = 100_000;
const WIDTH: u16 = 48;
const HEIGHT: u16 = 12;
const RESIZED_HEIGHT: u16 = 16;
const OVERLAY_WIDTH: u16 = 40;
const OVERLAY_HEIGHT: u16 = 12;
const RESIZED_OVERLAY_WIDTH: u16 = 44;
const RESIZED_OVERLAY_HEIGHT: u16 = 14;
const UNICODE_WIDTH: u16 = 36;
const UNICODE_HEIGHT: u16 = 10;
const NARROW_UNICODE_WIDTH: u16 = 30;
const SHIFT_BOUNDARY_OFFSET: usize = 15;

#[derive(Clone, Copy, Debug)]
enum Scenario {
    InitialRender,
    PageDown,
    End,
    Resize,
    Selection,
    Reverse,
    UnchangedRedraw,
}

#[derive(Clone, Copy, Debug)]
enum TableScenario {
    InitialRender,
    PageDown,
    Resize,
    Selection,
    VisibleUpdate,
    OffscreenUpdate,
}

#[derive(Clone, Copy, Debug)]
enum LogScenario {
    InitialRender,
    PageUp,
    Resize,
    AppendFollowing,
    AppendPaused,
}

#[derive(Clone, Copy, Debug)]
enum OverlayScenario {
    InitialRender,
    Open,
    FocusNext,
    Cancel,
    Confirm,
    BackgroundActivation,
    ResizeOpen,
}

#[derive(Clone, Copy, Debug)]
enum UnicodeScenario {
    InitialRender,
    ShiftBoundary,
    ReplaceWide,
    ResizeNarrow,
}

impl UnicodeScenario {
    const ALL: [Self; 4] = [
        Self::InitialRender,
        Self::ShiftBoundary,
        Self::ReplaceWide,
        Self::ResizeNarrow,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::InitialRender => "initial-render",
            Self::ShiftBoundary => "shift-boundary",
            Self::ReplaceWide => "replace-wide",
            Self::ResizeNarrow => "resize-narrow",
        }
    }
}

impl OverlayScenario {
    const ALL: [Self; 7] = [
        Self::InitialRender,
        Self::Open,
        Self::FocusNext,
        Self::Cancel,
        Self::Confirm,
        Self::BackgroundActivation,
        Self::ResizeOpen,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::InitialRender => "initial-render",
            Self::Open => "open",
            Self::FocusNext => "focus-next",
            Self::Cancel => "cancel",
            Self::Confirm => "confirm",
            Self::BackgroundActivation => "background-activation",
            Self::ResizeOpen => "resize-open",
        }
    }
}

impl LogScenario {
    const ALL: [Self; 5] = [
        Self::InitialRender,
        Self::PageUp,
        Self::Resize,
        Self::AppendFollowing,
        Self::AppendPaused,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::InitialRender => "initial-render",
            Self::PageUp => "page-up",
            Self::Resize => "resize",
            Self::AppendFollowing => "append-following",
            Self::AppendPaused => "append-paused",
        }
    }
}

impl TableScenario {
    const ALL: [Self; 6] = [
        Self::InitialRender,
        Self::PageDown,
        Self::Resize,
        Self::Selection,
        Self::VisibleUpdate,
        Self::OffscreenUpdate,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::InitialRender => "initial-render",
            Self::PageDown => "page-down",
            Self::Resize => "resize",
            Self::Selection => "selection",
            Self::VisibleUpdate => "visible-update",
            Self::OffscreenUpdate => "offscreen-update",
        }
    }
}

impl Scenario {
    const ALL: [Self; 7] = [
        Self::InitialRender,
        Self::PageDown,
        Self::End,
        Self::Resize,
        Self::Selection,
        Self::Reverse,
        Self::UnchangedRedraw,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::InitialRender => "initial-render",
            Self::PageDown => "page-down",
            Self::End => "end",
            Self::Resize => "resize",
            Self::Selection => "selection",
            Self::Reverse => "reverse",
            Self::UnchangedRedraw => "unchanged-redraw",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct OutputMetrics {
    bytes: usize,
    writes: usize,
    flushes: usize,
}

#[derive(Clone, Default)]
struct CountingWriter {
    metrics: Arc<Mutex<OutputMetrics>>,
}

impl CountingWriter {
    fn metrics(&self) -> OutputMetrics {
        *self.metrics.lock().expect("metrics lock must be available")
    }
}

impl Write for CountingWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let mut metrics = self
            .metrics
            .lock()
            .map_err(|_| io::Error::other("metrics lock poisoned"))?;
        metrics.bytes = metrics.bytes.saturating_add(buffer.len());
        metrics.writes = metrics.writes.saturating_add(1);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut metrics = self
            .metrics
            .lock()
            .map_err(|_| io::Error::other("metrics lock poisoned"))?;
        metrics.flushes = metrics.flushes.saturating_add(1);
        Ok(())
    }
}

#[test]
fn reports_production_ansi_output_metrics() {
    println!("| Mode | Scenario | Framework | ANSI bytes | Writer calls | Flushes |");
    println!("| --- | --- | --- | ---: | ---: | ---: |");

    for mode in [CollectionMode::Fixed, CollectionMode::Variable] {
        for scenario in Scenario::ALL {
            let arborui = arborui_metrics(mode, scenario);
            let ratatui = ratatui_metrics(mode, scenario);
            print_metrics(mode, scenario, "ArborUI", arborui);
            print_metrics(mode, scenario, "Ratatui", ratatui);

            if matches!(scenario, Scenario::UnchangedRedraw) {
                assert_eq!(arborui, OutputMetrics::default());
                assert!(ratatui.bytes > 0);
                assert_eq!(ratatui.flushes, 1);
            } else {
                assert!(arborui.bytes > 0);
                assert!(ratatui.bytes > 0);
                assert_eq!(arborui.flushes, 1);
                let expected_ratatui_flushes = if matches!(scenario, Scenario::Resize) {
                    2
                } else {
                    1
                };
                assert_eq!(ratatui.flushes, expected_ratatui_flushes);
            }
        }
    }
}

#[test]
fn reports_table_ansi_output_metrics() {
    println!("| Scenario | Framework | ANSI bytes | Writer calls | Flushes |");
    println!("| --- | --- | ---: | ---: | ---: |");

    for scenario in TableScenario::ALL {
        let arborui = arborui_table_metrics(scenario);
        let ratatui = ratatui_table_metrics(scenario);
        println!(
            "| {} | ArborUI | {} | {} | {} |",
            scenario.name(),
            arborui.bytes,
            arborui.writes,
            arborui.flushes
        );
        println!(
            "| {} | Ratatui | {} | {} | {} |",
            scenario.name(),
            ratatui.bytes,
            ratatui.writes,
            ratatui.flushes
        );

        if matches!(scenario, TableScenario::OffscreenUpdate) {
            assert_eq!(arborui, OutputMetrics::default());
        } else {
            assert!(arborui.bytes > 0);
            assert!(ratatui.bytes > 0);
        }
    }
}

#[test]
fn reports_scrolling_log_ansi_output_metrics() {
    println!("| Scenario | Framework | ANSI bytes | Writer calls | Flushes |");
    println!("| --- | --- | ---: | ---: | ---: |");

    for scenario in LogScenario::ALL {
        let arborui = arborui_log_metrics(scenario);
        let ratatui = ratatui_log_metrics(scenario);
        println!(
            "| {} | ArborUI | {} | {} | {} |",
            scenario.name(),
            arborui.bytes,
            arborui.writes,
            arborui.flushes
        );
        println!(
            "| {} | Ratatui | {} | {} | {} |",
            scenario.name(),
            ratatui.bytes,
            ratatui.writes,
            ratatui.flushes
        );

        if matches!(scenario, LogScenario::AppendPaused) {
            assert_eq!(arborui, OutputMetrics::default());
        } else {
            assert!(arborui.bytes > 0);
            assert!(ratatui.bytes > 0);
        }
    }
}

#[test]
fn reports_overlay_ansi_output_metrics() {
    println!("| Scenario | Framework | ANSI bytes | Writer calls | Flushes |");
    println!("| --- | --- | ---: | ---: | ---: |");

    for scenario in OverlayScenario::ALL {
        let arborui = arborui_overlay_metrics(scenario);
        let ratatui = ratatui_overlay_metrics(scenario);
        println!(
            "| {} | ArborUI | {} | {} | {} |",
            scenario.name(),
            arborui.bytes,
            arborui.writes,
            arborui.flushes
        );
        println!(
            "| {} | Ratatui | {} | {} | {} |",
            scenario.name(),
            ratatui.bytes,
            ratatui.writes,
            ratatui.flushes
        );

        if !matches!(scenario, OverlayScenario::BackgroundActivation) {
            assert!(arborui.bytes > 0);
        }
        assert!(ratatui.bytes > 0);
    }
}

#[test]
fn reports_unicode_ansi_output_metrics() {
    println!("| Scenario | Framework | ANSI bytes | Writer calls | Flushes |");
    println!("| --- | --- | ---: | ---: | ---: |");

    for scenario in UnicodeScenario::ALL {
        let arborui = arborui_unicode_metrics(scenario);
        let ratatui = ratatui_unicode_metrics(scenario);
        println!(
            "| {} | ArborUI | {} | {} | {} |",
            scenario.name(),
            arborui.bytes,
            arborui.writes,
            arborui.flushes
        );
        println!(
            "| {} | Ratatui | {} | {} | {} |",
            scenario.name(),
            ratatui.bytes,
            ratatui.writes,
            ratatui.flushes
        );

        if !matches!(scenario, UnicodeScenario::ShiftBoundary) {
            assert!(arborui.bytes > 0);
            assert!(ratatui.bytes > 0);
        }
        let expected_ratatui_flushes = if matches!(scenario, UnicodeScenario::ResizeNarrow) {
            2
        } else {
            1
        };
        assert_eq!(ratatui.flushes, expected_ratatui_flushes);
    }
}

#[test]
fn reports_resize_storm_ansi_output_metrics() {
    println!("| Workload | Framework | ANSI bytes | Writer calls | Flushes |");
    println!("| --- | --- | ---: | ---: | ---: |");

    for (workload, arborui, ratatui) in [
        (
            "Collection fixed",
            arborui_collection_resize_storm(CollectionMode::Fixed),
            ratatui_collection_resize_storm(CollectionMode::Fixed),
        ),
        (
            "Collection variable",
            arborui_collection_resize_storm(CollectionMode::Variable),
            ratatui_collection_resize_storm(CollectionMode::Variable),
        ),
        (
            "Table",
            arborui_table_resize_storm(),
            ratatui_table_resize_storm(),
        ),
        (
            "Scrolling log paused",
            arborui_log_resize_storm(),
            ratatui_log_resize_storm(),
        ),
        (
            "Overlay open",
            arborui_overlay_resize_storm(),
            ratatui_overlay_resize_storm(),
        ),
        (
            "Unicode",
            arborui_unicode_resize_storm(),
            ratatui_unicode_resize_storm(),
        ),
    ] {
        println!(
            "| {workload} | ArborUI | {} | {} | {} |",
            arborui.bytes, arborui.writes, arborui.flushes
        );
        println!(
            "| {workload} | Ratatui | {} | {} | {} |",
            ratatui.bytes, ratatui.writes, ratatui.flushes
        );
        assert!(arborui.bytes > 0);
        assert!(ratatui.bytes > 0);
        assert_eq!(arborui.flushes, STANDARD_RESIZE_STORM.len());
        assert_eq!(ratatui.flushes, STANDARD_RESIZE_STORM.len() * 2);
    }
}

fn arborui_collection_resize_storm(mode: CollectionMode) -> OutputMetrics {
    let mut application = TestApp::new(
        CollectionLab::new(mode, ITEM_COUNT, usize::from(HEIGHT - 4)),
        ArboruiSize::new(WIDTH, HEIGHT),
    );
    application.send(Message::SelectActive);
    serialize_arborui_resize_storm(&mut application, &STANDARD_RESIZE_STORM)
}

fn arborui_table_resize_storm() -> OutputMetrics {
    let mut application = TestApp::new(
        TableLab::new(ITEM_COUNT, WIDTH, HEIGHT),
        ArboruiSize::new(WIDTH, HEIGHT),
    );
    application.send(TableAction::SelectActive);
    serialize_arborui_resize_storm(&mut application, &STANDARD_RESIZE_STORM)
}

fn arborui_log_resize_storm() -> OutputMetrics {
    let mut application = TestApp::new(
        LogLab::new(ITEM_COUNT, ITEM_COUNT.saturating_mul(2), WIDTH, HEIGHT),
        ArboruiSize::new(WIDTH, HEIGHT),
    );
    application.send(LogAction::PageUp);
    serialize_arborui_resize_storm(&mut application, &STANDARD_RESIZE_STORM)
}

fn arborui_overlay_resize_storm() -> OutputMetrics {
    let mut application = TestApp::new(
        OverlayLab::new(OVERLAY_WIDTH, OVERLAY_HEIGHT),
        ArboruiSize::new(OVERLAY_WIDTH, OVERLAY_HEIGHT),
    );
    application.key(KeyCode::Enter);
    application.key(KeyCode::Tab);
    serialize_arborui_resize_storm(&mut application, &OVERLAY_RESIZE_STORM)
}

fn arborui_unicode_resize_storm() -> OutputMetrics {
    let mut application = TestApp::new(
        UnicodeLab::new(UNICODE_WIDTH, UNICODE_HEIGHT),
        ArboruiSize::new(UNICODE_WIDTH, UNICODE_HEIGHT),
    );
    for _ in 0..UNICODE_RESIZE_STORM_OFFSET {
        application.send(UnicodeAction::ShiftRight);
    }
    serialize_arborui_resize_storm(&mut application, &UNICODE_RESIZE_STORM)
}

fn serialize_arborui_resize_storm<A: Application>(
    application: &mut TestApp<A>,
    trace: &[(u16, u16)],
) -> OutputMetrics {
    let previous_count = application.frame_patches().len();
    for &(width, height) in trace {
        application.resize(ArboruiSize::new(width, height));
    }
    let patches = &application.frame_patches()[previous_count..];
    assert_eq!(patches.len(), trace.len());
    assert!(patches.iter().all(|patch| patch.full_repaint));
    serialize_arborui_patches(patches)
}

fn ratatui_collection_resize_storm(mode: CollectionMode) -> OutputMetrics {
    let mut application = RatatuiCollectionLab::new(mode, ITEM_COUNT, WIDTH, HEIGHT);
    let mut terminal =
        Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("test terminal must open");
    draw_test_terminal(&mut terminal, &mut application).expect("initial frame must draw");
    application.apply(ComparisonAction::SelectActive);
    draw_test_terminal(&mut terminal, &mut application).expect("prepared frame must draw");
    let mut frames = Vec::with_capacity(STANDARD_RESIZE_STORM.len());
    for (width, height) in STANDARD_RESIZE_STORM {
        terminal.backend_mut().resize(width, height);
        application.apply(ComparisonAction::Resize { width, height });
        draw_test_terminal(&mut terminal, &mut application).expect("resize frame must draw");
        frames.push(terminal.backend().buffer().clone());
    }
    serialize_ratatui_full_frames(&frames)
}

fn ratatui_table_resize_storm() -> OutputMetrics {
    let mut application = RatatuiTableLab::new(ITEM_COUNT, WIDTH, HEIGHT);
    let mut terminal =
        Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("test terminal must open");
    draw_test_table_terminal(&mut terminal, &mut application).expect("initial frame must draw");
    application.apply(TableAction::SelectActive);
    draw_test_table_terminal(&mut terminal, &mut application)
        .expect("prepared table frame must draw");
    let mut frames = Vec::with_capacity(STANDARD_RESIZE_STORM.len());
    for (width, height) in STANDARD_RESIZE_STORM {
        terminal.backend_mut().resize(width, height);
        application.apply(TableAction::Resize { width, height });
        draw_test_table_terminal(&mut terminal, &mut application).expect("table frame must draw");
        frames.push(terminal.backend().buffer().clone());
    }
    serialize_ratatui_full_frames(&frames)
}

fn ratatui_log_resize_storm() -> OutputMetrics {
    let mut application =
        RatatuiLogLab::new(ITEM_COUNT, ITEM_COUNT.saturating_mul(2), WIDTH, HEIGHT);
    let mut terminal =
        Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("test terminal must open");
    draw_test_log_terminal(&mut terminal, &mut application).expect("initial frame must draw");
    application.apply(LogAction::PageUp);
    draw_test_log_terminal(&mut terminal, &mut application).expect("prepared frame must draw");
    let mut frames = Vec::with_capacity(STANDARD_RESIZE_STORM.len());
    for (width, height) in STANDARD_RESIZE_STORM {
        terminal.backend_mut().resize(width, height);
        application.apply(LogAction::Resize { width, height });
        draw_test_log_terminal(&mut terminal, &mut application).expect("log frame must draw");
        frames.push(terminal.backend().buffer().clone());
    }
    serialize_ratatui_full_frames(&frames)
}

fn ratatui_overlay_resize_storm() -> OutputMetrics {
    let mut application = RatatuiOverlayLab::new(OVERLAY_WIDTH, OVERLAY_HEIGHT);
    let mut terminal = Terminal::new(TestBackend::new(OVERLAY_WIDTH, OVERLAY_HEIGHT))
        .expect("test terminal must open");
    application.apply(OverlayAction::Open);
    application.focus_next();
    draw_test_overlay_terminal(&mut terminal, &application).expect("prepared frame must draw");
    let mut frames = Vec::with_capacity(OVERLAY_RESIZE_STORM.len());
    for (width, height) in OVERLAY_RESIZE_STORM {
        terminal.backend_mut().resize(width, height);
        application.apply(OverlayAction::Resize { width, height });
        draw_test_overlay_terminal(&mut terminal, &application).expect("overlay frame must draw");
        frames.push(terminal.backend().buffer().clone());
    }
    serialize_ratatui_full_frames(&frames)
}

fn ratatui_unicode_resize_storm() -> OutputMetrics {
    let mut application = RatatuiUnicodeLab::new(UNICODE_WIDTH, UNICODE_HEIGHT);
    let mut terminal = Terminal::new(TestBackend::new(UNICODE_WIDTH, UNICODE_HEIGHT))
        .expect("test terminal must open");
    for _ in 0..UNICODE_RESIZE_STORM_OFFSET {
        application.apply(UnicodeAction::ShiftRight);
    }
    let mut frames = Vec::with_capacity(UNICODE_RESIZE_STORM.len());
    for (width, height) in UNICODE_RESIZE_STORM {
        terminal.backend_mut().resize(width, height);
        application.apply(UnicodeAction::Resize { width, height });
        frames.push(draw_ratatui_unicode_buffer(&mut terminal, &application));
    }
    serialize_ratatui_full_frames(&frames)
}

fn serialize_ratatui_full_frames(frames: &[Buffer]) -> OutputMetrics {
    let writer = CountingWriter::default();
    let metrics = writer.clone();
    let mut backend = RatatuiCrosstermBackend::new(writer);
    for frame in frames {
        let blank = Buffer::empty(frame.area);
        backend.clear().expect("Ratatui backend must clear");
        backend
            .draw(blank.diff_iter(frame))
            .expect("Ratatui full frame must serialize");
        Backend::flush(&mut backend).expect("Ratatui output must flush");
    }
    metrics.metrics()
}

fn arborui_metrics(mode: CollectionMode, scenario: Scenario) -> OutputMetrics {
    let mut application = TestApp::new(
        CollectionLab::new(mode, ITEM_COUNT, usize::from(HEIGHT - 4)),
        ArboruiSize::new(WIDTH, HEIGHT),
    );
    let patch = match scenario {
        Scenario::InitialRender => application.last_frame_patch().cloned(),
        Scenario::PageDown => patch_after(&mut application, |app| {
            app.send(Message::PageDown);
        }),
        Scenario::End => patch_after(&mut application, |app| {
            app.send(Message::End);
        }),
        Scenario::Resize => patch_after(&mut application, |app| {
            app.resize(ArboruiSize::new(WIDTH, RESIZED_HEIGHT));
        }),
        Scenario::Selection => {
            application.send(Message::Down);
            application.send(Message::SelectActive);
            application.send(Message::Home);
            patch_after(&mut application, |app| {
                app.send(Message::SelectActive);
            })
        }
        Scenario::Reverse => patch_after(&mut application, |app| {
            app.send(Message::Reverse);
        }),
        Scenario::UnchangedRedraw => patch_after(&mut application, |app| {
            app.send(Message::Home);
        }),
    };
    patch.map_or_else(OutputMetrics::default, |patch| serialize_arborui(&patch))
}

fn patch_after(
    application: &mut TestApp<CollectionLab>,
    action: impl FnOnce(&mut TestApp<CollectionLab>),
) -> Option<FramePatch> {
    let previous_count = application.frame_patches().len();
    action(application);
    application.frame_patches().get(previous_count).cloned()
}

fn serialize_arborui(patch: &FramePatch) -> OutputMetrics {
    serialize_arborui_patches(std::slice::from_ref(patch))
}

fn serialize_arborui_patches(patches: &[FramePatch]) -> OutputMetrics {
    let writer = CountingWriter::default();
    let metrics = writer.clone();
    let mut backend = ArboruiCrosstermBackend::new(writer)
        .expect("ArborUI production backend must open")
        .with_capabilities(Capabilities::default());
    for patch in patches {
        backend
            .write_patch(patch)
            .expect("ArborUI patch must serialize");
    }
    metrics.metrics()
}

fn arborui_table_metrics(scenario: TableScenario) -> OutputMetrics {
    let mut application = TestApp::new(
        TableLab::new(ITEM_COUNT, WIDTH, HEIGHT),
        ArboruiSize::new(WIDTH, HEIGHT),
    );
    let patch = match scenario {
        TableScenario::InitialRender => application.last_frame_patch().cloned(),
        TableScenario::PageDown => patch_after_table(&mut application, TableAction::PageDown),
        TableScenario::Resize => {
            let previous_count = application.frame_patches().len();
            application.resize(ArboruiSize::new(WIDTH, RESIZED_HEIGHT));
            application.frame_patches().get(previous_count).cloned()
        }
        TableScenario::Selection => patch_after_table(&mut application, TableAction::SelectActive),
        TableScenario::VisibleUpdate => patch_after_table(
            &mut application,
            TableAction::BackgroundUpdate {
                key: 0,
                revision: 1,
            },
        ),
        TableScenario::OffscreenUpdate => patch_after_table(
            &mut application,
            TableAction::BackgroundUpdate {
                key: u64::try_from(ITEM_COUNT - 1).unwrap_or(u64::MAX),
                revision: 1,
            },
        ),
    };
    patch.map_or_else(OutputMetrics::default, |patch| serialize_arborui(&patch))
}

fn patch_after_table(
    application: &mut TestApp<TableLab>,
    action: TableAction,
) -> Option<FramePatch> {
    let previous_count = application.frame_patches().len();
    application.send(action);
    application.frame_patches().get(previous_count).cloned()
}

fn arborui_log_metrics(scenario: LogScenario) -> OutputMetrics {
    let mut application = TestApp::new(
        LogLab::new(ITEM_COUNT, ITEM_COUNT.saturating_mul(2), WIDTH, HEIGHT),
        ArboruiSize::new(WIDTH, HEIGHT),
    );
    if matches!(scenario, LogScenario::AppendPaused) {
        application.send(LogAction::PageUp);
    }
    let patch = match scenario {
        LogScenario::InitialRender => application.last_frame_patch().cloned(),
        LogScenario::PageUp => patch_after_log(&mut application, LogAction::PageUp),
        LogScenario::Resize => {
            let previous_count = application.frame_patches().len();
            application.resize(ArboruiSize::new(WIDTH, RESIZED_HEIGHT));
            application.frame_patches().get(previous_count).cloned()
        }
        LogScenario::AppendFollowing | LogScenario::AppendPaused => patch_after_log(
            &mut application,
            LogAction::Append {
                count: 1,
                generation: 1,
            },
        ),
    };
    patch.map_or_else(OutputMetrics::default, |patch| serialize_arborui(&patch))
}

fn arborui_overlay_metrics(scenario: OverlayScenario) -> OutputMetrics {
    let mut application = TestApp::new(
        OverlayLab::new(OVERLAY_WIDTH, OVERLAY_HEIGHT),
        ArboruiSize::new(OVERLAY_WIDTH, OVERLAY_HEIGHT),
    );
    match scenario {
        OverlayScenario::FocusNext
        | OverlayScenario::Cancel
        | OverlayScenario::Confirm
        | OverlayScenario::ResizeOpen => {
            application.key(KeyCode::Enter);
        }
        OverlayScenario::BackgroundActivation => {
            application.key(KeyCode::Tab);
        }
        OverlayScenario::InitialRender | OverlayScenario::Open => {}
    }

    let patch = if matches!(scenario, OverlayScenario::InitialRender) {
        application.last_frame_patch().cloned()
    } else {
        let previous_count = application.frame_patches().len();
        match scenario {
            OverlayScenario::Open | OverlayScenario::Confirm => {
                application.key(KeyCode::Enter);
            }
            OverlayScenario::FocusNext => {
                application.key(KeyCode::Tab);
            }
            OverlayScenario::Cancel => {
                application.key(KeyCode::Escape);
            }
            OverlayScenario::BackgroundActivation => {
                application.key(KeyCode::Enter);
            }
            OverlayScenario::ResizeOpen => {
                application.resize(ArboruiSize::new(
                    RESIZED_OVERLAY_WIDTH,
                    RESIZED_OVERLAY_HEIGHT,
                ));
            }
            OverlayScenario::InitialRender => unreachable!("initial render handled above"),
        }
        application.frame_patches().get(previous_count).cloned()
    };
    patch.map_or_else(OutputMetrics::default, |patch| serialize_arborui(&patch))
}

fn arborui_unicode_metrics(scenario: UnicodeScenario) -> OutputMetrics {
    let mut application = TestApp::new(
        UnicodeLab::new(UNICODE_WIDTH, UNICODE_HEIGHT),
        ArboruiSize::new(UNICODE_WIDTH, UNICODE_HEIGHT),
    );
    if matches!(scenario, UnicodeScenario::ShiftBoundary) {
        for _ in 0..SHIFT_BOUNDARY_OFFSET {
            application.send(UnicodeAction::ShiftRight);
        }
    }

    let patch = if matches!(scenario, UnicodeScenario::InitialRender) {
        application.last_frame_patch().cloned()
    } else {
        let previous_count = application.frame_patches().len();
        match scenario {
            UnicodeScenario::ShiftBoundary => {
                application.send(UnicodeAction::ShiftRight);
            }
            UnicodeScenario::ReplaceWide => {
                application.send(UnicodeAction::ReplaceWide);
            }
            UnicodeScenario::ResizeNarrow => {
                application.resize(ArboruiSize::new(NARROW_UNICODE_WIDTH, UNICODE_HEIGHT));
            }
            UnicodeScenario::InitialRender => unreachable!("initial render handled above"),
        }
        application.frame_patches().get(previous_count).cloned()
    };
    patch.map_or_else(OutputMetrics::default, |patch| serialize_arborui(&patch))
}

fn patch_after_log(application: &mut TestApp<LogLab>, action: LogAction) -> Option<FramePatch> {
    let previous_count = application.frame_patches().len();
    application.send(action);
    application.frame_patches().get(previous_count).cloned()
}

fn ratatui_metrics(mode: CollectionMode, scenario: Scenario) -> OutputMetrics {
    let mut application = RatatuiCollectionLab::new(mode, ITEM_COUNT, WIDTH, HEIGHT);
    let mut terminal =
        Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("test terminal must open");
    draw_test_terminal(&mut terminal, &mut application).expect("initial frame must draw");

    if matches!(scenario, Scenario::InitialRender) {
        let initial = terminal.backend().buffer().clone();
        let blank = Buffer::empty(Rect::new(0, 0, WIDTH, HEIGHT));
        return serialize_ratatui(&blank, &initial, false);
    }

    if matches!(scenario, Scenario::Selection) {
        for action in [
            ComparisonAction::Down,
            ComparisonAction::SelectActive,
            ComparisonAction::Home,
        ] {
            application.apply(action);
            draw_test_terminal(&mut terminal, &mut application)
                .expect("selection baseline must draw");
        }
    }
    let previous = terminal.backend().buffer().clone();

    match scenario {
        Scenario::PageDown => application.apply(ComparisonAction::PageDown),
        Scenario::End => application.apply(ComparisonAction::End),
        Scenario::Resize => {
            terminal.backend_mut().resize(WIDTH, RESIZED_HEIGHT);
            application.apply(ComparisonAction::Resize {
                width: WIDTH,
                height: RESIZED_HEIGHT,
            });
        }
        Scenario::Selection => application.apply(ComparisonAction::SelectActive),
        Scenario::Reverse => application.apply(ComparisonAction::Reverse),
        Scenario::UnchangedRedraw => application.apply(ComparisonAction::Home),
        Scenario::InitialRender => unreachable!("initial render returned above"),
    }
    draw_test_terminal(&mut terminal, &mut application).expect("scenario frame must draw");
    let current = terminal.backend().buffer().clone();
    if matches!(scenario, Scenario::Resize) {
        let blank = Buffer::empty(Rect::new(0, 0, WIDTH, RESIZED_HEIGHT));
        serialize_ratatui(&blank, &current, true)
    } else {
        serialize_ratatui(&previous, &current, false)
    }
}

fn serialize_ratatui(previous: &Buffer, current: &Buffer, clear: bool) -> OutputMetrics {
    let writer = CountingWriter::default();
    let metrics = writer.clone();
    let mut backend = RatatuiCrosstermBackend::new(writer);
    if clear {
        backend.clear().expect("Ratatui backend must clear");
    }
    backend
        .draw(previous.diff_iter(current))
        .expect("Ratatui diff must serialize");
    Backend::flush(&mut backend).expect("Ratatui output must flush");
    metrics.metrics()
}

fn ratatui_table_metrics(scenario: TableScenario) -> OutputMetrics {
    let mut application = RatatuiTableLab::new(ITEM_COUNT, WIDTH, HEIGHT);
    let mut terminal =
        Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("test terminal must open");
    draw_test_table_terminal(&mut terminal, &mut application)
        .expect("initial table frame must draw");

    if matches!(scenario, TableScenario::InitialRender) {
        let initial = terminal.backend().buffer().clone();
        let blank = Buffer::empty(Rect::new(0, 0, WIDTH, HEIGHT));
        return serialize_ratatui(&blank, &initial, false);
    }
    let previous = terminal.backend().buffer().clone();
    let action = match scenario {
        TableScenario::PageDown => TableAction::PageDown,
        TableScenario::Resize => {
            terminal.backend_mut().resize(WIDTH, RESIZED_HEIGHT);
            TableAction::Resize {
                width: WIDTH,
                height: RESIZED_HEIGHT,
            }
        }
        TableScenario::Selection => TableAction::SelectActive,
        TableScenario::VisibleUpdate => TableAction::BackgroundUpdate {
            key: 0,
            revision: 1,
        },
        TableScenario::OffscreenUpdate => TableAction::BackgroundUpdate {
            key: u64::try_from(ITEM_COUNT - 1).unwrap_or(u64::MAX),
            revision: 1,
        },
        TableScenario::InitialRender => unreachable!("initial render returned above"),
    };
    application.apply(action);
    draw_test_table_terminal(&mut terminal, &mut application).expect("table frame must draw");
    let current = terminal.backend().buffer().clone();
    if matches!(scenario, TableScenario::Resize) {
        let blank = Buffer::empty(Rect::new(0, 0, WIDTH, RESIZED_HEIGHT));
        serialize_ratatui(&blank, &current, true)
    } else {
        serialize_ratatui(&previous, &current, false)
    }
}

fn ratatui_log_metrics(scenario: LogScenario) -> OutputMetrics {
    let mut application =
        RatatuiLogLab::new(ITEM_COUNT, ITEM_COUNT.saturating_mul(2), WIDTH, HEIGHT);
    let mut terminal =
        Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("test terminal must open");
    draw_test_log_terminal(&mut terminal, &mut application)
        .expect("initial scrolling-log frame must draw");

    if matches!(scenario, LogScenario::InitialRender) {
        let initial = terminal.backend().buffer().clone();
        let blank = Buffer::empty(Rect::new(0, 0, WIDTH, HEIGHT));
        return serialize_ratatui(&blank, &initial, false);
    }
    if matches!(scenario, LogScenario::AppendPaused) {
        application.apply(LogAction::PageUp);
        draw_test_log_terminal(&mut terminal, &mut application)
            .expect("paused scrolling-log baseline must draw");
    }
    let previous = terminal.backend().buffer().clone();
    let action = match scenario {
        LogScenario::PageUp => LogAction::PageUp,
        LogScenario::Resize => {
            terminal.backend_mut().resize(WIDTH, RESIZED_HEIGHT);
            LogAction::Resize {
                width: WIDTH,
                height: RESIZED_HEIGHT,
            }
        }
        LogScenario::AppendFollowing | LogScenario::AppendPaused => LogAction::Append {
            count: 1,
            generation: 1,
        },
        LogScenario::InitialRender => unreachable!("initial render returned above"),
    };
    application.apply(action);
    draw_test_log_terminal(&mut terminal, &mut application).expect("scrolling-log frame must draw");
    let current = terminal.backend().buffer().clone();
    if matches!(scenario, LogScenario::Resize) {
        let blank = Buffer::empty(Rect::new(0, 0, WIDTH, RESIZED_HEIGHT));
        serialize_ratatui(&blank, &current, true)
    } else {
        serialize_ratatui(&previous, &current, false)
    }
}

fn ratatui_overlay_metrics(scenario: OverlayScenario) -> OutputMetrics {
    let mut application = RatatuiOverlayLab::new(OVERLAY_WIDTH, OVERLAY_HEIGHT);
    let mut terminal = Terminal::new(TestBackend::new(OVERLAY_WIDTH, OVERLAY_HEIGHT))
        .expect("test terminal must open");
    draw_test_overlay_terminal(&mut terminal, &application)
        .expect("initial overlay frame must draw");

    if matches!(scenario, OverlayScenario::InitialRender) {
        let initial = terminal.backend().buffer().clone();
        let blank = Buffer::empty(Rect::new(0, 0, OVERLAY_WIDTH, OVERLAY_HEIGHT));
        return serialize_ratatui(&blank, &initial, false);
    }

    match scenario {
        OverlayScenario::FocusNext
        | OverlayScenario::Cancel
        | OverlayScenario::Confirm
        | OverlayScenario::ResizeOpen => {
            application.apply(OverlayAction::Open);
            draw_test_overlay_terminal(&mut terminal, &application)
                .expect("open overlay baseline must draw");
        }
        OverlayScenario::BackgroundActivation => {
            application.focus_next();
            draw_test_overlay_terminal(&mut terminal, &application)
                .expect("background focus baseline must draw");
        }
        OverlayScenario::InitialRender | OverlayScenario::Open => {}
    }
    let previous = terminal.backend().buffer().clone();

    match scenario {
        OverlayScenario::Open => {
            application.apply(OverlayAction::Open);
        }
        OverlayScenario::FocusNext => application.focus_next(),
        OverlayScenario::Cancel => {
            application.apply(OverlayAction::Cancel);
        }
        OverlayScenario::Confirm => {
            application.apply(OverlayAction::Confirm);
        }
        OverlayScenario::BackgroundActivation => {
            application.apply(OverlayAction::ActivateBackground);
        }
        OverlayScenario::ResizeOpen => {
            terminal
                .backend_mut()
                .resize(RESIZED_OVERLAY_WIDTH, RESIZED_OVERLAY_HEIGHT);
            application.apply(OverlayAction::Resize {
                width: RESIZED_OVERLAY_WIDTH,
                height: RESIZED_OVERLAY_HEIGHT,
            });
        }
        OverlayScenario::InitialRender => unreachable!("initial render returned above"),
    }
    draw_test_overlay_terminal(&mut terminal, &application).expect("overlay frame must draw");
    let current = terminal.backend().buffer().clone();
    if matches!(scenario, OverlayScenario::FocusNext) {
        assert_eq!(
            previous.diff_iter(&current).count(),
            20,
            "focus-next must change 20 Ratatui cells, not merely serialize an empty-diff reset"
        );
    }
    if matches!(scenario, OverlayScenario::ResizeOpen) {
        let blank = Buffer::empty(Rect::new(
            0,
            0,
            RESIZED_OVERLAY_WIDTH,
            RESIZED_OVERLAY_HEIGHT,
        ));
        serialize_ratatui(&blank, &current, true)
    } else {
        serialize_ratatui(&previous, &current, false)
    }
}

fn ratatui_unicode_metrics(scenario: UnicodeScenario) -> OutputMetrics {
    let mut application = RatatuiUnicodeLab::new(UNICODE_WIDTH, UNICODE_HEIGHT);
    let mut terminal = Terminal::new(TestBackend::new(UNICODE_WIDTH, UNICODE_HEIGHT))
        .expect("test terminal must open");
    let initial = draw_ratatui_unicode_buffer(&mut terminal, &application);

    if matches!(scenario, UnicodeScenario::InitialRender) {
        let blank = Buffer::empty(Rect::new(0, 0, UNICODE_WIDTH, UNICODE_HEIGHT));
        return serialize_ratatui(&blank, &initial, false);
    }
    let mut previous = initial;
    if matches!(scenario, UnicodeScenario::ShiftBoundary) {
        for _ in 0..SHIFT_BOUNDARY_OFFSET {
            application.apply(UnicodeAction::ShiftRight);
            previous = draw_ratatui_unicode_buffer(&mut terminal, &application);
        }
    }

    match scenario {
        UnicodeScenario::ShiftBoundary => {
            application.apply(UnicodeAction::ShiftRight);
        }
        UnicodeScenario::ReplaceWide => {
            application.apply(UnicodeAction::ReplaceWide);
        }
        UnicodeScenario::ResizeNarrow => {
            terminal
                .backend_mut()
                .resize(NARROW_UNICODE_WIDTH, UNICODE_HEIGHT);
            application.apply(UnicodeAction::Resize {
                width: NARROW_UNICODE_WIDTH,
                height: UNICODE_HEIGHT,
            });
        }
        UnicodeScenario::InitialRender => unreachable!("initial render returned above"),
    }
    let current = draw_ratatui_unicode_buffer(&mut terminal, &application);
    if matches!(scenario, UnicodeScenario::ResizeNarrow) {
        let blank = Buffer::empty(Rect::new(0, 0, NARROW_UNICODE_WIDTH, UNICODE_HEIGHT));
        serialize_ratatui(&blank, &current, true)
    } else {
        serialize_ratatui(&previous, &current, false)
    }
}

fn draw_ratatui_unicode_buffer(
    terminal: &mut Terminal<TestBackend>,
    application: &RatatuiUnicodeLab,
) -> Buffer {
    terminal
        .draw(|frame| application.render(frame))
        .expect("Unicode frame must draw")
        .buffer
        .clone()
}

fn print_metrics(
    mode: CollectionMode,
    scenario: Scenario,
    framework: &str,
    metrics: OutputMetrics,
) {
    let mode = match mode {
        CollectionMode::Fixed => "Fixed",
        CollectionMode::Variable => "Variable",
    };
    println!(
        "| {mode} | {} | {framework} | {} | {} | {} |",
        scenario.name(),
        metrics.bytes,
        metrics.writes,
        metrics.flushes
    );
}
