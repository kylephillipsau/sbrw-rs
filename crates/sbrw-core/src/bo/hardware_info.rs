//! `HardwareInfoBO`: the hardware a game client reports, stored once per
//! hash, and hardware bans.

use sbrw_xml::ToXml;
use sbrw_xml::dto::http::HardwareInfo;
use sha1::{Digest, Sha1};

use crate::dao::hardware_info::{self, HardwareInfoEntity};
use crate::{Fault, World};

// port: bo/HardwareInfoBO#calcHardwareInfoHash(String)
/// The SHA-1 hex of the XML, or `empty`.
pub fn calc_hardware_info_hash(hardware_info_xml: Option<&str>) -> String {
    match hardware_info_xml {
        Some(xml) if !xml.is_empty() => hex::encode(Sha1::digest(xml.as_bytes())),
        _ => "empty".into(),
    }
}

// port: bo/HardwareInfoBO#save(HardwareInfo)
/// Store the reported hardware, without the fields that vary between runs of
/// the same machine, under the hash of its XML; an existing row for the hash
/// is reassigned to the reporting user.
pub fn save(w: &World, mut hardware_info: HardwareInfo) -> Result<HardwareInfoEntity, Fault> {
    let user_id = hardware_info.user_id;

    hardware_info.available_mem = 0;
    hardware_info.cpuid10 = 0;
    hardware_info.cpuid11 = 0;
    hardware_info.cpuid12 = 0;
    hardware_info.cpuid13 = 0;
    hardware_info.user_id = 0;
    let hardware_info_xml = hardware_info.to_xml();
    let calc_hardware_info_hash = calc_hardware_info_hash(Some(&hardware_info_xml));
    match hardware_info::find_by_hardware_hash(w.db(), &calc_hardware_info_hash)? {
        None => {
            let mut entity = HardwareInfoEntity {
                user_id: Some(user_id),
                hardware_info: Some(hardware_info_xml),
                hardware_hash: Some(calc_hardware_info_hash),
                ..Default::default()
            };
            hardware_info::insert(w.db(), &mut entity)?;
            Ok(entity)
        }
        Some(mut entity) => {
            entity.user_id = Some(user_id);
            hardware_info::update(w.db(), &entity)?;
            Ok(entity)
        }
    }
}

// port: bo/HardwareInfoBO#isHardwareHashBanned(String)
pub fn is_hardware_hash_banned(w: &World, hardware_hash: &str) -> Result<bool, Fault> {
    Ok(hardware_info::find_by_hardware_hash(w.db(), hardware_hash)?.is_some_and(|h| h.banned))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_the_xml_or_says_empty() {
        assert_eq!(calc_hardware_info_hash(Some("abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(calc_hardware_info_hash(Some("")), "empty");
        assert_eq!(calc_hardware_info_hash(None), "empty");
    }

    #[test]
    fn one_row_per_hash_reassigned_to_the_latest_user() {
        let w = World::test(&[]);
        let report = |user_id, available_mem| HardwareInfo {
            cpuid0: Some("GenuineIntel".into()),
            user_id,
            available_mem,
            ..Default::default()
        };
        let first = save(&w, report(1, 100)).unwrap();
        w.db().execute("UPDATE hardware_info SET banned = 1", []).unwrap();
        let again = save(&w, report(7, 200)).unwrap();
        assert_eq!((again.id, again.user_id), (first.id, Some(7)));
        assert!(is_hardware_hash_banned(&w, first.hardware_hash.as_deref().unwrap()).unwrap());
        let stored =
            hardware_info::find_by_hardware_hash(w.db(), first.hardware_hash.as_deref().unwrap()).unwrap().unwrap();
        assert!(stored.hardware_info.unwrap().contains("<availableMem>0</availableMem>"));
    }
}
