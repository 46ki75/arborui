use super::*;
use crate::{ImagePlacement, RgbaImage};

#[test]
fn nested_repaint_keeps_an_untouched_outer_draw_above_it() -> Result<(), Box<dyn std::error::Error>>
{
    let size = Size::new(1, 3);
    let c = ImagePlacement::new(RgbaImage::new(1, 1, vec![1; 4])?, Rect::new(0, 0, 1, 3));
    let b = ImagePlacement::new(RgbaImage::new(1, 1, vec![2; 4])?, Rect::new(0, 2, 1, 1));
    let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
    let initial = renderer.prepare(size, CursorState::HIDDEN, |canvas| {
        for placement in [&c, &b] {
            canvas.draw_image(placement.destination(), placement.image())?;
        }
        Ok(())
    })?;
    renderer.commit(initial)?;
    let prepared = renderer.prepare_from_current(CursorState::HIDDEN, |canvas| {
        let mut outer = canvas.with_damage_rows(&[true; 3]);
        outer.fill(c.destination(), Style::default())?;
        for placement in [&c, &b] {
            outer.draw_image(placement.destination(), placement.image())?;
        }
        let mut inner = outer.with_damage_rows(&[true, false, false]);
        inner.fill(c.destination(), Style::default())?;
        inner.draw_image(c.destination(), c.image())?;
        Ok(())
    })?;
    assert_cell_stacks_eq(prepared.images(), renderer.images(), size);
    assert_eq!(prepared.buffer(), renderer.current());
    assert_eq!(prepared.hit_map(), renderer.hit_map());
    Ok(())
}

#[test]
fn meaningful_nested_damage_preserves_the_enclosing_replay_prefix()
-> Result<(), Box<dyn std::error::Error>> {
    let size = Size::new(2, 3);
    let a = ImagePlacement::new(RgbaImage::new(1, 1, vec![1; 4])?, Rect::new(0, 0, 1, 1));
    let u = ImagePlacement::new(RgbaImage::new(1, 1, vec![2; 4])?, Rect::new(0, 2, 1, 1));
    let b = ImagePlacement::new(RgbaImage::new(1, 1, vec![3; 4])?, Rect::new(0, 0, 1, 3));
    let d = ImagePlacement::new(RgbaImage::new(1, 1, vec![4; 4])?, Rect::new(1, 0, 1, 1));
    let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
    let initial = renderer.prepare(size, CursorState::HIDDEN, |canvas| {
        for placement in [&a, &u, &b, &d] {
            canvas.draw_image(placement.destination(), placement.image())?;
        }
        Ok(())
    })?;
    renderer.commit(initial)?;
    let prepared = renderer.prepare_from_current(CursorState::HIDDEN, |canvas| {
        let mut outer = canvas.with_damage_rows(&[true, false, false]);
        outer.fill(Rect::new(0, 0, 2, 3), Style::default())?;
        for placement in [&b, &d] {
            outer.draw_image(placement.destination(), placement.image())?;
        }
        {
            let mut clipped = outer.scoped(d.destination(), Point::ORIGIN);
            let mut inner = clipped.with_damage_rows(&[true, false, false]);
            inner.fill(d.destination(), Style::default())?;
            inner.draw_image(d.destination(), d.image())?;
        }
        outer.draw_image(a.destination(), a.image())?;
        Ok(())
    })?;
    assert_cell_stacks_eq(
        prepared.images(),
        &ImageScene::from_placements([u, b, d, a]),
        size,
    );
    Ok(())
}

