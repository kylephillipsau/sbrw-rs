//! `SocialRelationshipEntity` (one user's friendship, friend request or block
//! towards another user) and `SocialRelationshipDAO`.

// port: dao/SocialRelationshipDAO#<init>()

use rusqlite::{Connection, OptionalExtension, Row, params};

/// A pending request the user received.
pub const PENDING: i64 = 0;
pub const FRIENDS: i64 = 1;
pub const BLOCKED: i64 = 2;
/// A request the user sent, not yet accepted.
pub const REQUESTED: i64 = 3;

/// `SocialRelationshipEntity`: `user` is the side the row belongs to,
/// `remoteUser` (column `fromUserId`) the other.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SocialRelationshipEntity {
    pub id: i64,
    pub user_id: Option<i64>,
    pub remote_user_id: Option<i64>,
    pub remote_persona_id: Option<i64>,
    pub status: Option<i64>,
}

impl SocialRelationshipEntity {
    fn from_row(row: &Row) -> rusqlite::Result<Self> {
        Ok(SocialRelationshipEntity {
            id: row.get("ID")?,
            user_id: row.get("userId")?,
            remote_user_id: row.get("fromUserId")?,
            remote_persona_id: row.get("remotePersonaId")?,
            status: row.get("status")?,
        })
    }
}

fn select(
    db: &Connection,
    condition: &str,
    params: impl rusqlite::Params,
) -> rusqlite::Result<Vec<SocialRelationshipEntity>> {
    db.prepare_cached(&format!("SELECT * FROM social_relationship WHERE {condition} ORDER BY ID"))?
        .query_map(params, SocialRelationshipEntity::from_row)?
        .collect()
}

// port: dao/SocialRelationshipDAO#findByUserId(Long)
// port: jpa/SocialRelationshipEntity@SocialRelationshipEntity.findByUser
pub fn find_by_user_id(db: &Connection, user_id: i64) -> rusqlite::Result<Vec<SocialRelationshipEntity>> {
    select(db, "userId = ?1", [user_id])
}

// port: dao/SocialRelationshipDAO#findByUserIdAndStatus(Long,Long)
// port: jpa/SocialRelationshipEntity@SocialRelationshipEntity.findByUserAndStatus
pub fn find_by_user_id_and_status(
    db: &Connection,
    user_id: i64,
    status: i64,
) -> rusqlite::Result<Vec<SocialRelationshipEntity>> {
    select(db, "userId = ?1 AND status = ?2", [user_id, status])
}

// port: dao/SocialRelationshipDAO#findByRemoteUserIdAndStatus(Long,Long)
// port: jpa/SocialRelationshipEntity@SocialRelationshipEntity.findByRemoteUserAndStatus
pub fn find_by_remote_user_id_and_status(
    db: &Connection,
    remote_user_id: i64,
    status: i64,
) -> rusqlite::Result<Vec<SocialRelationshipEntity>> {
    select(db, "fromUserId = ?1 AND status = ?2", [remote_user_id, status])
}

// port: dao/SocialRelationshipDAO#findByLocalAndRemoteUser(Long,Long)
// port: jpa/SocialRelationshipEntity@SocialRelationshipEntity.findByLocalAndRemoteUser
/// The first row of the user's towards the other user.
pub fn find_by_local_and_remote_user(
    db: &Connection,
    local_user_id: i64,
    remote_user_id: i64,
) -> rusqlite::Result<Option<SocialRelationshipEntity>> {
    db.prepare_cached("SELECT * FROM social_relationship WHERE userId = ?1 AND fromUserId = ?2 ORDER BY ID LIMIT 1")?
        .query_row([local_user_id, remote_user_id], SocialRelationshipEntity::from_row)
        .optional()
}

/// `SocialRelationshipDAO.insert` (`persist`); sets the generated id.
pub fn insert(db: &Connection, r: &mut SocialRelationshipEntity) -> rusqlite::Result<()> {
    db.prepare_cached(
        "INSERT INTO social_relationship (remotePersonaId, status, fromUserId, userId) VALUES (?1, ?2, ?3, ?4)",
    )?
    .execute(params![r.remote_persona_id, r.status, r.remote_user_id, r.user_id])?;
    r.id = db.last_insert_rowid();
    Ok(())
}

/// `SocialRelationshipDAO.update` (`merge`): every mapped column.
pub fn update(db: &Connection, r: &SocialRelationshipEntity) -> rusqlite::Result<()> {
    db.prepare_cached(
        "UPDATE social_relationship SET remotePersonaId = ?2, status = ?3, fromUserId = ?4, userId = ?5 WHERE ID = ?1",
    )?
    .execute(params![r.id, r.remote_persona_id, r.status, r.remote_user_id, r.user_id])?;
    Ok(())
}

/// `SocialRelationshipDAO.delete`.
pub fn delete(db: &Connection, r: &SocialRelationshipEntity) -> rusqlite::Result<()> {
    db.prepare_cached("DELETE FROM social_relationship WHERE ID = ?1")?.execute([r.id])?;
    Ok(())
}
