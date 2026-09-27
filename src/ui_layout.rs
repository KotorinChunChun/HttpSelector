//! 論理ピクセルで計算し、描画時にDPI変換する。
use crate::ui_support::Bounds;

pub const TILE_HEIGHT: i32 = 92;
pub const GAP: i32 = 12;
pub const MARGIN: i32 = 24;

pub struct RulesLayout {
    pub fixed: Vec<Bounds>,
    pub height: i32,
}

impl RulesLayout {
    pub fn new(width: i32) -> Self {
        let content = width - 48;
        let split = width >= 860;
        let list_width = if split { 300 } else { content };
        let list_height = if split { 350 } else { 200 };
        let editor_x = if split { 348 } else { 24 };
        let editor_width = if split {
            width - editor_x - 24
        } else {
            content
        };
        let top = if split { 100 } else { 354 };
        let preset_width = (editor_width - 16) / 3;
        let action_width = (editor_width - 12) / 2;
        let fixed = vec![
            Bounds::new(24, 18, content, 34),
            Bounds::new(24, 62, content, 32),
            Bounds::new(24, 100, list_width, 24),
            Bounds::new(24, 130, list_width, list_height),
            Bounds::new(editor_x, top, editor_width, 24),
            Bounds::new(editor_x, top + 30, editor_width, 36),
            Bounds::new(editor_x, top + 88, editor_width, 24),
            Bounds::new(editor_x, top + 118, editor_width, 36),
            Bounds::new(editor_x, top + 172, editor_width, 24),
            Bounds::new(editor_x, top + 204, preset_width, 36),
            Bounds::new(editor_x + preset_width + 8, top + 204, preset_width, 36),
            Bounds::new(
                editor_x + 2 * (preset_width + 8),
                top + 204,
                preset_width,
                36,
            ),
            Bounds::new(editor_x, top + 262, action_width, 36),
            Bounds::new(editor_x + action_width + 12, top + 262, action_width, 36),
            Bounds::new(editor_x, top + 310, action_width, 36),
            Bounds::new(editor_x + action_width + 12, top + 310, action_width, 36),
        ];
        Self {
            fixed,
            height: (top + 346).max(130 + list_height) + 24,
        }
    }
}

pub struct SettingsLayout {
    pub fixed: Vec<Bounds>,
    pub height: i32,
}

impl SettingsLayout {
    pub fn new(width: i32) -> Self {
        let content = width - 48;
        let split = width >= 760;
        let list_width = if split {
            (content - 24) * 2 / 5
        } else {
            content
        };
        let list_height = if split { 350 } else { 240 };
        let editor_x = if split { 48 + list_width } else { 24 };
        let editor_width = if split {
            content - list_width - 24
        } else {
            content
        };
        let editor_top = if split { 112 } else { 142 + list_height + 80 };
        let general_top = (editor_top + 380).max(142 + list_height + 44) + 28;
        let actions_top = general_top + 282;
        let half = (content - 12) / 2;
        let fixed = vec![
            Bounds::new(24, 18, content, 34),
            Bounds::new(24, 62, content, 36),
            Bounds::new(24, 112, list_width, 24),
            Bounds::new(24, 142, list_width, list_height),
            Bounds::new(24, 142 + list_height + 12, (list_width - 12) / 2, 32),
            Bounds::new(
                24 + (list_width - 12) / 2 + 12,
                142 + list_height + 12,
                (list_width - 12) / 2,
                32,
            ),
            Bounds::new(editor_x, editor_top, editor_width, 24),
            Bounds::new(editor_x, editor_top + 30, editor_width, 44),
            Bounds::new(editor_x, editor_top + 86, editor_width, 22),
            Bounds::new(editor_x, editor_top + 112, editor_width, 32),
            Bounds::new(editor_x, editor_top + 152, editor_width, 36),
            Bounds::new(editor_x, editor_top + 200, editor_width, 22),
            Bounds::new(editor_x, editor_top + 226, editor_width, 76),
            Bounds::new(editor_x, editor_top + 314, editor_width, 28),
            Bounds::new(editor_x, editor_top + 352, editor_width, 28),
            Bounds::new(24, general_top, content, 24),
            Bounds::new(24, general_top + 32, content, 24),
            Bounds::new(24, general_top + 60, 180, 32),
            Bounds::new(24, general_top + 96, content, 24),
            Bounds::new(24, general_top + 128, content, 28),
            Bounds::new(24, general_top + 164, content, 28),
            Bounds::new(24, general_top + 200, content, 68),
            Bounds::new(24, actions_top, content, 32),
            Bounds::new(24, actions_top + 44, half, 36),
            Bounds::new(24 + half + 12, actions_top + 44, half, 36),
            Bounds::new(24, actions_top + 92, content, 36),
            Bounds::new(24, actions_top + 136, content, 36),
        ];
        Self {
            fixed,
            height: actions_top + 196,
        }
    }
}

