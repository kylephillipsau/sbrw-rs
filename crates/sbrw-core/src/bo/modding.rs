//! `ModdingBO`: where the launcher's ModNet finds this server's mods.

use crate::World;
use crate::vo::ModInfoVO;

// port: bo/ModdingBO#getModInfo()
/// `None` when `MODDING_ENABLED` isn't true, which the launcher reads as
/// ModNet off.
pub fn get_mod_info(w: &World) -> Option<ModInfoVO> {
    if !w.params.get_bool_param("MODDING_ENABLED") {
        return None;
    }
    Some(ModInfoVO {
        server_id: w.params.get_str_param("MODDING_SERVER_ID"),
        base_path: w.params.get_str_param("MODDING_BASE_PATH"),
        features: Some(w.params.get_str_list_param("MODDING_FEATURES")),
    })
}
