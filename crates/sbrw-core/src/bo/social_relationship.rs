//! `SocialRelationshipBO`: friends, friend requests and blocks between users,
//! and telling a driver's friends when their presence changes.
//!
//! Each relationship is a row per side. A request is the sender's row with
//! status 3 and the recipient's with status 0; friends have 1 on both sides;
//! a block is the blocker's row with status 2.

use anyhow::anyhow;
use sbrw_xml::dto::http::{
    ArrayOfBasicBlockPlayerInfo, ArrayOfFriendPersona, ArrayOfLong, BasicBlockPlayerInfo, FriendPersona, FriendResult,
    PersonaBase, PersonaFriendsList,
};
use sbrw_xml::dto::xmpp::{XMPP_FriendPersonaType, XMPP_ResponseTypePersonaBase};

use crate::bo::{driver_persona, presence};
use crate::dao::persona::{self, PersonaEntity};
use crate::dao::social_relationship::{self as dao, BLOCKED, FRIENDS, PENDING, REQUESTED, SocialRelationshipEntity};
use crate::engine::EngineExceptionCode;
use crate::{Fault, World};

/// `FriendResultStatus`, as the game reads a `FriendResult`.
const SUCCESS: i32 = 0;
const CANNOT_ADD_SELF: i32 = 1;
const ALREADY_FRIENDS: i32 = 2;
const CANNOT_FIND_DRIVER: i32 = 3;

fn user_of(persona_entity: &PersonaEntity) -> Result<i64, Fault> {
    persona_entity.user_id.ok_or_else(|| Fault::null_pointer("PersonaEntity.user"))
}

fn remote_persona_id(r: &SocialRelationshipEntity) -> Result<i64, Fault> {
    r.remote_persona_id.ok_or_else(|| Fault::null_pointer("SocialRelationshipEntity.remotePersonaId"))
}

// port: bo/SocialRelationshipBO#handlePersonaPresenceUpdated(PersonaPresenceUpdated)
/// Observer of presence changes (asynchronous upstream).
pub fn handle_persona_presence_updated(w: &mut World, persona_id: i64, presence: i64) -> Result<(), Fault> {
    let persona_entity = persona::find(w.db(), persona_id)?;
    send_presence_packets(w, persona_entity.as_ref(), presence)
}

// port: bo/SocialRelationshipBO#getFriendsList(Long)
/// The requests the user has received (presence 3) and their friends, each
/// friend listed once per driver their user has.
pub fn get_friends_list(w: &World, user_id: i64) -> Result<PersonaFriendsList, Fault> {
    let mut friend_persona = ArrayOfFriendPersona::default();
    for r in dao::find_by_user_id(w.db(), user_id)? {
        match r.status {
            Some(PENDING) => add_pending_request_to_list(w, &mut friend_persona, &r)?,
            Some(FRIENDS) => add_friend_to_list(w, &mut friend_persona, &r)?,
            Some(_) => {}
            None => return Err(Fault::null_pointer("SocialRelationshipEntity.status")),
        }
    }
    Ok(PersonaFriendsList { friend_persona: Some(friend_persona) })
}

// port: bo/SocialRelationshipBO#getBlockedUserList(Long)
/// Every driver of every user the user has blocked.
pub fn get_blocked_user_list(w: &World, user_id: i64) -> Result<ArrayOfBasicBlockPlayerInfo, Fault> {
    let mut list = ArrayOfBasicBlockPlayerInfo::default();
    for r in dao::find_by_user_id_and_status(w.db(), user_id, BLOCKED)? {
        add_blocked_user_to_list(w, &mut list, &r)?;
    }
    Ok(list)
}

// port: bo/SocialRelationshipBO#getBlockersByUsers(Long)
/// The users who have blocked the driver's user.
pub fn get_blockers_by_users(w: &World, persona_id: i64) -> Result<ArrayOfLong, Fault> {
    let Some(persona_entity) = persona::find(w.db(), persona_id)? else {
        return Err(Fault::engine(EngineExceptionCode::RemotePersonaIdInvalid, false));
    };
    let mut array = ArrayOfLong::default();
    for r in dao::find_by_remote_user_id_and_status(w.db(), user_of(&persona_entity)?, BLOCKED)? {
        array._long.push(r.user_id.ok_or_else(|| Fault::null_pointer("SocialRelationshipEntity.user"))?);
    }
    Ok(array)
}