pub struct SelectorLayout {
    pub fixed: Vec<Bounds>,
    pub favorites_heading: Bounds,
    pub favorite_up: Bounds,
    pub favorite_down: Bounds,
    pub favorite_pin: Bounds,
    pub refresh: Bounds,
    pub default_rule: Bounds,
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
        let favorites_heading = Bounds::new(MARGIN, 352, (available - 112).max(1), 24);
        let favorite_top = favorites_heading.bottom() + 12;
        let section_height = |count: usize| {
            if count == 0 {
                36
            } else {
                count.div_ceil(columns) as i32 * (TILE_HEIGHT + GAP) - GAP
            }
        };
        let normal_heading_top = favorite_top + section_height(favorites) + 24;
        let refresh_width = 72.min(available.max(1));
        let default_width = 220.min((available - 192).max(1));
        let action_width = refresh_width + default_width + 120;
        let actions_on_heading = available >= action_width + 340;
        let normal_heading = if actions_on_heading {
            Bounds::new(
                MARGIN,
                normal_heading_top,
                ((available - action_width) / 2 - 12).max(1),
                24,
            )
        } else {
            Bounds::new(MARGIN, normal_heading_top, available, 24)
        };
        let actions_top = if actions_on_heading {
            normal_heading_top - 4
        } else {
            normal_heading.bottom() + 8
        };
        let normal_top = if actions_on_heading {
            normal_heading.bottom() + 12
        } else {
            actions_top + 32 + 12
        };
        let action_x = MARGIN + ((available - action_width).max(0) / 2);
        let settings_width = 100.min((available - 8).max(1) / 2);
        let windows_width = 210.min((available - settings_width - 8).max(1));
        let header_actions =
            available >= settings_width + windows_width + 16 + 132 + 14 + 160 + 178;
        let settings_x = if header_actions {
            width - MARGIN - 132 - 8 - windows_width - 8 - settings_width
        } else {
            MARGIN
        };
        let windows_x = settings_x + settings_width + 8;
        let action_y = if header_actions { 18 } else { 310 };
        let interruption = if header_actions {
            Bounds::new(settings_x - 178, 18, 170, 32)
        } else {
            Bounds::new(MARGIN, 272, 210.min(available), 32)
        };
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
            fixed: vec![
                Bounds::new(
                    24,
                    16,
                    if header_actions {
                        (settings_x - 178 - MARGIN - 14).max(1)
                    } else {
                        available - 146
                    },
                    32,
                ),
                Bounds::new(width - 156, 18, 132, 32),
                Bounds::new(24, 82, available, 24),
                Bounds::new(24, 110, available, 22),
                Bounds::new(24, 136, available, 42),
                Bounds::new(24, 190, available, 22),
                Bounds::new(24, 216, available, 42),
                Bounds::new(settings_x, action_y, settings_width, 32),
                Bounds::new(windows_x, action_y, windows_width, 32),
                interruption,
            ],
            favorites_heading,
            favorite_pin: Bounds::new(width - MARGIN - 100, 348, 100, 32),
            refresh: Bounds::new(action_x, actions_top, refresh_width, 32),
            default_rule: Bounds::new(action_x + refresh_width + 8, actions_top, default_width, 32),
            favorite_up: Bounds::new(
                action_x + refresh_width + 8 + default_width + 8,
                actions_top,
                48,
                32,
            ),
            favorite_down: Bounds::new(
                action_x + refresh_width + 8 + default_width + 8 + 48 + 8,
                actions_top,
                48,
                32,
            ),
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
    fn rules_controls_fit_small_screens_without_overlap() {
        for width in [360, 400, 512, 640, 859, 860, 940, 1120] {
            let layout = RulesLayout::new(width);
            for (index, rect) in layout.fixed.iter().enumerate() {
                assert!(rect.width > 0 && rect.x >= 24 && rect.x + rect.width <= width - 24);
                assert!(rect.bottom() <= layout.height - 24);
                assert!(
                    layout.fixed[index + 1..]
                        .iter()
                        .all(|other| !overlaps(*rect, *other)),
                    "幅{width}: 項目{index}"
                );
            }
        }
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
                        let mut regions = vec![
                            layout.favorites_heading,
                            layout.favorite_pin,
                            layout.refresh,
                            layout.favorite_up,
                            layout.favorite_down,
                            layout.default_rule,
                            layout.normal_heading,
                        ];
                        regions.extend(layout.fixed.iter().copied());
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
                                    .all(|other| !overlaps(*rect, *other)),
                                "dpi={dpi} width={width} favorites={favorites} normal={normal} index={index} rect=({}, {}, {}, {})",
                                rect.x,
                                rect.y,
                                rect.width,
                                rect.height
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

    #[test]
    fn selector_actions_are_aligned_to_normal_heading() {
        let slots = vec![None; 25];
        let layout = SelectorLayout::new(1120, &slots);
        assert_eq!(layout.default_rule.y, layout.normal_heading.y - 4);
        assert!(layout.default_rule.x > layout.normal_heading.x);
        assert_eq!(
            layout.refresh.x + layout.refresh.width + 8,
            layout.default_rule.x
        );
        assert_eq!(
            layout.favorite_up.x,
            layout.default_rule.x + layout.default_rule.width + 8
        );
        assert_eq!(
            layout.favorite_down.x,
            layout.favorite_up.x + layout.favorite_up.width + 8
        );

        let compact = SelectorLayout::new(400, &slots);
        assert_eq!(compact.default_rule.y, compact.normal_heading.bottom() + 8);
        assert_eq!(compact.refresh.y, compact.default_rule.y);
    }

    #[test]
    fn selector_settings_actions_are_left_of_close_button() {
        let layout = SelectorLayout::new(1120, &vec![None; 25]);
        let close = layout.fixed[1];
        let settings = layout.fixed[7];
        let windows = layout.fixed[8];
        assert_eq!(settings.y, close.y);
        assert_eq!(windows.y, close.y);
        assert!(settings.x + settings.width <= windows.x);
        assert!(windows.x + windows.width <= close.x);

        let compact = SelectorLayout::new(400, &vec![None; 25]);
        assert_eq!(compact.fixed[7].y, 310);
        assert_eq!(compact.fixed[8].y, 310);
        for width in [360, 400, 540, 600, 640, 700, 710, 800, 1120] {
            let layout = SelectorLayout::new(width, &vec![None; 25]);
            assert!(layout.fixed[0].width >= 160, "幅{width}でタイトルが隠れる");
        }
    }

    #[test]
    fn settings_controls_never_overlap_in_one_or_two_columns() {
        for width in [360, 400, 512, 640, 759, 760, 900, 1120, 1920] {
            let layout = SettingsLayout::new(width);
            for (index, rect) in layout.fixed.iter().enumerate() {
                assert!(rect.x >= 24 && rect.x + rect.width <= width - 24);
                assert!(rect.height > 0 && rect.bottom() <= layout.height - 24);
                assert!(
                    layout.fixed[index + 1..]
                        .iter()
                        .all(|other| !overlaps(*rect, *other)),
                    "幅{width}: 項目{index}が重なる"
                );
            }
        }
    }
}
