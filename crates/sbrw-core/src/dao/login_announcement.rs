//! `LoginAnnouncementEntity` (an image the game shows after login) and the
//! parts of `LoginAnnouncementDAO` ported so far.

use rusqlite::Connection;

#[derive(Debug, Clone, PartialEq)]
pub struct LoginAnnouncementEntity {
    pub id: i32,
    pub image_url: Option<String>,
    pub r#type: Option<String>,
    pub target: Option<String>,
    pub context: Option<String>,
    pub language: Option<String>,
}

// port: dao/LoginAnnouncementDAO#findAllByLanguage(String)
// port: jpa/LoginAnnouncementEntity@LoginAnnouncementEntity.findAllByLanguage
/// The announcements for every language plus those for this one, in
/// primary-key order as MySQL returns the unordered query.
pub fn find_all_by_language(db: &Connection, language: &str) -> rusqlite::Result<Vec<LoginAnnouncementEntity>> {
    let mut stmt = db.prepare_cached(
        "SELECT id, imageUrl, type, target, context, language FROM login_announcement
         WHERE language IS NULL OR language = ?1 ORDER BY id",
    )?;
    stmt.query_map([language], |r| {
        Ok(LoginAnnouncementEntity {
            id: r.get(0)?,
            image_url: r.get(1)?,
            r#type: r.get(2)?,
            target: r.get(3)?,
            context: r.get(4)?,
            language: r.get(5)?,
        })
    })?
    .collect()
}
