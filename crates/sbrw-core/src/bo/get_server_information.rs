//! `GetServerInformationBO`: the launcher's server list entry, from the
//! `SERVER_INFO_*` parameters and the latest player counts.

use crate::bo::online_users;
use crate::bo::util::server_information_vo::ServerInformationVO;
use crate::{Fault, World};

// port: bo/GetServerInformationBO#getServerInformation()
pub fn get_server_information(w: &World) -> Result<ServerInformationVO, Fault> {
    let online_users_entity = online_users::get_online_users_stats(w);
    let p = &w.params;
    let mut vo = ServerInformationVO {
        message_srv: p.get_str_param("SERVER_INFO_MESSAGE"),
        home_page_url: p.get_str_param("SERVER_INFO_HOMEPAGE_URL"),
        facebook_url: p.get_str_param("SERVER_INFO_FACEBOOK_URL"),
        discord_url: p.get_str_param("SERVER_INFO_DISCORD_URL"),
        server_name: p.get_str_param("SERVER_INFO_NAME"),
        country: p.get_str_param("SERVER_INFO_COUNTRY"),
        timezone: Some(p.get_int_param("SERVER_INFO_TIMEZONE")?),
        banner_url: p.get_str_param("SERVER_INFO_BANNER_URL"),
        admin_list: p.get_str_param("SERVER_INFO_ADMINS"),
        owner_list: p.get_str_param("SERVER_INFO_OWNERS"),
        seconds_to_shut_down: Some(p.get_int_param_or("SERVER_INFO_SHUTDOWN_TIMER", 7200)?),
        allowed_countries: p.get_str_param("SERVER_INFO_ALLOWED_COUNTRIES"),
        activated_holiday_scenery_groups: Some(p.get_str_list_param("SERVER_INFO_ENABLED_SCENERY")),
        disactivated_holiday_scenery_groups: Some(p.get_str_list_param("SERVER_INFO_DISABLED_SCENERY")),
        require_ticket: p.get_str_param("TICKET_TOKEN").is_some(),
        server_version: Some(w.build_info.version().to_owned()),
        ..Default::default()
    };
    let stats = online_users_entity.ok_or_else(|| Fault::null_pointer("OnlineUsersBO.getOnlineUsersStats()"))?;
    vo.online_number = Some(stats.number_of_online);
    vo.number_of_registered = Some(stats.number_of_registered);
    Ok(vo)
}
