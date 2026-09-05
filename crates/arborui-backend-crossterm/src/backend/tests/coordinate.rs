use std::panic::{AssertUnwindSafe, catch_unwind};

use arborui_render::{CellRun, PatchCell, PatchCellContent};
use arborui_terminal::TerminalSession;

use super::*;

#[test]
fn maximum_cursor_coordinate_is_rejected_without_panicking() -> io::Result<()> {
    for position in boundary_positions().filter(|&position| !representable(position)) {
        let mut bytes = Vec::new();
        let backend =
            CrosstermBackend::new(&mut bytes)?.with_kitty_graphics(KittyGraphicsMode::Disabled);
        let result = catch_unwind(AssertUnwindSafe(|| {
            TerminalSession::open(
                backend,
                TerminalState {
                    screen: ScreenMode::Alternate,
                    cursor: CursorState::visible(position),
                    ..TerminalState::default()
                },
            )
            .map(drop)
        }));

        let error = result
            .expect("an unrepresentable cursor must not panic")
            .expect_err("the coordinate is outside the one-based serializer range");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        // Failed open still performs the session's unconditional style/cursor reset.
        let mut restored =
            CrosstermBackend::new(Vec::new())?.with_kitty_graphics(KittyGraphicsMode::Disabled);
        restored.restore()?;
        assert_eq!(bytes, restored.writer, "no lifecycle mode may be activated");
    }
    Ok(())
}

fn boundary_positions() -> impl Iterator<Item = Point> {
    [-1, 0, 65_534, 65_535, 65_536]
        .into_iter()
        .flat_map(|value| [Point::new(value, 0), Point::new(0, value)])
}

fn representable(position: Point) -> bool {
    (0..=65_534).contains(&position.x) && (0..=65_534).contains(&position.y)
}

fn cursor_patch(position: Point) -> FramePatch {
    FramePatch {
        size: Size::new(1, 1),
        runs: Vec::new(),
        cursor: CursorState::visible(position),
        cursor_changed: true,
        full_repaint: false,
        images: None,
    }
}

#[test]
fn cursor_coordinate_boundaries_preflight_lifecycle_and_raw_mode() -> io::Result<()> {
    for position in boundary_positions() {
        for pending_recovery in [false, true] {
            let mut backend =
                CrosstermBackend::new(Vec::new())?.with_kitty_graphics(KittyGraphicsMode::Enabled);
            backend.active.cursor = CursorState::HIDDEN;
            backend.confirmed = backend.active.clone();
            backend.lifecycle_stream_uncertain = pending_recovery;
            backend.title_unconfirmed = pending_recovery;
            if pending_recovery {
                backend.kitty.mark_stream_uncertain();
            }
            let active = backend.active.clone();
            let confirmed = backend.confirmed.clone();
            let desired = TerminalState {
                // Invalid input must be rejected before touching process-global raw mode.
                raw_mode: !representable(position),
                screen: ScreenMode::Alternate,
                title: Some(String::from("coordinate")),
                cursor: CursorState::visible(position),
                ..TerminalState::default()
            };
            let result = backend.apply_state(&desired);
            if representable(position) {
                result?;
                let movement = format!("\x1b[{};{}H", position.y + 1, position.x + 1);
                assert!(
                    backend
                        .writer
                        .windows(movement.len())
                        .any(|w| w == movement.as_bytes())
                );
                assert_eq!(backend.active.cursor, desired.cursor);
                assert_eq!(backend.confirmed.cursor, desired.cursor);
            } else {
                assert_eq!(
                    result.expect_err("invalid cursor").kind(),
                    io::ErrorKind::InvalidInput
                );
                assert!(backend.writer.is_empty());
                assert_eq!(backend.active, active);
                assert_eq!(backend.confirmed, confirmed);
                assert!(!backend.keyboard_pushed);
                assert_eq!(backend.lifecycle_stream_uncertain, pending_recovery);
                assert_eq!(backend.title_unconfirmed, pending_recovery);
                assert_eq!(backend.kitty.stream_uncertain(), pending_recovery);
            }
        }
    }
    Ok(())
}

