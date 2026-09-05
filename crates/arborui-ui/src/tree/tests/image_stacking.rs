use super::*;
use arborui_render::RgbaImage;

#[test]
fn image_paint_update_preserves_stacking_without_recomposition()
-> Result<(), Box<dyn std::error::Error>> {
    let images = [
        RgbaImage::new(1, 1, vec![1; 4])?,
        RgbaImage::new(1, 1, vec![2; 4])?,
        RgbaImage::new(1, 1, vec![3; 4])?,
    ];
    let size = Size::new(1, 3);
    for timed in [false, true] {
        let calls = [Cell::new(0), Cell::new(0), Cell::new(0)];
        let view = |show_a: bool| {
            let image_node = |index: usize, height, visible, fingerprint| {
                let image = &images[index];
                let calls = &calls[index];
                Element::<()>::container([])
                    .key(index as u64)
                    .layout(LayoutStyle::new().size(Dimension::cells(1), Dimension::cells(height)))
                    .paint(fingerprint, move |size, canvas| {
                        calls.set(calls.get() + 1);
                        if visible {
                            canvas
                                .draw_image(Rect::from_origin_size(Point::ORIGIN, size), image)?;
                        }
                        Ok(())
                    })
            };
            Element::container([
                image_node(0, 1, show_a, if show_a { 1 } else { 2 }),
                Element::container([])
                    .key(3_u64)
                    .layout(LayoutStyle::new().size(Dimension::cells(1), Dimension::cells(1))),
                image_node(1, 1, true, 1),
                image_node(2, 3, true, 1).layout(
                    LayoutStyle::new()
                        .size(Dimension::cells(1), Dimension::cells(3))
                        .position(Position::Absolute),
                ),
            ])
            .layout(LayoutStyle::new().direction(FlexDirection::Column))
        };
        let mut tree = UiTree::new();
        let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
        prepare_and_commit(&mut tree, &view(true), size, &mut renderer)?;
        assert_eq!(renderer.images().placements().len(), 3);
        let changed = view(false);
        let mut reconciled = tree.clone_for_staging();
        let report = reconciled.reconcile(&changed)?;
        assert_eq!(report.created, 0);
        assert_eq!(report.removed, 0);
        assert_eq!(report.invalidation, Invalidation::Paint);

        let incremental = if timed {
            let (prepared, timings) = tree.prepare_timed(&changed, size, &mut renderer)?;
            assert_eq!(timings.layout, Duration::ZERO);
            assert_eq!(timings.repaint_regions, 1);
            assert_eq!(timings.repaint_cells, 1);
            prepared
        } else {
            tree.prepare(&changed, size, &mut renderer)?
        };
        assert_eq!(calls.each_ref().map(Cell::get), [2, 1, 2]);
        let reference = tree.prepare_full(&changed, size, &mut renderer)?;
        for y in 0..3 {
            let stack = |scene: &ImageScene| {
                scene
                    .placements()
                    .iter()
                    .filter(|placement| placement.destination().contains(Point::new(0, y)))
                    .cloned()
                    .collect::<Vec<_>>()
            };
            assert_eq!(stack(incremental.images()), stack(reference.images()));
        }
        assert_eq!(incremental.buffer(), reference.buffer());
        assert_eq!(incremental.hit_map(), reference.hit_map());
        tree.discard(reference, &mut renderer);
        tree.commit(incremental, &mut renderer)?;
        let unchanged = tree.prepare(&changed, size, &mut renderer)?;
        assert!(unchanged.patch().is_empty());
        assert_eq!(calls.each_ref().map(Cell::get), [3, 2, 3]);
        tree.discard(unchanged, &mut renderer);
    }
    Ok(())
}
