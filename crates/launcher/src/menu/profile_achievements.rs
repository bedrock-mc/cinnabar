//! The bounded Overview achievement lists shared by drawing and artwork loading.

use protocol::launcher_control::ProfileAchievement;
/// Reads the same embedded card limit used by the core's artwork selection.
fn visible_per_section() -> usize {
    include_str!("../../../../core/catalog/profile_overview_limit.txt")
        .trim()
        .parse()
        .expect("invalid embedded Profile Overview achievement limit")
}

/// Selects vanilla's first three suggestions and most recent completions in display order.
pub fn visible_achievements(entries: &[ProfileAchievement]) -> [Vec<&ProfileAchievement>; 2] {
    let limit = visible_per_section();
    let mut sections: [Vec<&ProfileAchievement>; 2] = [Vec::new(), Vec::new()];
    for entry in entries {
        let section = if entry.locked && entry.suggested_order.is_some() {
            0
        } else if !entry.locked && !entry.date_unlocked.is_empty() {
            1
        } else {
            continue;
        };
        let list = &mut sections[section];
        let index = list.partition_point(|existing| {
            if section == 0 {
                existing.suggested_order <= entry.suggested_order
            } else {
                existing.date_unlocked >= entry.date_unlocked
            }
        });
        if index < limit {
            list.insert(index, entry);
            list.truncate(limit);
        }
    }
    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_lists_are_bounded_and_follow_the_display_order() {
        let entries: Vec<_> = (0..100)
            .rev()
            .flat_map(|index| {
                [
                    ProfileAchievement {
                        locked: true,
                        suggested_order: Some(index),
                        ..Default::default()
                    },
                    ProfileAchievement {
                        date_unlocked: format!("{}-01-01T00:00:00Z", 2000 + index),
                        ..Default::default()
                    },
                ]
            })
            .collect();
        let [suggested, completed] = visible_achievements(&entries);
        assert_eq!(suggested.len(), visible_per_section());
        assert_eq!(completed.len(), visible_per_section());
        assert_eq!(suggested[0].suggested_order, Some(0));
        assert!(
            suggested
                .windows(2)
                .all(|pair| pair[0].suggested_order <= pair[1].suggested_order)
        );
        assert!(
            completed
                .windows(2)
                .all(|pair| pair[0].date_unlocked >= pair[1].date_unlocked)
        );
    }
}
