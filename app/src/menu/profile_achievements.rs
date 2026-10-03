//! The bounded Overview achievement lists shared by drawing and artwork loading.

use protocol::launcher_control::ProfileAchievement;
const VISIBLE_PER_SECTION: usize = 3;

/// Selects vanilla's first three suggestions and most recent completions in display order.
pub(crate) fn visible_achievements(
    entries: &[ProfileAchievement],
) -> [Vec<&ProfileAchievement>; 2] {
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
        if index < VISIBLE_PER_SECTION {
            list.insert(index, entry);
            list.truncate(VISIBLE_PER_SECTION);
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
        assert_eq!(suggested.len(), VISIBLE_PER_SECTION);
        assert_eq!(completed.len(), VISIBLE_PER_SECTION);
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
