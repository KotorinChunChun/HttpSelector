//! 論理ピクセルで計算し、描画時にDPI変換する。
use crate::ui_support::Bounds;

pub const TILE_HEIGHT: i32 = 92;
pub const GAP: i32 = 12;
pub const MARGIN: i32 = 24;

pub struct SelectorLayout {
    pub favorites_heading: Bounds,
    pub normal_heading: Bounds,
    pub favorite_empty: Bounds,
    pub normal_empty: Bounds,
    pub tiles: Vec<Option<Bounds>>,
    pub height: i32,
}

impl SelectorLayout {
    pub fn new(width: i32, slots: &[Option<usize>]) -> Self {
        let available = width - MARGIN * 2;
        let columns = ((available + GAP) / (196 + GAP)).clamp(1, 5) as usize;
        let tile_width = (available - GAP * (columns as i32 - 1)) / columns as i32;
        let favorites = slots[..5].iter().flatten().count();
        let normal = slots[5..].iter().flatten().count();
        let favorites_heading = Bounds::new(MARGIN, 322, available, 24);
        let favorite_top = favorites_heading.bottom() + 12;
        let section_height = |count: usize| {
            if count == 0 {
                36
            } else {
                count.div_ceil(columns) as i32 * (TILE_HEIGHT + GAP) - GAP
            }
        };
        let normal_heading = Bounds::new(
            MARGIN,
            favorite_top + section_height(favorites) + 24,
            available,
            24,
        );
        let normal_top = normal_heading.bottom() + 12;
        let mut tiles = vec![None; slots.len()];
        for (start, end, top) in [(0, 5, favorite_top), (5, slots.len(), normal_top)] {
            for (position, slot) in (start..end)
                .filter(|index| slots[*index].is_some())
                .enumerate()
            {
                tiles[slot] = Some(Bounds::new(
                    MARGIN + (position % columns) as i32 * (tile_width + GAP),
                    top + (position / columns) as i32 * (TILE_HEIGHT + GAP),
                    tile_width,
                    TILE_HEIGHT,
                ));
            }
        }
        Self {
            favorites_heading,
            normal_heading,
            favorite_empty: Bounds::new(MARGIN, favorite_top, available, 36),
            normal_empty: Bounds::new(MARGIN, normal_top, available, 36),
            tiles,
            height: normal_top + section_height(normal) + MARGIN,
        }
    }

    pub fn neighbor(&self, current: usize, horizontal: i32, vertical: i32) -> Option<usize> {
        let origin = self.tiles.get(current).copied().flatten()?;
        self.tiles
            .iter()
            .enumerate()
            .filter_map(|(index, bounds)| {
                let bounds = (*bounds)?;
                let dx = bounds.x - origin.x;
                let dy = bounds.y - origin.y;
                let valid = if horizontal != 0 {
                    dy == 0 && dx * horizontal > 0
                } else {
                    dy * vertical > 0
                };
                valid.then_some((
                    index,
                    if horizontal != 0 {
                        dx.abs()
                    } else {
                        dy.abs() * 10000 + dx.abs()
                    },
                ))
            })
            .min_by_key(|(_, distance)| *distance)
            .map(|(index, _)| index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn overlaps(a: Bounds, b: Bounds) -> bool {
        a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.bottom() && a.bottom() > b.y
    }

    #[test]
    fn all_profile_counts_fit_without_overlaps_at_supported_widths_and_dpis() {
        for dpi in [96, 120, 144, 192] {
            for pixels in [800, 1024, 1280, 1366, 1920] {
                let width = pixels * 96 / dpi;
                for favorites in 0..=5 {
                    for normal in 0..=20 {
                        let mut slots = vec![None; 25];
                        slots[..favorites].fill(Some(0));
                        slots[5..5 + normal].fill(Some(0));
                        let layout = SelectorLayout::new(width, &slots);
                        let mut regions = vec![layout.favorites_heading, layout.normal_heading];
                        regions.extend(layout.tiles.iter().flatten().copied());
                        if favorites == 0 {
                            regions.push(layout.favorite_empty);
                        }
                        if normal == 0 {
                            regions.push(layout.normal_empty);
                        }
                        for (index, rect) in regions.iter().enumerate() {
                            assert!(rect.x >= MARGIN && rect.x + rect.width <= width - MARGIN);
                            assert!(rect.bottom() <= layout.height - MARGIN);
                            assert!(
                                regions[index + 1..]
                                    .iter()
                                    .all(|other| !overlaps(*rect, *other))
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn arrows_cross_empty_slots_and_follow_actual_columns() {
        let mut slots = vec![None; 25];
        slots[0] = Some(0);
        slots[5..10].fill(Some(0));
        let layout = SelectorLayout::new(720, &slots);
        assert_eq!(layout.tiles[5].unwrap().y, layout.tiles[7].unwrap().y);
        assert_eq!(layout.neighbor(0, 0, 1), Some(5));
        assert_eq!(layout.neighbor(7, 0, -1), Some(0));
        assert_eq!(layout.neighbor(5, 0, 1), Some(8));
        assert_eq!(layout.neighbor(7, 1, 0), None);
    }
}
