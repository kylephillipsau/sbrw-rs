//! `LoginAnnouncementBO`: the images the game shows after login, per language.

use std::collections::HashMap;

use anyhow::anyhow;
use sbrw_xml::XmlValue;
use sbrw_xml::dto::http::{
    ArrayOfLoginAnnouncementDefinition, LoginAnnouncementDefinition, LoginAnnouncementsDefinition,
};

use crate::dao::login_announcement;
use crate::{Fault, World};

/// The singleton's state: each language's announcements, built on first
/// request and kept until reset.
#[derive(Debug, Default)]
pub struct LoginAnnouncements {
    announcements_cache: HashMap<String, LoginAnnouncementsDefinition>,
}

// port: bo/LoginAnnouncementBO#resetCachedAnnouncements()
pub fn reset_cached_announcements(w: &mut World) {
    w.login_announcements.announcements_cache.clear();
    tracing::info!("Cleared announcement cache");
}

// port: bo/LoginAnnouncementBO#getLoginAnnouncements(String)
/// A language's announcements: those for every language and its own. Cached,
/// so rows added later only show in languages not yet asked for.
pub fn get_login_announcements(w: &mut World, language: Option<&str>) -> Result<LoginAnnouncementsDefinition, Fault> {
    // The cache is a ConcurrentHashMap, which refuses a null key.
    let key = language.ok_or_else(|| Fault::null_pointer("language"))?;
    if let Some(cached) = w.login_announcements.announcements_cache.get(key) {
        return Ok(cached.clone());
    }

    let mut array = ArrayOfLoginAnnouncementDefinition::default();
    for entity in login_announcement::find_all_by_language(w.db(), key)? {
        array.login_announcement_definition.push(LoginAnnouncementDefinition {
            context: Some(from_value(entity.context.as_deref(), "LoginAnnouncementContext")?),
            id: entity.id,
            image_checksum: -1,
            image_url: entity.image_url,
            r#type: Some(from_value(entity.r#type.as_deref(), "LoginAnnouncementType")?),
            target: entity.target,
        });
    }
    let definition = LoginAnnouncementsDefinition {
        announcements: Some(array),
        images_path: w.params.get_str_param("ANNOUNCEMENT_DOMAIN"),
    };
    w.login_announcements.announcements_cache.insert(key.to_owned(), definition.clone());
    Ok(definition)
}

/// A JAXB enum's `fromValue`: the exact XML value, or an
/// `IllegalArgumentException`.
fn from_value<T: XmlValue>(value: Option<&str>, enum_name: &str) -> Result<T, Fault> {
    let value = value.unwrap_or("null");
    T::parse(value)
        .filter(|parsed| parsed.print() == value)
        .ok_or_else(|| anyhow!("IllegalArgumentException: {value} is not a {enum_name}").into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sbrw_xml::dto::http::{LoginAnnouncementContext, LoginAnnouncementType};

    #[test]
    fn enum_values_must_match_exactly() {
        let context: LoginAnnouncementContext = from_value(Some("VinylShop"), "LoginAnnouncementContext").unwrap();
        assert_eq!(context, LoginAnnouncementContext::VINYL_SHOP);
        assert!(from_value::<LoginAnnouncementType>(Some(" ImageOnly"), "LoginAnnouncementType").is_err());
        assert!(from_value::<LoginAnnouncementType>(None, "LoginAnnouncementType").is_err());
    }

    #[test]
    fn caches_per_language_and_refuses_no_language() {
        let mut w = World::test(&[]);
        assert!(get_login_announcements(&mut w, None).is_err());
        let insert = |w: &World, language: &str| {
            w.db()
                .execute(
                    "INSERT INTO login_announcement (imageUrl, type, context, language) VALUES ('a.jpg', 'ImageOnly', 'NotApplicable', ?1)",
                    [language],
                )
                .unwrap();
        };
        assert!(
            get_login_announcements(&mut w, Some("EN"))
                .unwrap()
                .announcements
                .unwrap()
                .login_announcement_definition
                .is_empty()
        );
        insert(&w, "EN");
        let cached = get_login_announcements(&mut w, Some("EN")).unwrap();
        assert!(cached.announcements.unwrap().login_announcement_definition.is_empty());
        reset_cached_announcements(&mut w);
        let fresh = get_login_announcements(&mut w, Some("EN")).unwrap();
        assert_eq!(fresh.announcements.unwrap().login_announcement_definition.len(), 1);
        assert_eq!(fresh.images_path.as_deref(), Some("http://i.imgur.com/"));
    }
}