#[test]
fn dirty_row_insertions_match_all_orders_preserving_existing_overlap_pairs()
-> Result<(), Box<dyn std::error::Error>> {
    let placements = [
        ImagePlacement::new(RgbaImage::new(1, 1, vec![1; 4])?, Rect::new(0, 0, 1, 2)),
        ImagePlacement::new(RgbaImage::new(1, 1, vec![2; 4])?, Rect::new(0, 0, 1, 1)),
        ImagePlacement::new(RgbaImage::new(1, 1, vec![3; 4])?, Rect::new(1, 0, 1, 1)),
        ImagePlacement::new(RgbaImage::new(1, 1, vec![4; 4])?, Rect::new(1, 0, 1, 2)),
        ImagePlacement::new(RgbaImage::new(1, 1, vec![5; 4])?, Rect::new(0, 1, 2, 1)),
    ];
    let mut cases = 0;
    for initial_encoded in 0..4_usize.pow(4) {
        let initial_order: [usize; 4] =
            std::array::from_fn(|index| initial_encoded / 4_usize.pow(index as u32) % 4);
        if initial_order.iter().collect::<HashSet<_>>().len() != 4 {
            continue;
        }
        let mut initial_positions = [0; 4];
        for (position, occurrence) in initial_order.into_iter().enumerate() {
            initial_positions[occurrence] = position;
        }
        let initial = initial_order.map(|index| placements[index].clone());
        for encoded in 0..5_usize.pow(5) {
            let order: [usize; 5] =
                std::array::from_fn(|index| encoded / 5_usize.pow(index as u32) % 5);
            if order.iter().collect::<HashSet<_>>().len() != 5 {
                continue;
            }
            let mut positions = [0; 5];
            for (position, occurrence) in order.into_iter().enumerate() {
                positions[occurrence] = position;
            }
            if (positions[0] < positions[1]) != (initial_positions[0] < initial_positions[1])
                || (positions[2] < positions[3]) != (initial_positions[2] < initial_positions[3])
            {
                continue;
            }
            let desired = order.map(|index| placements[index].clone());
            check_repaint(Size::new(2, 2), &initial, &desired, &[false, true])?;
            cases += 1;
        }
    }
    assert_eq!(cases, 720);
    Ok(())
}

fn assert_cell_stacks_eq(actual: &ImageScene, expected: &ImageScene, size: Size) {
    // Disjoint placements may exchange positions; keep every equal occurrence.
    for y in 0..size.height {
        for x in 0..size.width {
            let point = Point::new(i32::from(x), i32::from(y));
            let stack = |scene: &ImageScene| {
                scene
                    .placements()
                    .iter()
                    .filter(|placement| placement.destination().contains(point))
                    .cloned()
                    .collect::<Vec<_>>()
            };
            assert_eq!(stack(actual), stack(expected), "image stack at {point:?}");
        }
    }
}

#[test]
fn new_bridge_between_disjoint_replays_preserves_both_clean_row_anchors()
-> Result<(), Box<dyn std::error::Error>> {
    let a = ImagePlacement::new(RgbaImage::new(1, 1, vec![1; 4])?, Rect::new(0, 0, 1, 2));
    let u = ImagePlacement::new(RgbaImage::new(1, 1, vec![2; 4])?, Rect::new(0, 0, 1, 1));
    let v = ImagePlacement::new(RgbaImage::new(1, 1, vec![3; 4])?, Rect::new(1, 0, 1, 1));
    let b = ImagePlacement::new(RgbaImage::new(1, 1, vec![4; 4])?, Rect::new(1, 0, 1, 2));
    let x = ImagePlacement::new(RgbaImage::new(1, 1, vec![5; 4])?, Rect::new(0, 1, 2, 1));
    // X is the only changed draw; all existing overlapping pairs retain order.
    check_repaint(
        Size::new(2, 2),
        &[a.clone(), u.clone(), v.clone(), b.clone()],
        &[v, b, x, a, u],
        &[false, true],
    )
}

#[test]
fn empty_damage_mask_during_full_paint_does_not_change_image_order()
-> Result<(), Box<dyn std::error::Error>> {
    let size = Size::new(1, 1);
    let rect = Rect::new(0, 0, 1, 1);
    let a = RgbaImage::new(1, 1, vec![1; 4])?;
    let b = RgbaImage::new(1, 1, vec![2; 4])?;
    let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
    let prepared = renderer.prepare(size, CursorState::HIDDEN, |canvas| {
        canvas.draw_image(rect, &a)?;
        let _ = canvas.with_damage_rows(&[false]);
        canvas.draw_image(rect, &b)?;
        Ok(())
    })?;
    assert_eq!(
        prepared.images().placements(),
        &[ImagePlacement::new(a, rect), ImagePlacement::new(b, rect)]
    );
    Ok(())
}

