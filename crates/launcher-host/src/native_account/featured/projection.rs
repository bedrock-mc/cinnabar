//! The existing featured presentation policy over actual source-ordered service records.

use super::super::service_route::host_port;
use bedrock_services::minecraft::gatherings::{CatalogItem, ExperiencePlayerCount, Image};
use bridge::{Artwork, FeaturedGame, FeaturedServer};
use rand::seq::SliceRandom;
use std::collections::{BTreeMap, HashMap, HashSet};
use uuid::Uuid;

/// Filters and projects metadata without joining, fetching artwork or treating it as authority.
pub(super) fn rows(items: Vec<CatalogItem>) -> Vec<(FeaturedServer, Option<Uuid>)> {
    let mut seen = HashSet::new();
    let mut rows = Vec::with_capacity(items.len());
    for item in items {
        if item.hidden || item.content_type != "3PP_V2.0" {
            continue;
        }
        let Ok(properties) = item.properties() else {
            continue;
        };
        let host = properties.host.trim();
        let experience = Uuid::parse_str(&properties.experience_id)
            .ok()
            .filter(|id| !id.is_nil());
        let (address, selected) = if !host.is_empty() && properties.port != 0 {
            (host_port(host, properties.port), None)
        } else if let Some(id) = experience {
            (
                format!("{}{id}", bridge::contract::GATHERING_TARGET_PREFIX),
                Some(id),
            )
        } else {
            continue;
        };
        let name = localized(&item.title);
        if name.is_empty() || !seen.insert(address.clone()) {
            continue;
        }
        let server = FeaturedServer {
            name,
            address,
            group: if properties.rank.is_some() {
                "featured"
            } else {
                "creator"
            }
            .into(),
            description: localized(&item.description),
            news_title: properties.news_title.trim().into(),
            news: properties.news.trim().into(),
            logo: image(&item.images, &["Icon", "Thumbnail"]),
            background: image(&item.images, &["Banner"]),
            screenshots: item
                .images
                .iter()
                .filter(|image| {
                    image.tag.eq_ignore_ascii_case("screenshot") && valid_artwork(&image.url)
                })
                .map(|image| Artwork {
                    url: image.url.clone(),
                    ..Default::default()
                })
                .collect(),
            games: properties
                .available_games
                .iter()
                .filter(|game| !game.title.trim().is_empty() || !game.subtitle.trim().is_empty())
                .map(|game| FeaturedGame {
                    title: game.title.trim().into(),
                    subtitle: game.subtitle.trim().into(),
                    description: game.description.trim().into(),
                    image: image(&item.images, &[&game.image_tag]),
                })
                .collect(),
            ..Default::default()
        };
        rows.push((properties.rank, server, selected));
    }
    rows.sort_by_key(|row| std::cmp::Reverse(row.0));
    let creators = rows.partition_point(|row| row.0.is_some());
    rows[creators..].shuffle(&mut rand::rng());
    rows.into_iter()
        .map(|(_, server, id)| (server, id))
        .collect()
}

/// Overlays the original first population entry without replacing the cached layout.
pub(crate) fn counts(servers: &mut [FeaturedServer], counts: &[ExperiencePlayerCount]) {
    let mut by_id = HashMap::with_capacity(counts.len());
    for count in counts {
        by_id
            .entry(count.experience_id)
            .or_insert(count.player_count.filter(|count| *count >= 0));
    }
    for server in servers {
        server.player_count = server
            .address
            .strip_prefix(bridge::contract::GATHERING_TARGET_PREFIX)
            .and_then(|id| Uuid::parse_str(id).ok())
            .filter(|id| !id.is_nil())
            .and_then(|id| by_id.get(&id).copied().flatten());
    }
}

/// Preserves the catalog's explicit locale fallback rather than selecting a random dictionary key.
fn localized(values: &BTreeMap<String, String>) -> String {
    ["en-US", "en_US", "NEUTRAL", "neutral"]
        .into_iter()
        .filter_map(|locale| values.get(locale))
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_owned()
}

/// Selects tags in caller priority order, retaining the first valid image at each tag.
fn image(images: &[Image], tags: &[&str]) -> Artwork {
    for tag in tags.iter().filter(|tag| !tag.is_empty()) {
        if let Some(image) = images
            .iter()
            .find(|image| image.tag.eq_ignore_ascii_case(tag) && valid_artwork(&image.url))
        {
            return Artwork {
                url: image.url.clone(),
                ..Default::default()
            };
        }
    }
    Artwork::default()
}

/// Checks an artwork locator only; actual downloads retain the existing bounded image owner.
fn valid_artwork(raw: &str) -> bool {
    url::Url::parse(raw).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
    })
}
