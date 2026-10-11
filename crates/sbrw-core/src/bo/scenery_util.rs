//! `SceneryUtil`: the holiday scenery groups the game knows, by id.

const SCENERY_IDS: &[(&str, i64)] = &[
    ("SCENERY_GROUP_NORMAL", 0),
    ("SCENERY_GROUP_OKTOBERFEST", 1),
    ("SCENERY_GROUP_HALLOWEEN", 2),
    ("SCENERY_GROUP_CHRISTMAS", 3),
    ("SCENERY_GROUP_NEWYEARS", 5),
];

// port: bo/SceneryUtil#getSceneryId(String)
/// 0, like the normal scenery, for a group the game doesn't know.
pub fn get_scenery_id(scenery: &str) -> i64 {
    SCENERY_IDS.iter().find(|(name, _)| *name == scenery).map_or(0, |&(_, id)| id)
}

// port: bo/SceneryUtil#isValid(String)
pub fn is_valid(scenery: &str) -> bool {
    SCENERY_IDS.iter().any(|(name, _)| *name == scenery)
}
