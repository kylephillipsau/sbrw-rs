//! `AchievementTransaction`: achievement updates gathered for one persona, then
//! committed together by `AchievementBO.commitTransaction`.

use std::collections::BTreeMap;

use anyhow::anyhow;
use sbrw_java::util::hash_map_order;

use crate::Fault;
use crate::bo::util::achievement_commerce_context::AchievementCommerceContext;
use crate::bo::util::achievement_progression_context::AchievementProgressionContext;
use crate::dao::car::CarEntity;
use crate::dao::persona::PersonaEntity;
use crate::dao::persona_achievement::PersonaAchievementEntity;

/// A script global, as upstream's `Map<String, Object>` held it. Upstream holds
/// the live entities; a binding here is a copy, taken when nothing the scripts
/// read can change before they run.
#[derive(Debug, Clone, PartialEq)]
pub enum Binding {
    Persona(PersonaEntity),
    PersonaAchievement(PersonaAchievementEntity),
    Progression(AchievementProgressionContext),
    OwnedCar(Box<CarEntity>),
    CommerceCtx(Box<AchievementCommerceContext>),
}

/// A script's globals by name.
pub type Bindings = BTreeMap<String, Binding>;

#[derive(Debug, Clone, PartialEq)]
pub struct AchievementTransaction {
    persona_id: i64,
    /// Category -> the globals of each update, by first use of the category.
    entries: Vec<(String, Vec<Bindings>)>,
    committed: bool,
}

impl AchievementTransaction {
    // port: bo/AchievementTransaction#<init>(Long)
    pub fn new(persona_id: i64) -> Self {
        Self { persona_id, entries: Vec::new(), committed: false }
    }

    // port: bo/AchievementTransaction#add(String,Map)
    pub fn add(&mut self, category: &str, parameters: Bindings) -> Result<(), Fault> {
        if self.committed {
            return Err(anyhow!("add() is not permitted for committed transactions.").into());
        }
        match self.entries.iter_mut().find(|(c, _)| c == category) {
            Some((_, list)) => list.push(parameters),
            None => self.entries.push((category.to_owned(), vec![parameters])),
        }
        Ok(())
    }

    // port: bo/AchievementTransaction#getPersonaId()
    pub fn persona_id(&self) -> i64 {
        self.persona_id
    }

    // port: bo/AchievementTransaction#getEntries()
    /// By category, in the order upstream's `HashMap` iterates them.
    pub fn entries(&self) -> Vec<(&str, &[Bindings])> {
        let categories: Vec<&str> = self.entries.iter().map(|(c, _)| c.as_str()).collect();
        hash_map_order(&categories).into_iter().map(|i| (categories[i], self.entries[i].1.as_slice())).collect()
    }

    // port: bo/AchievementTransaction#isCommitted()
    pub fn is_committed(&self) -> bool {
        self.committed
    }

    // port: bo/AchievementTransaction#markCommitted()
    pub fn mark_committed(&mut self) -> Result<(), Fault> {
        if self.committed {
            return Err(anyhow!("Attempted to commit the same transaction more than once!").into());
        }
        self.committed = true;
        Ok(())
    }

    // port: bo/AchievementTransaction#clear()
    pub fn clear(&mut self) -> Result<(), Fault> {
        if !self.committed {
            return Err(anyhow!("Non-committed transactions cannot be cleared").into());
        }
        self.entries.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn persona(id: i64) -> Bindings {
        Bindings::from([(
            "persona".to_owned(),
            Binding::Persona(PersonaEntity { persona_id: id, ..Default::default() }),
        )])
    }

    #[test]
    fn entries_iterate_as_upstreams_hash_map() {
        let mut t = AchievementTransaction::new(100);
        for category in ["PROGRESSION", "LOGIN", "COMMERCE", "LOGIN", "EVENT"] {
            t.add(category, persona(100)).unwrap();
        }
        let order: Vec<(&str, usize)> = t.entries().into_iter().map(|(c, list)| (c, list.len())).collect();
        // JDK 11 iterates COMMERCE, LOGIN, then PROGRESSION and EVENT (one bucket, by insertion).
        assert_eq!(order, [("COMMERCE", 1), ("LOGIN", 2), ("PROGRESSION", 1), ("EVENT", 1)]);
    }

    #[test]
    fn commits_once_then_rejects_adds() {
        let mut t = AchievementTransaction::new(100);
        t.add("LOGIN", persona(100)).unwrap();
        assert!(t.clear().is_err());
        t.mark_committed().unwrap();
        assert!(t.is_committed());
        assert!(t.mark_committed().is_err());
        assert!(t.add("LOGIN", persona(100)).is_err());
        t.clear().unwrap();
        assert!(t.entries().is_empty());
    }
}
