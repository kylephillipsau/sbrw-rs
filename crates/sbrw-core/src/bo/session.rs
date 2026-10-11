//! `SessionBO`: the public chat rooms the game lists.

use sbrw_xml::dto::http::{ArrayOfChatRoom, ChatRoom};

use crate::dao::chat_room;
use crate::{Fault, World};

// port: bo/SessionBO#getAllChatRoom()
pub fn get_all_chat_room(w: &World) -> Result<ArrayOfChatRoom, Fault> {
    let mut array = ArrayOfChatRoom::default();
    for entity in chat_room::find_all(w.db())? {
        array.chat_room.push(ChatRoom {
            channel_count: entity.amount.ok_or_else(|| Fault::null_pointer("ChatRoomEntity.amount"))?,
            long_name: entity.long_name,
            short_name: entity.short_name,
        });
    }
    Ok(array)
}
