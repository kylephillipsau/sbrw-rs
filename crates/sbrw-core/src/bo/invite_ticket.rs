//! `InviteTicketBO`: invite tickets, which registration needs when the
//! server has a `TICKET_TOKEN`.

use crate::dao::invite_ticket::{self, InviteTicketEntity};
use crate::{Fault, World, datetime};

// port: bo/InviteTicketBO#createTicket(String)
/// The name's ticket, made if it has none. The number is the clock's
/// milliseconds cut to 32 bits, so tickets made in the same millisecond share
/// one.
pub fn create_ticket(w: &mut World, discord_name: Option<&str>) -> Result<InviteTicketEntity, Fault> {
    if let Some(found) = invite_ticket::find_by_discord_name(w.db(), discord_name)? {
        return Ok(found);
    }
    let time = datetime::current_time_millis(&w.env);
    let mut ticket = InviteTicketEntity {
        ticket: Some(ticket_for(time)),
        discord_name: discord_name.map(str::to_owned),
        ..Default::default()
    };
    invite_ticket::insert(w.db(), &mut ticket)?;
    Ok(ticket)
}

/// `"SBRW-" + time.intValue()`: the low 32 bits, signed.
fn ticket_for(time: i64) -> String {
    format!("SBRW-{}", time as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_the_low_32_bits_of_the_clock() {
        let mut w = World::test(&[]);
        // 2026-01-01T12:00:00Z is 1767268800000 ms.
        let alice = create_ticket(&mut w, Some("alice")).unwrap();
        assert_eq!(alice.ticket.as_deref(), Some("SBRW-2037241344"));
        assert_eq!(create_ticket(&mut w, Some("ALICE")).unwrap(), alice);
        let nobody = create_ticket(&mut w, None).unwrap();
        assert_ne!(create_ticket(&mut w, None).unwrap().id, nobody.id);
    }

    #[test]
    fn numbers_past_the_31st_bit_are_negative() {
        assert_eq!(ticket_for(1_767_468_800_000), "SBRW--2057725952");
    }
}
