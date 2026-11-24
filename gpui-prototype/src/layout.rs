//! Pane layout presets: a pure plan of how many tab groups to make and which tabs go where.
//! The GPUI side (`workspace.rs`) turns a plan into a `DockLayout`.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutPreset {
    Single,
    LeftRight,
    TopBottom,
    Grid,
}

impl LayoutPreset {
    /// Tab groups the preset asks for.
    pub fn pane_count(self) -> usize {
        match self {
            Self::Single => 1,
            Self::LeftRight | Self::TopBottom => 2,
            Self::Grid => 4,
        }
    }
}

/// Splits `tab_count` tabs, in order, over at most `preset.pane_count()` groups.
/// Earlier groups get the extra tabs. A dock cannot hold an empty group, so the number of
/// groups never exceeds the number of tabs.
pub fn distribute(tab_count: usize, preset: LayoutPreset) -> Vec<Vec<usize>> {
    let groups = preset.pane_count().min(tab_count);
    if groups == 0 {
        return Vec::new();
    }
    let base = tab_count / groups;
    let extra = tab_count % groups;
    let mut next = 0;
    (0..groups)
        .map(|group| {
            let size = base + usize::from(group < extra);
            let range = next..next + size;
            next += size;
            range.collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_keeps_every_tab_in_one_group() {
        assert_eq!(distribute(3, LayoutPreset::Single), vec![vec![0, 1, 2]]);
    }

    #[test]
    fn two_pane_presets_split_in_order_and_give_extras_to_the_first() {
        assert_eq!(distribute(5, LayoutPreset::LeftRight), vec![vec![0, 1, 2], vec![3, 4]]);
        assert_eq!(distribute(4, LayoutPreset::TopBottom), vec![vec![0, 1], vec![2, 3]]);
    }

    #[test]
    fn never_makes_more_groups_than_tabs() {
        assert_eq!(distribute(1, LayoutPreset::Grid), vec![vec![0]]);
        assert_eq!(distribute(3, LayoutPreset::Grid), vec![vec![0], vec![1], vec![2]]);
        assert_eq!(distribute(6, LayoutPreset::Grid), vec![vec![0, 1], vec![2, 3], vec![4], vec![5]]);
        assert!(distribute(0, LayoutPreset::Grid).is_empty());
    }
}