// port: bo/SocialRelationshipBO#addFriend(Long,String)
/// A friend request from the driver to the one named: a row for each side,
/// and the recipient hears of it. Any row already between the two users
/// counts as already friends, except the recipient's block, which refuses.
pub fn add_friend(w: &mut World, active_persona_id: i64, display_name: Option<&str>) -> Result<FriendResult, Fault> {
    // activePersonaEntity is the SENDER, targetPersonaEntity is the RECIPIENT.
    let active_persona_entity = persona::find(w.db(), active_persona_id)?;
    let target_persona_entity = persona::find_by_name(w.db(), display_name)?;
    let mut friend_result = FriendResult::default();

    // This shouldn't happen, but we have to catch it anyway.
    let Some(active_persona_entity) = active_persona_entity.filter(|_| active_persona_id != 0) else {
        return Err(Fault::engine(EngineExceptionCode::FailedSessionSecurityPolicy, false));
    };

    // Sanity check
    let Some(target_persona_entity) = target_persona_entity else {
        friend_result.result = CANNOT_FIND_DRIVER;
        return Ok(friend_result);
    };

    if target_persona_entity.persona_id == active_persona_id {
        friend_result.result = CANNOT_ADD_SELF;
        return Ok(friend_result);
    }

    let active_user = user_of(&active_persona_entity)?;
    let target_user = user_of(&target_persona_entity)?;
    if dao::find_by_local_and_remote_user(w.db(), active_user, target_user)?
        .is_some_and(|r| r.status != Some(REQUESTED))
    {
        friend_result.result = ALREADY_FRIENDS;
        return Ok(friend_result);
    }

    if let Some(r) = dao::find_by_local_and_remote_user(w.db(), target_user, active_user)? {
        // check to see if target has blocked active player
        if r.status == Some(BLOCKED) {
            return Err(Fault::engine(EngineExceptionCode::SocialFriendRequestNotResolvable, false));
        }
        friend_result.result = ALREADY_FRIENDS;
        return Ok(friend_result);
    }
    // Create remote relationship
    create_new_relationship(w, &target_persona_entity, &active_persona_entity, PENDING)?;
    // Create local relationship
    create_new_relationship(w, &active_persona_entity, &target_persona_entity, REQUESTED)?;

    send_friend_persona_packet(w, &active_persona_entity, target_persona_entity.persona_id)?;

    friend_result.result = SUCCESS;
    friend_result.persona = Some(copy_persona_entity_to_friend_persona(Some(&target_persona_entity))?);
    Ok(friend_result)
}