#[test]
fn image_repaint_keeps_stacking_across_clean_rows() -> Result<(), Box<dyn std::error::Error>> {
    let size = Size::new(1, 3);
    let bounds = Rect::new(0, 0, 1, 3);
    let a = ImagePlacement::new(RgbaImage::new(1, 1, vec![1; 4])?, Rect::new(0, 0, 1, 1));
    let b = ImagePlacement::new(RgbaImage::new(1, 1, vec![2; 4])?, Rect::new(0, 2, 1, 1));
    let c = ImagePlacement::new(RgbaImage::new(1, 1, vec![3; 4])?, bounds);
    let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
    let initial = renderer.prepare(size, CursorState::HIDDEN, |canvas| {
        for placement in [&a, &b, &c] {
            canvas.draw_image(placement.destination(), placement.image())?;
        }
        Ok(())
    })?;
    renderer.commit(initial)?;

    let incremental = renderer.prepare_from_current(CursorState::HIDDEN, |canvas| {
        let mut damaged = canvas.with_damage_rows(&[true, false, false]);
        damaged.fill(bounds, Style::default())?;
        damaged.draw_image(c.destination(), c.image())?;
        Ok(())
    })?;
    let full = renderer.prepare(size, CursorState::HIDDEN, |canvas| {
        for placement in [&b, &c] {
            canvas.draw_image(placement.destination(), placement.image())?;
        }
        Ok(())
    })?;
    assert_cell_stacks_eq(incremental.images(), full.images(), size);
    assert_eq!(incremental.buffer(), full.buffer());
    assert_eq!(incremental.hit_map(), full.hit_map());
    Ok(())
}

fn check_repaint(
    size: Size,
    initial: &[ImagePlacement],
    desired: &[ImagePlacement],
    rows: &[bool],
) -> Result<(), Box<dyn std::error::Error>> {
    let bounds = Rect::from_origin_size(Point::ORIGIN, size);
    let paint = |canvas: &mut Canvas<'_>, placements: &[ImagePlacement]| {
        for placement in placements {
            canvas.draw_image(placement.destination(), placement.image())?;
        }
        Ok(())
    };
    let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
    let first = renderer.prepare(size, CursorState::HIDDEN, |canvas| paint(canvas, initial))?;
    renderer.commit(first)?;
    let committed = renderer.images().clone();
    let selected = desired
        .iter()
        .filter(|placement| {
            (placement.destination().y..placement.destination().bottom())
                .any(|y| rows.get(y as usize).copied().unwrap_or(false))
        })
        .cloned()
        .collect::<Vec<_>>();
    let incremental = renderer.prepare_from_current(CursorState::HIDDEN, |canvas| {
        let mut damaged = canvas.with_damage_rows(rows);
        damaged.fill(bounds, Style::default())?;
        paint(&mut damaged, &selected)
    })?;
    let full = renderer.prepare(size, CursorState::HIDDEN, |canvas| paint(canvas, desired))?;
    assert_cell_stacks_eq(incremental.images(), full.images(), size);
    assert_eq!(incremental.buffer(), full.buffer());
    assert_eq!(incremental.hit_map(), full.hit_map());
    renderer.discard(full);
    assert_eq!(renderer.images(), &committed);
    renderer.commit(incremental)?;
    assert_cell_stacks_eq(
        renderer.images(),
        &ImageScene::from_placements(desired.iter().cloned()),
        size,
    );
    Ok(())
}

#[test]
fn inverse_untouched_overlap_keeps_the_replayed_image_below()
-> Result<(), Box<dyn std::error::Error>> {
    let a = ImagePlacement::new(RgbaImage::new(1, 1, vec![1; 4])?, Rect::new(0, 0, 1, 1));
    let b = ImagePlacement::new(RgbaImage::new(1, 1, vec![2; 4])?, Rect::new(0, 2, 1, 1));
    let c = ImagePlacement::new(RgbaImage::new(1, 1, vec![3; 4])?, Rect::new(0, 0, 1, 3));
    check_repaint(
        Size::new(1, 3),
        &[a, c.clone(), b.clone()],
        &[c, b],
        &[true, false, false],
    )
}

#[test]
fn new_draw_before_a_stale_match_keeps_its_clean_row_anchor()
-> Result<(), Box<dyn std::error::Error>> {
    let c = ImagePlacement::new(RgbaImage::new(1, 1, vec![1; 4])?, Rect::new(0, 0, 1, 3));
    let b = ImagePlacement::new(RgbaImage::new(1, 1, vec![2; 4])?, Rect::new(0, 2, 1, 1));
    let d = ImagePlacement::new(RgbaImage::new(1, 1, vec![3; 4])?, Rect::new(0, 0, 1, 1));
    let x = ImagePlacement::new(RgbaImage::new(1, 1, vec![4; 4])?, Rect::new(0, 0, 1, 1));
    check_repaint(
        Size::new(1, 3),
        &[c.clone(), b.clone(), d],
        &[x, c, b],
        &[true, false, false],
    )
}

