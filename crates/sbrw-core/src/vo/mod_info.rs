//! `ModInfoVO`: where the launcher's ModNet finds this server's mods, as JSON.

use serde::Serialize;

/// Serialised with JSON-B: alphabetical, nulls left out.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ModInfoVO {
    // port: vo/ModInfoVO#getBasePath()
    // port: vo/ModInfoVO#setBasePath(String)
    #[serde(rename = "basePath", skip_serializing_if = "Option::is_none")]
    pub base_path: Option<String>,
    // port: vo/ModInfoVO#getFeatures()
    // port: vo/ModInfoVO#setFeatures(List)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<Vec<String>>,
    // port: vo/ModInfoVO#getServerID()
    // port: vo/ModInfoVO#setServerID(String)
    #[serde(rename = "serverID", skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
}