// port: bo/SocialRelationshipBO#resolveFriendsRequest(Long,Long,int)
/// Accept (1) or reject (0) the other driver's request. Rejecting deletes
/// both rows, tells the sender the driver is offline and answers nothing;
/// accepting tells the sender the driver's presence and answers with the
/// sender.
pub fn resolve_friends_request(
    w: &mut World,
    active_persona_id: i64,
    friend_persona_id: Option<i64>,
    resolution: i32,
) -> Result<Option<PersonaBase>, Fault> {
    if active_persona_id == 0 {
        return Err(Fault::engine(EngineExceptionCode::FailedSessionSecurityPolicy, false));
    }
    let friend_persona_id = friend_persona_id.ok_or_else(|| Fault::null_pointer("friendPersonaId"))?;
    if friend_persona_id == 0 {
        return Err(Fault::engine(EngineExceptionCode::RemotePersonaIdInvalid, false));
    }

    let Some(active_persona_entity) = persona::find(w.db(), active_persona_id)? else {
        return Err(Fault::engine(EngineExceptionCode::RemotePersonaIdInvalid, false));
    };
    let Some(friend_persona_entity) = persona::find(w.db(), friend_persona_id)? else {
        return Err(Fault::engine(EngineExceptionCode::RemotePersonaIdInvalid, false));
    };

    let (active_user, friend_user) = (user_of(&active_persona_entity)?, user_of(&friend_persona_entity)?);
    let social_relationship_entity = dao::find_by_local_and_remote_user(w.db(), active_user, friend_user)?;
    let pending_social_relationship_entity = dao::find_by_local_and_remote_user(w.db(), friend_user, active_user)?;

    let (Some(mut social_relationship_entity), Some(mut pending_social_relationship_entity)) =
        (social_relationship_entity, pending_social_relationship_entity)
    else {
        return Err(Fault::engine(EngineExceptionCode::SocialFriendRequestNotResolvable, false));
    };

    match resolution {
        // reject
        0 => {
            dao::delete(w.db(), &social_relationship_entity)?;
            dao::delete(w.db(), &pending_social_relationship_entity)?;
            send_presence_packet(w, &active_persona_entity, 0, friend_persona_id)?;
            Ok(None)
        }
        // accept
        1 => {
            let presence = presence::get_presence(w, active_persona_id)?;
            send_presence_packet(w, &active_persona_entity, presence, friend_persona_id)?;
            pending_social_relationship_entity.status = Some(FRIENDS);
            social_relationship_entity.status = Some(FRIENDS);

            dao::update(w.db(), &pending_social_relationship_entity)?;
            dao::update(w.db(), &social_relationship_entity)?;

            Ok(Some(driver_persona::get_persona_base(w, &friend_persona_entity)?))
        }
        _ => Err(Fault::engine(EngineExceptionCode::SocialFriendRequestNotResolvable, false)),
    }
}

// port: bo/SocialRelationshipBO#removeFriend(Long,Long)
/// Unfriend, or cancel a request either way: both rows go. Blocks stay.
/// Answers with the driver's own persona.
pub fn remove_friend(
    w: &mut World,
    active_persona_id: i64,
    friend_persona_id: Option<i64>,
) -> Result<PersonaBase, Fault> {
    let Some(active_persona_entity) = persona::find(w.db(), active_persona_id)? else {
        return Err(Fault::engine(EngineExceptionCode::FailedSessionSecurityPolicy, false));
    };
    let friend_persona_id = friend_persona_id.ok_or_else(|| anyhow!("id to load is required for loading"))?;
    let Some(friend_persona_entity) = persona::find(w.db(), friend_persona_id)? else {
        return Err(Fault::engine(EngineExceptionCode::RemotePersonaIdInvalid, false));
    };

    // Possible cases:
    // - driver sends friend request and then cancels it: just delete the remote relationship
    // - one side of a friendship unfriends the other side - delete both sides
    // DO NOT MESS WITH BLOCKS!
    let (active_user, friend_user) = (user_of(&active_persona_entity)?, user_of(&friend_persona_entity)?);
    let active_side = dao::find_by_local_and_remote_user(w.db(), active_user, friend_user)?;
    let remote_side = dao::find_by_local_and_remote_user(w.db(), friend_user, active_user)?;
    let (Some(active_side), Some(remote_side)) = (active_side, remote_side) else {
        // for lack of a better exception code, we just use the "not resolvable" error... they're all the same
        // to the game.
        return Err(Fault::engine(EngineExceptionCode::SocialFriendRequestNotResolvable, false));
    };

    if remote_side.status == Some(BLOCKED) || active_side.status == Some(BLOCKED) {
        // something like this shouldn't ever happen, but... better to be safe than sorry
        return Err(Fault::engine(EngineExceptionCode::SocialFriendRequestNotResolvable, false));
    }

    dao::delete(w.db(), &active_side)?;
    dao::delete(w.db(), &remote_side)?;

    driver_persona::get_persona_base(w, &active_persona_entity)
}

