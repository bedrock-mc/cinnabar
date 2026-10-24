//! Application summary and cards from the account's bounded achievement collection.

use bedrock_services::xbox::achievements::{Achievement, Collection};
use std::io;

/// Rejects another account and retains source ordering, unavailable rewards and checked totals.
pub(super) fn project(
    values: Collection,
    xuid: &str,
) -> io::Result<Option<bridge::ProfileAchievements>> {
    if values.owner_xuid != xuid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "native achievement identity changed",
        ));
    }
    if values.malformed_items != 0 {
        return Ok(None);
    }
    let total = u32::try_from(values.achievements.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "native achievement count exceeds bounds",
        )
    })?;
    let mut result = bridge::ProfileAchievements {
        total,
        current_gamerscore: Some(0),
        max_gamerscore: Some(0),
        ..Default::default()
    };
    for value in values.achievements {
        let entry = entry(value);
        if !entry.locked {
            result.unlocked += 1;
        }
        add(&mut result.max_gamerscore, entry.gamerscore);
        if !entry.locked {
            add(&mut result.current_gamerscore, entry.gamerscore);
        }
        result.entries.push(entry);
    }
    Ok(Some(result))
}

/// Selects only the first actual Icon and Gamerscore, never inventing suggestion or reward art.
fn entry(value: Achievement) -> bridge::ProfileAchievement {
    let locked = value.progress_state.as_deref() != Some("Achieved");
    let image = value
        .media_assets
        .into_iter()
        .find(|asset| {
            asset.kind == "Icon" && crate::remote_images::LAUNCHER_ART.accepts(&asset.url)
        })
        .map(|asset| bridge::Artwork {
            url: asset.url,
            path: String::new(),
        })
        .unwrap_or_default();
    let gamerscore = value
        .rewards
        .iter()
        .find(|reward| reward.kind.as_deref() == Some("Gamerscore"))
        .and_then(|reward| reward.value.as_deref())
        .and_then(|raw| raw.parse::<i64>().ok())
        .filter(|score| *score >= 0);
    bridge::ProfileAchievement {
        id: value.id,
        name: value.name,
        description: if locked {
            value.locked_description
        } else {
            value.description
        },
        image,
        gamerscore,
        locked,
        date_unlocked: if locked {
            String::new()
        } else {
            value
                .progression
                .as_ref()
                .and_then(|progress| progress.get("timeUnlocked"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned()
        },
        suggested_order: None,
    }
}

/// Once a score is unavailable or overflows, later known rewards cannot restore the total.
fn add(total: &mut Option<i64>, score: Option<i64>) {
    *total = total.and_then(|total| score.and_then(|score| total.checked_add(score)));
}
