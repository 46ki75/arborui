use arborui_core::{Point, Rect};

use crate::{Buffer, BufferError, CellContent, HitMap};

/// How empty cells on a surface affect lower surfaces.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Opacity {
    /// Empty cells leave lower surfaces unchanged.
    #[default]
    Transparent,
    /// Empty cells overwrite lower surfaces.
    Opaque,
}

/// An independently positioned and clipped cell buffer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Surface {
    /// Surface cell contents.
    pub buffer: Buffer,
    /// Surface origin in target coordinates.
    pub position: Point,
    /// Visible region in local surface coordinates.
    pub clip: Rect,
    /// Paint ordering; larger values appear above smaller values.
    pub z_index: i32,
    /// Empty-cell composition behavior.
    pub opacity: Opacity,
    /// Optional interactive identities painted with the surface.
    pub hit_map: Option<HitMap>,
}

impl Surface {
    /// Creates a transparent surface at the origin.
    #[must_use]
    pub fn new(buffer: Buffer) -> Self {
        let clip = buffer.bounds();
        Self {
            buffer,
            position: Point::ORIGIN,
            clip,
            z_index: 0,
            opacity: Opacity::Transparent,
            hit_map: None,
        }
    }

    /// Attaches interactive identities in local surface coordinates.
    #[must_use]
    pub fn with_hit_map(mut self, hit_map: HitMap) -> Self {
        self.hit_map = Some(hit_map);
        self
    }
}

/// Composes ordered surfaces into a target buffer.
pub struct Compositor;

impl Compositor {
    /// Composes surfaces in ascending z-index order.
    pub fn compose(target: &mut Buffer, surfaces: &[Surface]) -> Result<(), BufferError> {
        let mut ignored_hits = HitMap::new(target.size());
        Self::compose_with_hits(target, &mut ignored_hits, surfaces)
    }

    /// Composes visuals and interactive identities using the same ordering and clipping.
    pub fn compose_with_hits(
        target: &mut Buffer,
        target_hits: &mut HitMap,
        surfaces: &[Surface],
    ) -> Result<(), BufferError> {
        if target_hits.size() != target.size() {
            return Err(BufferError::SizeMismatch {
                expected: target.size(),
                actual: target_hits.size(),
            });
        }
        for surface in surfaces {
            if let Some(hit_map) = &surface.hit_map {
                if hit_map.size() != surface.buffer.size() {
                    return Err(BufferError::SizeMismatch {
                        expected: surface.buffer.size(),
                        actual: hit_map.size(),
                    });
                }
            }
        }
        let mut ordered: Vec<_> = surfaces.iter().enumerate().collect();
        ordered.sort_by_key(|(index, surface)| (surface.z_index, *index));

        for (_, surface) in ordered {
            compose_surface(target, target_hits, surface)?;
        }
        Ok(())
    }
}

