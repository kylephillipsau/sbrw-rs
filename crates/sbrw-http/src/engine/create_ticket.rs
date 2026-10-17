//! `api/CreateTicket`: invite tickets, for whoever has the ticket token.

use axum::http::StatusCode;
use axum::response::Response;
use sbrw_core::Fault;
use sbrw_core::bo::invite_ticket;

use super::{Call, entity};
use crate::AppState;

// port: api/CreateTicket#createTicket(String,String)
/// With no `TICKET_TOKEN` anyone can make a ticket, though registration then
/// ignores them.
pub(super) async fn create_ticket(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let form = |name: &str| call.form.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    let (discord_name, token) = (form("discordName"), form("ticketAuth"));
    let body = state
        .world()
        .call(move |w| {
            let ticket_token = w.params.get_str_param("TICKET_TOKEN");
            if ticket_token.is_none() || ticket_token == token {
                let ticket = invite_ticket::create_ticket(w, discord_name.as_deref())?;
                Ok(format!(
                    "Discord Name: [{}] with ticket: <strong>{}</strong>",
                    ticket.discord_name.as_deref().unwrap_or("null"),
                    ticket.ticket.as_deref().unwrap_or("null"),
                ))
            } else {
                Ok("ERROR! invalid admin token!".to_owned())
            }
        })
        .await?;
    Ok(entity(call, StatusCode::OK, body))
}
