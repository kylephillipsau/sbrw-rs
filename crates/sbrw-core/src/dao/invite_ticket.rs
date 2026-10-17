//! `InviteTicketEntity` and `InviteTicketDAO`.

// port: dao/InviteTicketDAO#<init>()

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::dao::user;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct InviteTicketEntity {
    pub id: i64,
    pub ticket: Option<String>,
    pub discord_name: Option<String>,
    /// `@ManyToOne user`, by id; loaded on demand with [`user::find`].
    pub user_id: Option<i64>,
}

impl InviteTicketEntity {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("ID")?,
            ticket: row.get("TICKET")?,
            discord_name: row.get("DISCORD_NAME")?,
            user_id: row.get("USERID")?,
        })
    }

    /// `getUser()`: the linked account, if any.
    pub fn user(&self, db: &Connection) -> rusqlite::Result<Option<user::UserEntity>> {
        self.user_id.map(|id| user::find(db, id)).transpose().map(Option::flatten)
    }
}

// port: dao/InviteTicketDAO#findByTicket(String)
// port: jpa/InviteTicketEntity@InviteTicketEntity.findByTicket
pub fn find_by_ticket(db: &Connection, ticket: Option<&str>) -> rusqlite::Result<Option<InviteTicketEntity>> {
    // JPQL `= :ticket` with a null parameter matches nothing, as in SQL.
    db.prepare_cached("SELECT * FROM invite_ticket WHERE TICKET = ?1 ORDER BY ID LIMIT 1")?
        .query_row([ticket], InviteTicketEntity::from_row)
        .optional()
}

// port: dao/InviteTicketDAO#findByDiscordName(String)
// port: jpa/InviteTicketEntity@InviteTicketEntity.findByDiscordName
/// The first ticket made for a name; names match without regard to case, as
/// in MySQL's collation, and no ticket matches a null name.
pub fn find_by_discord_name(
    db: &Connection,
    discord_name: Option<&str>,
) -> rusqlite::Result<Option<InviteTicketEntity>> {
    db.prepare_cached("SELECT * FROM invite_ticket WHERE DISCORD_NAME = ?1 ORDER BY ID LIMIT 1")?
        .query_row([discord_name], InviteTicketEntity::from_row)
        .optional()
}

/// `InviteTicketDAO.insert` (`persist`); sets the generated id.
pub fn insert(db: &Connection, ticket: &mut InviteTicketEntity) -> rusqlite::Result<()> {
    db.prepare_cached("INSERT INTO invite_ticket (DISCORD_NAME, TICKET, USERID) VALUES (?1, ?2, ?3)")?
        .execute(params![ticket.discord_name, ticket.ticket, ticket.user_id])?;
    ticket.id = db.last_insert_rowid();
    Ok(())
}

/// `InviteTicketDAO.update` (`merge`): every mapped column, from this copy.
pub fn update(db: &Connection, ticket: &InviteTicketEntity) -> rusqlite::Result<()> {
    db.prepare_cached("UPDATE invite_ticket SET DISCORD_NAME = ?2, TICKET = ?3, USERID = ?4 WHERE ID = ?1")?
        .execute(params![ticket.id, ticket.discord_name, ticket.ticket, ticket.user_id])?;
    Ok(())
}