#[test]
fn fully_damaged_images_follow_actual_replayed_order_with_equal_occurrences()
-> Result<(), Box<dyn std::error::Error>> {
    let rect = Rect::new(0, 0, 1, 1);
    let a = ImagePlacement::new(RgbaImage::new(1, 1, vec![1; 4])?, rect);
    let b = ImagePlacement::new(RgbaImage::new(1, 1, vec![2; 4])?, rect);
    let c = ImagePlacement::new(RgbaImage::new(1, 1, vec![3; 4])?, rect);
    for (initial, desired) in [
        (
            vec![a.clone(), b.clone(), c.clone()],
            vec![c, b.clone(), a.clone()],
        ),
        (
            vec![a.clone(), b.clone(), a.clone()],
            vec![a.clone(), b.clone(), a.clone()],
        ),
        (
            vec![a.clone(), b.clone(), a.clone()],
            vec![b.clone(), a.clone(), a.clone()],
        ),
        (vec![a.clone(), a.clone(), b.clone()], vec![a.clone(), b, a]),
    ] {
        check_repaint(Size::new(1, 1), &initial, &desired, &[true])?;
    }
    Ok(())
}

#[test]
fn disjoint_scene_order_is_not_a_stacking_change() -> Result<(), Box<dyn std::error::Error>> {
    let a = ImagePlacement::new(RgbaImage::new(1, 1, vec![1; 4])?, Rect::new(0, 0, 1, 1));
    let b = ImagePlacement::new(RgbaImage::new(1, 1, vec![2; 4])?, Rect::new(0, 1, 1, 1));
    let c = ImagePlacement::new(RgbaImage::new(1, 1, vec![3; 4])?, Rect::new(0, 2, 1, 1));
    check_repaint(
        Size::new(1, 3),
        &[a, b.clone(), c.clone()],
        &[b, c],
        &[true, false, true],
    )
}

#[test]
fn disjoint_replays_do_not_cross_each_others_untouched_overlaps()
-> Result<(), Box<dyn std::error::Error>> {
    let a = ImagePlacement::new(RgbaImage::new(1, 1, vec![1; 4])?, Rect::new(0, 0, 1, 2));
    let u = ImagePlacement::new(RgbaImage::new(1, 1, vec![2; 4])?, Rect::new(0, 1, 1, 1));
    let v = ImagePlacement::new(RgbaImage::new(1, 1, vec![3; 4])?, Rect::new(0, 3, 1, 1));
    let b = ImagePlacement::new(RgbaImage::new(1, 1, vec![4; 4])?, Rect::new(0, 2, 1, 2));
    check_repaint(
        Size::new(1, 4),
        &[a.clone(), u.clone(), v.clone(), b.clone()],
        &[v, b, a, u],
        &[true, false, true, false],
    )
}

#[test]
fn duplicate_removal_and_new_overlaps_repaint_complete_changed_coverage()
-> Result<(), Box<dyn std::error::Error>> {
    let a = ImagePlacement::new(RgbaImage::new(1, 1, vec![1; 4])?, Rect::new(0, 0, 1, 3));
    let b = ImagePlacement::new(RgbaImage::new(1, 1, vec![2; 4])?, Rect::new(0, 2, 1, 1));
    for desired in [vec![a.clone(), b.clone()], vec![b.clone(), a.clone()]] {
        check_repaint(
            Size::new(1, 3),
            &[a.clone(), b.clone(), a.clone()],
            &desired,
            &[true; 3],
        )?;
        check_repaint(Size::new(1, 3), &[b.clone()], &desired, &[true; 3])?;
    }
    check_repaint(Size::new(1, 3), &[a, b], &[], &[true; 3])?;
    Ok(())
}

#[test]
fn removal_only_repaint_preserves_clean_images() -> Result<(), Box<dyn std::error::Error>> {
    let a = ImagePlacement::new(RgbaImage::new(1, 1, vec![1; 4])?, Rect::new(0, 0, 1, 1));
    let b = ImagePlacement::new(RgbaImage::new(1, 1, vec![2; 4])?, Rect::new(0, 2, 1, 1));
    check_repaint(
        Size::new(1, 3),
        &[a, b.clone()],
        &[b],
        &[true, false, false],
    )
}

