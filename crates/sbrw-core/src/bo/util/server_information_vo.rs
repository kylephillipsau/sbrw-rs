//! `ServerInformationVO`: the launcher's server list entry, as JSON.

use serde::Serialize;

/// Upstream serialises it with JSON-B: properties in alphabetical order, nulls
/// left out. The VO's `basePath`, `serverID` and `features` fields have no
/// getters, so JSON-B never writes them, and they aren't here.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInformationVO {
    // port: bo/util/ServerInformationVO#getActivatedHolidaySceneryGroups()
    // port: bo/util/ServerInformationVO#setActivatedHolidaySceneryGroups(List)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activated_holiday_scenery_groups: Option<Vec<String>>,
    // port: bo/util/ServerInformationVO#getAdminList()
    // port: bo/util/ServerInformationVO#setAdminList(String)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admin_list: Option<String>,
    // port: bo/util/ServerInformationVO#getAllowedCountries()
    // port: bo/util/ServerInformationVO#setAllowedCountries(String)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_countries: Option<String>,
    // port: bo/util/ServerInformationVO#getBannerUrl()
    // port: bo/util/ServerInformationVO#setBannerUrl(String)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub banner_url: Option<String>,
    // port: bo/util/ServerInformationVO#getCountry()
    // port: bo/util/ServerInformationVO#setCountry(String)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    // port: bo/util/ServerInformationVO#getDisactivatedHolidaySceneryGroups()
    // port: bo/util/ServerInformationVO#setDisactivatedHolidaySceneryGroups(List)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disactivated_holiday_scenery_groups: Option<Vec<String>>,
    // port: bo/util/ServerInformationVO#getDiscordUrl()
    // port: bo/util/ServerInformationVO#setDiscordUrl(String)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discord_url: Option<String>,
    // port: bo/util/ServerInformationVO#getFacebookUrl()
    // port: bo/util/ServerInformationVO#setFacebookUrl(String)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facebook_url: Option<String>,
    // port: bo/util/ServerInformationVO#getHomePageUrl()
    // port: bo/util/ServerInformationVO#setHomePageUrl(String)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home_page_url: Option<String>,
    // port: bo/util/ServerInformationVO#getMessageSrv()
    // port: bo/util/ServerInformationVO#setMessageSrv(String)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_srv: Option<String>,
    // port: bo/util/ServerInformationVO#getNumberOfRegistered()
    // port: bo/util/ServerInformationVO#setNumberOfRegistered(Long)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number_of_registered: Option<i64>,
    // port: bo/util/ServerInformationVO#getOnlineNumber()
    // port: bo/util/ServerInformationVO#setOnlineNumber(Long)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub online_number: Option<i64>,
    // port: bo/util/ServerInformationVO#getOwnerList()
    // port: bo/util/ServerInformationVO#setOwnerList(String)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_list: Option<String>,
    // port: bo/util/ServerInformationVO#isRequireTicket()
    // port: bo/util/ServerInformationVO#setRequireTicket(boolean)
    pub require_ticket: bool,
    // port: bo/util/ServerInformationVO#getSecondsToShutDown()
    // port: bo/util/ServerInformationVO#setSecondsToShutDown(Integer)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds_to_shut_down: Option<i32>,
    // port: bo/util/ServerInformationVO#getServerName()
    // port: bo/util/ServerInformationVO#setServerName(String)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    // port: bo/util/ServerInformationVO#getServerVersion()
    // port: bo/util/ServerInformationVO#setServerVersion(String)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_version: Option<String>,
    // port: bo/util/ServerInformationVO#getTimezone()
    // port: bo/util/ServerInformationVO#setTimezone(Integer)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<i32>,
}