// port: bo/SocialRelationshipBO#blockPlayer(Long,Long,Long)
/// Block the other driver's user. The other side's row goes, and with it
/// their block of this user if they had one; they're told the driver is
/// offline. Answers with the blocked driver.
pub fn block_player(
    w: &mut World,
    user_id: i64,
    active_persona_id: i64,
    other_persona_id: Option<i64>,
) -> Result<PersonaBase, Fault> {
    let Some(active_persona_entity) = persona::find(w.db(), active_persona_id)? else {
        return Err(Fault::engine(EngineExceptionCode::FailedSessionSecurityPolicy, false));
    };
    let other_persona_id = other_persona_id.ok_or_else(|| anyhow!("id to load is required for loading"))?;
    let Some(other_persona_entity) = persona::find(w.db(), other_persona_id)? else {
        return Err(Fault::engine(EngineExceptionCode::RemotePersonaIdInvalid, false));
    };

    let other_user = user_of(&other_persona_entity)?;
    let local_side = dao::find_by_local_and_remote_user(w.db(), user_id, other_user)?;
    let remote_side = dao::find_by_local_and_remote_user(w.db(), other_user, user_id)?;

    match local_side {
        None => create_new_relationship(w, &active_persona_entity, &other_persona_entity, BLOCKED)?,
        Some(mut local_side) => {
            local_side.status = Some(BLOCKED);
            dao::update(w.db(), &local_side)?;
        }
    }

    if let Some(remote_side) = remote_side {
        dao::delete(w.db(), &remote_side)?;
        send_presence_packet(w, &active_persona_entity, 0, other_persona_id)?;
    }

    driver_persona::get_persona_base(w, &other_persona_entity)
}

// port: bo/SocialRelationshipBO#unblockPlayer(Long,Long)
/// The user's block of the other driver's user goes. Answers with that driver.
pub fn unblock_player(w: &mut World, user_id: i64, other_persona_id: Option<i64>) -> Result<PersonaBase, Fault> {
    let other_persona_id = other_persona_id.ok_or_else(|| anyhow!("id to load is required for loading"))?;
    let Some(other_persona_entity) = persona::find(w.db(), other_persona_id)? else {
        return Err(Fault::engine(EngineExceptionCode::RemotePersonaIdInvalid, false));
    };

    match dao::find_by_local_and_remote_user(w.db(), user_id, user_of(&other_persona_entity)?)? {
        Some(local_side) if local_side.status == Some(BLOCKED) => dao::delete(w.db(), &local_side)?,
        _ => return Err(Fault::engine(EngineExceptionCode::SocialFriendRequestNotResolvable, false)),
    }

    driver_persona::get_persona_base(w, &other_persona_entity)
}

// port: bo/SocialRelationshipBO#createNewRelationship(PersonaEntity,PersonaEntity,long)
fn create_new_relationship(
    w: &World,
    local_persona: &PersonaEntity,
    remote_persona: &PersonaEntity,
    status: i64,
) -> Result<(), Fault> {
    let mut r = SocialRelationshipEntity {
        remote_persona_id: Some(remote_persona.persona_id),
        status: Some(status),
        user_id: local_persona.user_id,
        remote_user_id: remote_persona.user_id,
        ..Default::default()
    };
    dao::insert(w.db(), &mut r)?;
    Ok(())
}

// port: bo/SocialRelationshipBO#addBlockedUserToList(ArrayOfBasicBlockPlayerInfo,SocialRelationshipEntity)
fn add_blocked_user_to_list(
    w: &World,
    list: &mut ArrayOfBasicBlockPlayerInfo,
    r: &SocialRelationshipEntity,
) -> Result<(), Fault> {
    let remote_user = r.remote_user_id.ok_or_else(|| Fault::null_pointer("SocialRelationshipEntity.remoteUser"))?;
    for persona_entity in persona::find_by_user_id(w.db(), remote_user)? {
        list.basic_block_player_info
            .push(BasicBlockPlayerInfo { persona_id: persona_entity.persona_id, user_id: remote_user });
    }
    Ok(())
}