#[test]
fn repeated_and_nested_masks_preserve_each_draw_occurrence()
-> Result<(), Box<dyn std::error::Error>> {
    let size = Size::new(1, 3);
    let bounds = Rect::new(0, 0, 1, 3);
    let a = RgbaImage::new(1, 1, vec![1; 4])?;
    let b = RgbaImage::new(1, 1, vec![2; 4])?;
    let row = Rect::new(0, 0, 1, 1);
    let clean = Rect::new(0, 2, 1, 1);
    let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
    let initial = renderer.prepare(size, CursorState::HIDDEN, |canvas| {
        for image in [&a, &b, &a] {
            canvas.draw_image(row, image)?;
        }
        canvas.draw_image(clean, &b)?;
        Ok(())
    })?;
    renderer.commit(initial)?;
    let prepared = renderer.prepare_from_current(CursorState::HIDDEN, |canvas| {
        for _ in 0..2 {
            let mut outer = canvas.with_damage_rows(&[true, false, false]);
            let mut inner = outer.with_damage_rows(&[true, true, true]);
            inner.fill(bounds, Style::default())?;
            inner.draw_image(row, &a)?;
            {
                let mut empty = inner.with_damage_rows(&[false; 3]);
                assert!(!empty.draw_image(row, &b)?);
            }
            inner.draw_image(row, &b)?;
            inner.draw_image(row, &a)?;
            assert!(!inner.draw_image(clean, &a)?);
        }
        Ok(())
    })?;
    assert_cell_stacks_eq(prepared.images(), renderer.images(), size);
    assert_eq!(prepared.buffer(), renderer.current());
    assert_eq!(prepared.hit_map(), renderer.hit_map());
    Ok(())
}

#[test]
fn image_budget_rejections_do_not_overwrite_hit_targets() -> Result<(), Box<dyn std::error::Error>>
{
    let size = Size::new(1, 1);
    let image = RgbaImage::new(1, 1, vec![1; 4])?;
    let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
    let at_limit = renderer.prepare(size, CursorState::HIDDEN, |canvas| {
        let mut canvas = canvas
            .scoped(canvas.clip(), Point::ORIGIN)
            .with_hit(Some(crate::HitId::new(1)));
        for _ in 0..crate::MAX_IMAGE_PLACEMENTS {
            canvas.draw_image(Rect::new(0, 0, 1, 1), &image)?;
        }
        let mut rejected = canvas
            .scoped(canvas.clip(), Point::ORIGIN)
            .with_hit(Some(crate::HitId::new(2)));
        assert_eq!(
            rejected.draw_image(Rect::new(0, 0, 1, 1), &image),
            Err(DrawError::ImageScene(
                crate::ImageSceneError::TooManyPlacements {
                    maximum: crate::MAX_IMAGE_PLACEMENTS
                }
            ))
        );
        Ok(())
    })?;
    assert_eq!(
        at_limit.images().placements().len(),
        crate::MAX_IMAGE_PLACEMENTS
    );
    assert_eq!(
        at_limit.hit_map().get(Point::ORIGIN),
        Some(crate::HitId::new(1))
    );
    Ok(())
}

#[test]
fn failed_and_discarded_image_repaints_release_only_staged_source_references()
-> Result<(), Box<dyn std::error::Error>> {
    let size = Size::new(1, 3);
    let pixels: Arc<[u8]> = vec![1; 4].into();
    let image = RgbaImage::new(1, 1, Arc::clone(&pixels))?;
    let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
    let initial = renderer.prepare(size, CursorState::HIDDEN, |canvas| {
        canvas.draw_image(Rect::new(0, 0, 1, 3), &image)?;
        canvas.draw_image(Rect::new(0, 2, 1, 1), &image)?;
        Ok(())
    })?;
    renderer.commit(initial)?;
    let failed = renderer.prepare_from_current(CursorState::HIDDEN, |canvas| {
        let mut damaged = canvas.with_damage_rows(&[true; 3]);
        damaged.fill(Rect::new(0, 0, 1, 3), Style::default())?;
        for _ in 0..=crate::MAX_IMAGE_PLACEMENTS {
            damaged.draw_image(Rect::new(0, 0, 1, 3), &image)?;
        }
        Ok(())
    });
    drop(image);
    assert_eq!(
        failed.expect_err("the placement budget also applies during replay"),
        RenderError::Draw(DrawError::ImageScene(
            crate::ImageSceneError::TooManyPlacements {
                maximum: crate::MAX_IMAGE_PLACEMENTS
            }
        ))
    );
    assert_eq!(Arc::strong_count(&pixels), 3);
    let reused = renderer.prepare_reused(CursorState::HIDDEN)?;
    assert_eq!(Arc::strong_count(&pixels), 5);
    renderer.discard(reused);
    assert_eq!(Arc::strong_count(&pixels), 3);
    let removed = renderer.prepare(size, CursorState::HIDDEN, |_| Ok(()))?;
    assert_eq!(Arc::strong_count(&pixels), 3);
    renderer.commit(removed)?;
    assert_eq!(Arc::strong_count(&pixels), 1);
    Ok(())
}