fn compose_surface(
    target: &mut Buffer,
    target_hits: &mut HitMap,
    surface: &Surface,
) -> Result<(), BufferError> {
    let Some(clip) = surface.clip.intersection(surface.buffer.bounds()) else {
        return Ok(());
    };

    for y in clip.y..clip.bottom() {
        for x in clip.x..clip.right() {
            let source_point = Point::new(x, y);
            let Some(cell) = surface.buffer.get(source_point) else {
                continue;
            };
            let target_point = surface.position.translated(x, y);
            match cell.content {
                CellContent::Empty if surface.opacity == Opacity::Opaque => {
                    if target.bounds().contains(target_point) {
                        clear_target_hit_span(target, target_hits, target_point);
                        target.set_empty(target_point, cell.style)?;
                        copy_hit_option(surface, source_point, target_hits, target_point);
                    }
                }
                CellContent::Empty | CellContent::Continuation { .. } => {}
                CellContent::Grapheme { id, width } => {
                    let source_end = source_point.x.saturating_add(i32::from(width) - 1);
                    let target_end = target_point.x.saturating_add(i32::from(width) - 1);
                    if clip.contains(Point::new(source_end, source_point.y))
                        && target.bounds().contains(target_point)
                        && target
                            .bounds()
                            .contains(Point::new(target_end, target_point.y))
                    {
                        for offset in 0..width {
                            clear_target_hit_span(
                                target,
                                target_hits,
                                target_point.translated(i32::from(offset), 0),
                            );
                        }
                        target.set_grapheme(target_point, id, width, cell.style, cell.hyperlink)?;
                        for offset in 0..width {
                            copy_hit_option(
                                surface,
                                source_point.translated(i32::from(offset), 0),
                                target_hits,
                                target_point.translated(i32::from(offset), 0),
                            );
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn copy_hit_option(surface: &Surface, source: Point, target: &mut HitMap, destination: Point) {
    let hit = surface.hit_map.as_ref().and_then(|map| map.get(source));
    let _ = target.set_option(destination, hit);
}

fn clear_target_hit_span(target: &Buffer, target_hits: &mut HitMap, point: Point) {
    let span = match target.get(point).map(|cell| cell.content) {
        Some(CellContent::Grapheme { width, .. }) => Some((point, width)),
        Some(CellContent::Continuation { offset, .. }) => {
            let start = point.translated(-i32::from(offset), 0);
            match target.get(start).map(|cell| cell.content) {
                Some(CellContent::Grapheme { width, .. }) => Some((start, width)),
                _ => None,
            }
        }
        Some(CellContent::Empty) => Some((point, 1)),
        None => None,
    };
    if let Some((start, width)) = span {
        for offset in 0..width {
            let _ = target_hits.clear(start.translated(i32::from(offset), 0));
        }
    }
}

#[cfg(test)]
mod tests {
    use arborui_core::{Color, Size, Style};

    use super::*;
    use crate::GraphemeId;

    #[test]
    fn higher_surfaces_overwrite_lower_surfaces() -> Result<(), BufferError> {
        let mut lower = Buffer::new(Size::new(1, 1));
        lower.set_empty(Point::ORIGIN, Style::new().background(Color::Blue))?;
        let mut upper = Buffer::new(Size::new(1, 1));
        upper.set_grapheme(
            Point::ORIGIN,
            GraphemeId::from_test_value(1),
            1,
            Style::new().foreground(Color::White),
            None,
        )?;
        let surfaces = [Surface::new(lower), Surface::new(upper)];
        let mut target = Buffer::new(Size::new(1, 1));

        Compositor::compose(&mut target, &surfaces)?;

        assert!(matches!(
            target.get(Point::ORIGIN).map(|cell| cell.content),
            Some(CellContent::Grapheme { .. })
        ));
        Ok(())
    }

    #[test]
    fn hit_composition_follows_z_index_and_clipping() -> Result<(), BufferError> {
        let mut lower_buffer = Buffer::new(Size::new(2, 1));
        lower_buffer.set_empty(Point::ORIGIN, Style::default())?;
        lower_buffer.set_empty(Point::new(1, 0), Style::default())?;
        let mut lower_hits = HitMap::new(Size::new(2, 1));
        lower_hits.fill(lower_buffer.bounds(), crate::HitId::new(1));
        let mut lower = Surface::new(lower_buffer).with_hit_map(lower_hits);
        lower.opacity = Opacity::Opaque;
        lower.z_index = 10;

        let mut upper_buffer = Buffer::new(Size::new(2, 1));
        upper_buffer.set_empty(Point::ORIGIN, Style::default())?;
        upper_buffer.set_empty(Point::new(1, 0), Style::default())?;
        let mut upper_hits = HitMap::new(Size::new(2, 1));
        upper_hits.fill(upper_buffer.bounds(), crate::HitId::new(2));
        let mut upper = Surface::new(upper_buffer).with_hit_map(upper_hits);
        upper.opacity = Opacity::Opaque;
        upper.clip = Rect::new(1, 0, 1, 1);
        upper.z_index = 20;

        let mut target = Buffer::new(Size::new(2, 1));
        let mut target_hits = HitMap::new(Size::new(2, 1));
        Compositor::compose_with_hits(&mut target, &mut target_hits, &[upper, lower])?;

        assert_eq!(target_hits.get(Point::ORIGIN), Some(crate::HitId::new(1)));
        assert_eq!(
            target_hits.get(Point::new(1, 0)),
            Some(crate::HitId::new(2))
        );
        Ok(())
    }

    #[test]
    fn opaque_noninteractive_surface_blocks_lower_hit() -> Result<(), BufferError> {
        let mut lower_buffer = Buffer::new(Size::new(1, 1));
        lower_buffer.set_empty(Point::ORIGIN, Style::default())?;
        let mut lower_hits = HitMap::new(Size::new(1, 1));
        let _ = lower_hits.set(Point::ORIGIN, crate::HitId::new(1));
        let mut lower = Surface::new(lower_buffer).with_hit_map(lower_hits);
        lower.opacity = Opacity::Opaque;

        let mut upper_buffer = Buffer::new(Size::new(1, 1));
        upper_buffer.set_empty(Point::ORIGIN, Style::default())?;
        let mut upper = Surface::new(upper_buffer);
        upper.opacity = Opacity::Opaque;
        upper.z_index = 1;

        let mut target = Buffer::new(Size::new(1, 1));
        let mut target_hits = HitMap::new(Size::new(1, 1));
        Compositor::compose_with_hits(&mut target, &mut target_hits, &[lower, upper])?;

        assert_eq!(target_hits.get(Point::ORIGIN), None);
        Ok(())
    }
}
