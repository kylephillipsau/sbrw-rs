//! `HardwareInfoBO`: the parts ported so far.

use crate::dao::hardware_info;
use crate::{Fault, World};

// port: bo/HardwareInfoBO#isHardwareHashBanned(String)
pub fn is_hardware_hash_banned(w: &World, hardware_hash: &str) -> Result<bool, Fault> {
    Ok(hardware_info::find_by_hardware_hash(w.db(), hardware_hash)?.is_some_and(|h| h.banned))
}
