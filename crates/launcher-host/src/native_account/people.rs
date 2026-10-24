//! Invite-screen friends from the selected account's existing PeopleHub owner.

use super::read_identity::ReadIdentity;
use super::*;
use bedrock_services::xbox::social::{PeopleList, ProfileDetail, User};

/// Retains the verified reader while the existing artwork worker fills local paths.
pub(crate) struct NativePeople {
    pub(crate) people: Vec<bridge::Person>,
    identity: ReadIdentity,
}

impl NativePeople {
    /// Retains the original rows through account contention under the invite worker's cancellation.
    pub(crate) async fn into_current(self) -> Option<Vec<bridge::Person>> {
        self.identity.wait_current().await.then_some(self.people)
    }
}

impl NativeAccount {
    /// Reads decorated friends on the current verified proof, releasing its lease before artwork.
    /// The existing invite worker supplies the original timeout and cancellation.
    pub(crate) async fn read_people(self: Arc<Self>) -> io::Result<NativePeople> {
        let mut operation = Arc::clone(&self).acquire_verified().await?;
        let identity = ReadIdentity::capture(&operation);
        let response = operation
            .social()
            .await?
            .people(PeopleList::Friends, ProfileDetail::Full)
            .await;
        operation.persist()?;
        let people = project(response.map_err(service_error)?);
        drop(operation);
        Ok(NativePeople { people, identity })
    }
}

/// Keeps visible friends stable within each presence group before applying the display bound.
fn project(users: Vec<User>) -> Vec<bridge::Person> {
    let mut people: Vec<_> = users
        .into_iter()
        .filter_map(|user| {
            let name = if user.gamertag.trim().is_empty() {
                user.display_name.trim()
            } else {
                user.gamertag.trim()
            };
            if user.xuid.is_empty() || name.is_empty() {
                return None;
            }
            Some(bridge::Person {
                online: user.is_online(),
                gamertag: name.to_owned(),
                gamerpic: bridge::Artwork {
                    url: user
                        .display_picture
                        .as_deref()
                        .and_then(picture)
                        .unwrap_or_default(),
                    ..Default::default()
                },
                xuid: user.xuid,
            })
        })
        .collect();
    people.sort_by_cached_key(|person| (!person.online, simple_lowercase(&person.gamertag)));
    people.truncate(bridge::contract::INVITE_MAX_PEOPLE);
    people
}

/// Uses scalar lowercase keys without expanding a display-name character into several scalars.
fn simple_lowercase(value: &str) -> String {
    value
        .chars()
        .map(|ch| ch.to_lowercase().next().expect("lowercase scalar"))
        .collect()
}

/// Applies the existing launcher artwork policy before requesting a row-sized gamerpic.
fn picture(raw: &str) -> Option<String> {
    if !crate::remote_images::LAUNCHER_ART.accepts(raw) {
        return None;
    }
    let mut url = url::Url::parse(raw).ok()?;
    let size = bridge::contract::INVITE_GAMERPIC_SIDE.to_string();
    let mut query: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for (key, value) in url.query_pairs().into_owned() {
        query.entry(key).or_default().push(value);
    }
    query.insert("format".into(), vec!["png".into()]);
    query.insert("w".into(), vec![size.clone()]);
    query.insert("h".into(), vec![size]);
    let encoded = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(
            query
                .iter()
                .flat_map(|(key, values)| values.iter().map(move |value| (key, value))),
        )
        .finish()
        // URL-query escaping leaves tilde literal and quotes asterisk.
        .replace('*', "%2A")
        .replace("%7E", "~");
    url.set_query(Some(&encoded));
    Some(url.into())
}

#[cfg(test)]
mod tests;