#[test]
fn patch_cursor_coordinate_preflight_preserves_pending_recovery()
-> Result<(), Box<dyn std::error::Error>> {
    let image = RgbaImage::new(1, 1, vec![0; 4])?;
    let scene = ImageScene::from_placements([ImagePlacement::new(image, Rect::new(0, 0, 1, 1))]);
    let mut renderer = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);
    let repaint = renderer.prepare(Size::new(1, 1), CursorState::HIDDEN, |canvas| {
        canvas.draw_text(Point::ORIGIN, "x", Style::default(), None)?;
        Ok(())
    })?;

    for position in boundary_positions() {
        for kind in ["repaint", "cursor", "image", "deletion", "recovery"] {
            for pending_recovery in [false, true] {
                let mut backend = CrosstermBackend::new(Vec::new())?
                    .with_capabilities(Capabilities {
                        synchronized_updates: true,
                        ..Capabilities::default()
                    })
                    .with_kitty_graphics(KittyGraphicsMode::Enabled);
                backend.active.screen = ScreenMode::Alternate;
                backend.active.synchronized_updates = true;
                backend.confirmed = backend.active.clone();
                backend.lifecycle_stream_uncertain = pending_recovery;
                backend.title_unconfirmed = pending_recovery;
                if pending_recovery || kind == "recovery" {
                    backend.kitty.mark_stream_uncertain();
                }
                let mut patch = cursor_patch(position);
                patch.cursor_changed = kind == "cursor";
                match kind {
                    "repaint" => {
                        patch.runs.clone_from(&repaint.patch().runs);
                        patch.full_repaint = true;
                    }
                    "image" => patch.images = Some(scene.clone()),
                    "deletion" => {
                        let update = backend.kitty.prepare_with_viewport(&scene, None)?;
                        backend.kitty.confirm(&update);
                        if pending_recovery {
                            backend.kitty.mark_stream_uncertain();
                        }
                        patch.images = Some(ImageScene::new());
                    }
                    "recovery" => patch.images = Some(ImageScene::new()),
                    _ => {}
                }
                let active = backend.active.clone();
                let confirmed = backend.confirmed.clone();
                let cleanup = backend.kitty.cleanup_ids();
                let result = backend.write_patch(&patch);
                if representable(position) {
                    assert_eq!(result?, WriteOutcome::Applied);
                    let movement = format!("\x1b[{};{}H", position.y + 1, position.x + 1);
                    assert!(
                        backend
                            .writer
                            .windows(movement.len())
                            .any(|w| w == movement.as_bytes())
                    );
                    assert_eq!(backend.active.cursor, patch.cursor);
                    assert_eq!(backend.confirmed.cursor, patch.cursor);
                    continue;
                }
                let error = result.expect_err("invalid emitted cursor");
                assert_eq!(
                    error.kind(),
                    io::ErrorKind::InvalidInput,
                    "{kind} {position:?}"
                );
                assert!(
                    backend.writer.is_empty(),
                    "{kind}: no recovery, Begin, clear, or draw"
                );
                assert_eq!(backend.active, active);
                assert_eq!(backend.confirmed, confirmed);
                assert_eq!(backend.lifecycle_stream_uncertain, pending_recovery);
                assert_eq!(backend.title_unconfirmed, pending_recovery);
                assert_eq!(
                    backend.kitty.stream_uncertain(),
                    pending_recovery || kind == "recovery"
                );
                assert!(
                    cleanup
                        .iter()
                        .all(|id| backend.kitty.cleanup_ids().contains(id))
                );
            }
        }
    }
    Ok(())
}

#[test]
fn output_cursor_coordinate_preflight_precedes_begin() -> Result<(), Box<dyn std::error::Error>> {
    for position in boundary_positions() {
        let mut bytes = Vec::new();
        let result = output::apply_cursor(&mut bytes, CursorState::visible(position));
        if representable(position) {
            result?;
            assert!(
                bytes
                    .starts_with(format!("\x1b[{};{}H", position.y + 1, position.x + 1).as_bytes())
            );
        } else {
            assert_eq!(
                result.expect_err("invalid cursor").kind(),
                io::ErrorKind::InvalidInput
            );
            assert!(bytes.is_empty());
            let patch = cursor_patch(position);
            let error = output::write_patch(
                &mut bytes,
                &patch,
                &Capabilities {
                    synchronized_updates: true,
                    ..Capabilities::default()
                },
            )
            .expect_err("invalid cursor must be rejected before synchronized-update output");
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
            assert!(bytes.is_empty());
        }
    }
    Ok(())
}

#[test]
fn hidden_and_noop_cursor_coordinates_are_not_interpreted() -> Result<(), Box<dyn std::error::Error>>
{
    let image = RgbaImage::new(1, 1, vec![0; 4])?;
    let oversized = RgbaImage::new(10_001, 1, vec![0; 10_001 * 4])?;
    for position in [Point::new(i32::MIN, i32::MAX), Point::new(65_535, 65_535)] {
        let hidden = CursorState {
            position,
            ..CursorState::HIDDEN
        };
        let mut bytes = Vec::new();
        output::apply_cursor(&mut bytes, hidden)?;
        assert_eq!(bytes, b"\x1b[?25l");
        let mut backend =
            CrosstermBackend::new(Vec::new())?.with_kitty_graphics(KittyGraphicsMode::Disabled);
        backend.apply_state(&TerminalState {
            cursor: hidden,
            ..TerminalState::default()
        })?;
        let mut patch = cursor_patch(position);
        patch.cursor = hidden;
        backend.write_patch(&patch)?;
        assert_eq!(backend.active.cursor, hidden);

        for (mode, screen) in [
            (KittyGraphicsMode::Disabled, ScreenMode::Alternate),
            (KittyGraphicsMode::Enabled, ScreenMode::Main),
            (KittyGraphicsMode::Enabled, ScreenMode::Alternate),
        ] {
            for images in [
                None,
                Some(ImageScene::new()),
                Some(ImageScene::from_placements([ImagePlacement::new(
                    if mode == KittyGraphicsMode::Enabled && screen == ScreenMode::Alternate {
                        oversized.clone()
                    } else {
                        image.clone()
                    },
                    Rect::new(0, 0, 1, 1),
                )])),
            ] {
                let mut backend = CrosstermBackend::new(Vec::new())?.with_kitty_graphics(mode);
                backend.active.screen = screen;
                backend.confirmed = backend.active.clone();
                let active = backend.active.clone();
                let mut patch = cursor_patch(position);
                patch.cursor_changed = false;
                patch.images = images;
                assert_eq!(backend.write_patch(&patch)?, WriteOutcome::Applied);
                assert!(backend.writer.is_empty());
                assert_eq!(backend.active, active);
                assert_eq!(backend.confirmed, active);
            }
        }
    }
    Ok(())
}