// port: bo/SocialRelationshipBO#addFriendToList(PersonaFriendsList,SocialRelationshipEntity)
fn add_friend_to_list(w: &World, list: &mut ArrayOfFriendPersona, r: &SocialRelationshipEntity) -> Result<(), Fault> {
    let remote_user = r.remote_user_id.ok_or_else(|| Fault::null_pointer("SocialRelationshipEntity.remoteUser"))?;
    for persona_entity in persona::find_by_user_id(w.db(), remote_user)? {
        let mut friend_persona = copy_persona_entity_to_friend_persona(Some(&persona_entity))?;
        friend_persona.presence = Some(presence::get_presence(w, persona_entity.persona_id)?);
        list.friend_persona.push(friend_persona);
    }
    Ok(())
}

// port: bo/SocialRelationshipBO#addPendingRequestToList(PersonaFriendsList,SocialRelationshipEntity)
fn add_pending_request_to_list(
    w: &World,
    list: &mut ArrayOfFriendPersona,
    r: &SocialRelationshipEntity,
) -> Result<(), Fault> {
    let persona_entity = persona::find(w.db(), remote_persona_id(r)?)?;
    let mut friend_persona = copy_persona_entity_to_friend_persona(persona_entity.as_ref())?;
    friend_persona.presence = Some(3);
    list.friend_persona.push(friend_persona);
    Ok(())
}

// port: bo/SocialRelationshipBO#copyPersonaEntityToFriendPersona(PersonaEntity)
fn copy_persona_entity_to_friend_persona(persona_entity: Option<&PersonaEntity>) -> Result<FriendPersona, Fault> {
    let persona_entity = persona_entity.ok_or_else(|| Fault::null_pointer("personaEntity is null!"))?;
    Ok(FriendPersona {
        persona_id: persona_entity.persona_id,
        name: persona_entity.name.clone(),
        original_name: Some("test".into()),
        social_network: 0,
        user_id: user_of(persona_entity)?,
        level: persona_entity.level,
        icon_index: persona_entity.icon_index,
        presence: None,
    })
}

// port: bo/SocialRelationshipBO#sendPresencePackets(PersonaEntity,Long)
/// The driver's presence to the driver of every row their user has: friends,
/// requests either way, and the users they have blocked.
fn send_presence_packets(w: &World, persona_entity: Option<&PersonaEntity>, presence: i64) -> Result<(), Fault> {
    let persona_entity = persona_entity.ok_or_else(|| Fault::null_pointer("personaEntity is null!"))?;
    for r in dao::find_by_user_id(w.db(), user_of(persona_entity)?)? {
        send_presence_packet(w, persona_entity, presence, remote_persona_id(&r)?)?;
    }
    Ok(())
}

// port: bo/SocialRelationshipBO#sendPresencePacket(PersonaEntity,Long,Long)
// port: bo/SocialRelationshipBO#sendPresencePacket(PersonaEntity,Long,SocialRelationshipEntity)
fn send_presence_packet(
    w: &World,
    persona_entity: &PersonaEntity,
    presence: i64,
    target_persona_id: i64,
) -> Result<(), Fault> {
    let mut persona_base = driver_persona::get_persona_base(w, persona_entity)?;
    persona_base.presence = Some(presence);
    let persona_packet = XMPP_ResponseTypePersonaBase { persona_base: Some(persona_base), ..Default::default() };
    w.openfire.send(&persona_packet, target_persona_id, &w.effects);
    Ok(())
}

// port: bo/SocialRelationshipBO#sendFriendPersonaPacket(PersonaEntity,Long)
/// The request, to its recipient: the sender with presence 3.
fn send_friend_persona_packet(w: &World, persona_entity: &PersonaEntity, target_persona_id: i64) -> Result<(), Fault> {
    let persona_base = driver_persona::get_persona_base(w, persona_entity)?;
    let persona_packet = XMPP_FriendPersonaType {
        icon_index: persona_base.icon_index,
        level: persona_base.level,
        presence: Some(3),
        name: persona_base.name,
        original_name: Some("test".into()),
        social_network: 0,
        user_id: persona_base.user_id,
        persona_id: persona_entity.persona_id,
    };
    w.openfire.send(&persona_packet, target_persona_id, &w.effects);
    Ok(())
}
