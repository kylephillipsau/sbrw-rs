//! `UserEntity` (an account) and `UserDAO`.

// port: dao/UserDAO#<init>()
// port: dao/util/LongKeyedDAO#<init>(Class)

use rusqlite::{Connection, OptionalExtension, Row, params};
use time::PrimitiveDateTime;

use crate::dao::persona::{self, PersonaEntity};
use crate::datetime;

#[derive(Debug, Clone, PartialEq)]
pub struct UserEntity {
    pub id: i64,
    pub email: Option<String>,
    /// What the launcher sends: the SHA-1 hex of the password. sbrw-rs stores
    /// an argon2id hash of it (see `crate::password`).
    pub password: Option<String>,
    pub hwid: Option<String>,
    pub game_hardware_hash: Option<String>,
    pub ip_address: Option<String>,
    /// EAGER: loaded with the user, so copies held elsewhere (like a session)
    /// keep the list as it was when they were loaded.
    pub personas: Vec<PersonaEntity>,
    pub premium: bool,
    pub is_admin: bool,
    pub is_locked: bool,
    pub created: Option<PrimitiveDateTime>,
    pub last_login: Option<PrimitiveDateTime>,
    pub selected_persona_index: Option<i32>,
}

impl UserEntity {
    /// `new UserEntity()`: an unsaved user with Java's field defaults.
    pub fn new() -> Self {
        Self {
            id: 0,
            email: None,
            password: None,
            hwid: None,
            game_hardware_hash: None,
            ip_address: None,
            personas: Vec::new(),
            premium: false,
            is_admin: false,
            is_locked: false,
            created: None,
            last_login: None,
            selected_persona_index: Some(0),
        }
    }

    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("ID")?,
            email: row.get("EMAIL")?,
            password: row.get("PASSWORD")?,
            hwid: row.get("HWID")?,
            game_hardware_hash: row.get("gameHardwareHash")?,
            ip_address: row.get("IP_ADDRESS")?,
            personas: Vec::new(),
            // Primitive booleans: Hibernate reads SQL NULL as false.
            premium: row.get::<_, Option<bool>>("premium")?.unwrap_or(false),
            is_admin: row.get::<_, Option<bool>>("isAdmin")?.unwrap_or(false),
            is_locked: row.get::<_, Option<bool>>("isLocked")?.unwrap_or(false),
            created: datetime::column(row, "created")?,
            last_login: datetime::column(row, "lastLogin")?,
            selected_persona_index: row.get("selectedPersonaIndex")?,
        })
    }

    /// Load the EAGER `personas` relationship.
    fn with_personas(mut self, db: &Connection) -> rusqlite::Result<Self> {
        self.personas = persona::find_by_user_id(db, self.id)?;
        Ok(self)
    }
}

impl Default for UserEntity {
    fn default() -> Self {
        Self::new()
    }
}

// port: dao/util/LongKeyedDAO#find(Long)
// port: dao/util/BaseDAO#find(TK)
pub fn find(db: &Connection, id: i64) -> rusqlite::Result<Option<UserEntity>> {
    db.prepare_cached("SELECT * FROM user WHERE ID = ?1")?
        .query_row([id], UserEntity::from_row)
        .optional()?
        .map(|u| u.with_personas(db))
        .transpose()
}

// port: dao/UserDAO#findByEmail(String)
// port: jpa/UserEntity@UserEntity.findByEmail
pub fn find_by_email(db: &Connection, email: &str) -> rusqlite::Result<Option<UserEntity>> {
    db.prepare_cached("SELECT * FROM user WHERE EMAIL = ?1 ORDER BY ID LIMIT 1")?
        .query_row([email], UserEntity::from_row)
        .optional()?
        .map(|u| u.with_personas(db))
        .transpose()
}

// port: dao/UserDAO#countUsers()
// port: jpa/UserEntity@UserEntity.countUsers
pub fn count_users(db: &Connection) -> rusqlite::Result<i64> {
    db.query_row("SELECT COUNT(*) FROM user", [], |r| r.get(0))
}

// port: dao/util/BaseDAO#insert(TE)
/// `UserDAO.insert` (`persist`); sets the generated id.
pub fn insert(db: &Connection, user: &mut UserEntity) -> rusqlite::Result<()> {
    db.prepare_cached(
        "INSERT INTO user (EMAIL, PASSWORD, HWID, gameHardwareHash, IP_ADDRESS, premium, isAdmin, isLocked, created, lastLogin, selectedPersonaIndex)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )?
    .execute(params![
        user.email,
        user.password,
        user.hwid,
        user.game_hardware_hash,
        user.ip_address,
        user.premium,
        user.is_admin,
        user.is_locked,
        datetime::opt_to_sql(user.created),
        datetime::opt_to_sql(user.last_login),
        user.selected_persona_index,
    ])?;
    user.id = db.last_insert_rowid();
    Ok(())
}

// port: dao/util/BaseDAO#update(TE)
/// `UserDAO.update` (`merge`): every mapped column, from this copy.
pub fn update(db: &Connection, user: &UserEntity) -> rusqlite::Result<()> {
    db.prepare_cached(
        "UPDATE user SET EMAIL = ?2, PASSWORD = ?3, HWID = ?4, gameHardwareHash = ?5, IP_ADDRESS = ?6, premium = ?7,
         isAdmin = ?8, isLocked = ?9, created = ?10, lastLogin = ?11, selectedPersonaIndex = ?12 WHERE ID = ?1",
    )?
    .execute(params![
        user.id,
        user.email,
        user.password,
        user.hwid,
        user.game_hardware_hash,
        user.ip_address,
        user.premium,
        user.is_admin,
        user.is_locked,
        datetime::opt_to_sql(user.created),
        datetime::opt_to_sql(user.last_login),
        user.selected_persona_index,
    ])?;
    Ok(())
}