#[test]
fn cell_coordinate_geometry_checks_offsets_before_output() -> io::Result<()> {
    assert_eq!(output::coordinate(65_533, 1)?, 65_534);
    for (base, offset) in [(65_534, 1), (i32::MAX, 1), (0, usize::MAX)] {
        assert_eq!(
            output::coordinate(base, offset)
                .expect_err("offset overflow")
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
    let cell = PatchCell {
        content: PatchCellContent::Empty,
        style: Style::default(),
        hyperlink: None,
    };
    for position in boundary_positions() {
        let mut patch = cursor_patch(Point::ORIGIN);
        patch.size = Size::new(u16::MAX, u16::MAX);
        patch.cursor = CursorState::HIDDEN;
        patch.runs = vec![CellRun {
            position,
            cells: vec![cell.clone()],
        }];
        let mut bytes = Vec::new();
        let result = output::write_patch(&mut bytes, &patch, &Capabilities::default());
        if representable(position) {
            result?;
            assert!(
                bytes
                    .starts_with(format!("\x1b[{};{}H", position.y + 1, position.x + 1).as_bytes())
            );
        } else {
            assert_eq!(
                result.expect_err("out-of-frame cell").kind(),
                io::ErrorKind::InvalidInput
            );
            assert!(bytes.is_empty());
        }
    }
    let mut patch = cursor_patch(Point::ORIGIN);
    patch.size = Size::new(u16::MAX, 1);
    patch.runs = vec![CellRun {
        position: Point::new(65_533, 0),
        cells: vec![cell.clone(); 2],
    }];
    let mut bytes = Vec::new();
    output::write_patch(&mut bytes, &patch, &Capabilities::default())?;
    assert!(
        bytes
            .windows(b"\x1b[1;65535H".len())
            .any(|w| w == b"\x1b[1;65535H")
    );
    patch.runs[0].cells.push(cell);
    bytes.clear();
    assert_eq!(
        output::write_patch(&mut bytes, &patch, &Capabilities::default())
            .expect_err("offset crosses serializer range")
            .kind(),
        io::ErrorKind::InvalidInput
    );
    assert!(bytes.is_empty());
    Ok(())
}

#[test]
fn kitty_coordinate_checks_cover_upload_and_repeated_placement()
-> Result<(), Box<dyn std::error::Error>> {
    let image = RgbaImage::new(1, 1, vec![0; 4])?;
    for position in boundary_positions() {
        for repeated in [false, true] {
            let mut placements = Vec::new();
            let mut bytes = Vec::new();
            let mut expected_prefix = Vec::new();
            if repeated {
                placements.push(ImagePlacement::new(image.clone(), Rect::new(0, 0, 1, 1)));
                let scene = ImageScene::from_placements(placements.clone());
                let mut kitty = kitty::KittyState::default();
                let update = kitty.prepare_with_viewport(&scene, None)?;
                kitty::write_update_content(&mut expected_prefix, &update)?;
            }
            placements.push(ImagePlacement::new(
                image.clone(),
                Rect::new(position.x, position.y, 1, 1),
            ));
            let scene = ImageScene::from_placements(placements);
            let mut patch = cursor_patch(Point::ORIGIN);
            patch.size = Size::new(u16::MAX, u16::MAX);
            patch.images = Some(scene.clone());
            assert_eq!(patch.validate().is_ok(), representable(position));

            let mut kitty = kitty::KittyState::default();
            let update = kitty.prepare_with_viewport(&scene, None)?;
            let result = kitty::write_update_content(&mut bytes, &update);
            if representable(position) {
                result?;
                let action = if repeated { "p" } else { "T" };
                let movement = format!(
                    "\x1b[{};{}H\x1b_Ga={action},",
                    position.y + 1,
                    position.x + 1
                );
                assert!(bytes[expected_prefix.len()..].starts_with(movement.as_bytes()));
            } else {
                assert_eq!(
                    result.expect_err("invalid image coordinate").kind(),
                    io::ErrorKind::InvalidInput
                );
                assert_eq!(bytes, expected_prefix);
            }
        }
    }
    Ok(())
}
