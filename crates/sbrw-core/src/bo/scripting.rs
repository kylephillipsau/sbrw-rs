//! `ScriptingBO`: the JavaScript in upstream's game data (achievement triggers
//! and values, reward scripts), which upstream ran on Nashorn. `sbrw-script`
//! runs it; this module shows each binding to scripts as the Java object
//! Nashorn saw, through the getters of upstream's class.
//!
//! Relationship getters (a car's persona, a persona's user, a car's part
//! sets) and `PersonaEntity.getDaysSinceFirstLogin()` aren't shown: no script
//! in the data reads them.

use sbrw_script::{Bean, JavaValue};
pub use sbrw_script::{ScriptException, ScriptValue};
use time::PrimitiveDateTime;

use crate::Fault;
use crate::bo::achievement_transaction::{Binding, Bindings};
use crate::bo::util::achievement_commerce_context::AchievementCommerceContext;
use crate::bo::util::achievement_progression_context::AchievementProgressionContext;
use crate::dao::car::CarEntity;
use crate::dao::car_classes::CarClassesEntity;
use crate::dao::persona::PersonaEntity;
use crate::dao::persona_achievement::PersonaAchievementEntity;
use crate::dao::product::ProductEntity;

// port: bo/ScriptingBO#init()
// port: bo/ScriptingBO#eval(String,Map)
/// `ScriptingBO.eval(script, bindings)`: the script's result, or the
/// `ScriptException` callers catch. The engine starts with the first script.
pub fn eval(script: &str, bindings: &Bindings) -> Result<Result<ScriptValue, ScriptException>, Fault> {
    let globals: Vec<(&str, JavaValue)> = bindings.iter().map(|(name, b)| (name.as_str(), java(b))).collect();
    Ok(sbrw_script::eval(script, &globals))
}

fn java(binding: &Binding) -> JavaValue {
    match binding {
        Binding::Persona(p) => persona(p),
        Binding::PersonaAchievement(pa) => persona_achievement(pa),
        Binding::Progression(p) => progression(p),
        Binding::OwnedCar(c) => car(c),
        Binding::CommerceCtx(c) => commerce_context(c),
    }
}

fn opt<T>(value: Option<T>, f: impl FnOnce(T) -> JavaValue) -> JavaValue {
    value.map_or(JavaValue::Null, f)
}

fn string(value: &Option<String>) -> JavaValue {
    opt(value.clone(), JavaValue::String)
}

/// A `LocalDateTime`: not null, and `toString()` as Java prints it.
fn local_date_time(value: Option<PrimitiveDateTime>) -> JavaValue {
    opt(value, |t| JavaValue::Object(sbrw_java::time::local_date_time_to_string(t)))
}

fn persona(p: &PersonaEntity) -> JavaValue {
    JavaValue::Bean(
        Bean::new()
            .get("boost", JavaValue::Double(p.boost))
            .get("cash", JavaValue::Double(p.cash))
            .get("iconIndex", JavaValue::Integer(p.icon_index))
            .get("level", JavaValue::Integer(p.level))
            .get("motto", string(&p.motto))
            .get("name", string(&p.name))
            .get("percentToLevel", JavaValue::Float(p.percent_to_level))
            .get("personaId", JavaValue::Long(p.persona_id))
            .get("rating", JavaValue::Double(p.rating))
            .get("rep", JavaValue::Double(p.rep))
            .get("repAtCurrentLevel", JavaValue::Integer(p.rep_at_current_level))
            .get("score", JavaValue::Integer(p.score))
            .get("curCarIndex", JavaValue::Integer(p.cur_car_index))
            .get("created", local_date_time(p.created))
            .get("firstLogin", local_date_time(p.first_login))
            .get("lastLogin", local_date_time(p.last_login)),
    )
}

fn persona_achievement(pa: &PersonaAchievementEntity) -> JavaValue {
    JavaValue::Bean(
        Bean::new()
            .get("id", JavaValue::Long(pa.id))
            .is("canProgress", pa.can_progress)
            .get("currentValue", opt(pa.current_value, JavaValue::Long)),
    )
}

// port: bo/util/AchievementProgressionContext#getCash()
// port: bo/util/AchievementProgressionContext#getRep()
// port: bo/util/AchievementProgressionContext#getLevel()
// port: bo/util/AchievementProgressionContext#getScore()
// port: bo/util/AchievementProgressionContext#isLevelChanged()
// port: bo/util/AchievementProgressionContext#isInEvent()
// port: bo/util/AchievementProgressionContext#isScoreChanged()
// port: bo/util/AchievementProgressionContext#isTreasureStreakChanged()
// port: bo/util/AchievementProgressionContext#getTreasureStreak()
fn progression(p: &AchievementProgressionContext) -> JavaValue {
    JavaValue::Bean(
        Bean::new()
            .get("cash", opt(p.cash, JavaValue::Integer))
            .get("rep", opt(p.rep, JavaValue::Integer))
            .get("level", opt(p.level, JavaValue::Integer))
            .get("score", opt(p.score, JavaValue::Integer))
            .is("levelChanged", p.level_changed)
            .is("inEvent", p.in_event)
            .is("scoreChanged", p.score_changed)
            .is("treasureStreakChanged", p.treasure_streak_changed)
            .get("treasureStreak", opt(p.treasure_streak, JavaValue::Integer)),
    )
}

fn car(c: &CarEntity) -> JavaValue {
    JavaValue::Bean(
        Bean::new()
            .get("id", JavaValue::Long(c.id))
            .get("durability", JavaValue::Integer(c.durability))
            .get("heat", JavaValue::Float(c.heat))
            .get("expirationDate", local_date_time(c.expiration_date))
            .get("ownershipType", string(&c.ownership_type))
            .get("baseCar", JavaValue::Integer(c.base_car))
            .get("carClassHash", JavaValue::Integer(c.car_class_hash))
            .is("preset", c.is_preset)
            .get("level", JavaValue::Integer(c.level))
            .get("name", string(&c.name))
            .get("physicsProfileHash", JavaValue::Integer(c.physics_profile_hash))
            .get("rating", JavaValue::Integer(c.rating))
            .get("resalePrice", JavaValue::Float(c.resale_price))
            .get("rideHeightDrop", JavaValue::Float(c.ride_height_drop))
            .get("skillModSlotCount", JavaValue::Integer(c.skill_mod_slot_count))
            .get("version", JavaValue::Integer(c.version)),
    )
}

// port: bo/util/AchievementCommerceContext#getCarClassesEntity()
// port: bo/util/AchievementCommerceContext#getProductEntity()
// port: bo/util/AchievementCommerceContext#getCommerceType()
fn commerce_context(c: &AchievementCommerceContext) -> JavaValue {
    JavaValue::Bean(
        Bean::new()
            .get("carClassesEntity", opt(c.car_classes_entity.as_ref(), car_classes))
            .get("productEntity", opt(c.product_entity.as_ref(), product))
            .get("commerceType", JavaValue::Enum(c.commerce_type.name().to_owned()))
            .get("commerceTypeVal", JavaValue::Integer(c.commerce_type_val())),
    )
}

fn car_classes(c: &CarClassesEntity) -> JavaValue {
    let int = |v: Option<i32>| opt(v, JavaValue::Integer);
    JavaValue::Bean(
        Bean::new()
            .get("storeName", JavaValue::String(c.store_name.clone()))
            .get("fullName", string(&c.full_name))
            .get("manufactor", string(&c.manufactor))
            .get("model", string(&c.model))
            .get("tsStock", int(c.ts_stock))
            .get("tsVar1", int(c.ts_var1))
            .get("tsVar2", int(c.ts_var2))
            .get("tsVar3", int(c.ts_var3))
            .get("acStock", int(c.ac_stock))
            .get("acVar1", int(c.ac_var1))
            .get("acVar2", int(c.ac_var2))
            .get("acVar3", int(c.ac_var3))
            .get("haStock", int(c.ha_stock))
            .get("haVar1", int(c.ha_var1))
            .get("haVar2", int(c.ha_var2))
            .get("haVar3", int(c.ha_var3))
            .get("hash", int(c.hash))
            .get("productId", string(&c.product_id)),
    )
}

fn product(p: &ProductEntity) -> JavaValue {
    let int = |v: Option<i32>| opt(v, JavaValue::Integer);
    JavaValue::Bean(
        Bean::new()
            .get("id", JavaValue::Long(p.id))
            .get("categoryId", string(&p.category_id))
            .get("currency", JavaValue::String(p.currency.clone()))
            .get("description", string(&p.description))
            .get("durationMinute", JavaValue::Integer(p.duration_minute))
            .get("hash", int(p.hash))
            .get("icon", JavaValue::String(p.icon.clone()))
            .get("level", JavaValue::Integer(p.level))
            .get("longDescription", string(&p.long_description))
            .get("price", JavaValue::Float(p.price))
            .get("priority", JavaValue::Integer(p.priority))
            .get("productId", JavaValue::String(p.product_id.clone()))
            .get("productTitle", string(&p.product_title))
            .get("productType", JavaValue::String(p.product_type.clone()))
            .get("secondaryIcon", string(&p.secondary_icon))
            .get("useCount", JavaValue::Integer(p.use_count))
            .get("visualStyle", string(&p.visual_style))
            .get("webIcon", string(&p.web_icon))
            .get("webLocation", string(&p.web_location))
            .get("categoryName", string(&p.category_name))
            .is("enabled", p.enabled)
            .get("minLevel", JavaValue::Integer(p.min_level))
            .is("premium", p.premium)
            .is("dropable", p.is_dropable)
            .get("resalePrice", JavaValue::Float(p.resale_price))
            .get("topSpeed", int(p.top_speed))
            .get("accel", int(p.accel))
            .get("handling", int(p.handling))
            .get("skillValue", opt(p.skill_value, JavaValue::Float))
            .get("entitlementTag", string(&p.entitlement_tag))
            .get("subType", string(&p.sub_type))
            .get("brand", string(&p.brand))
            .get("rarity", int(p.rarity))
            .get("dropWeight", opt(p.drop_weight, JavaValue::Double)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bo::util::achievement_commerce_context::CommerceType;
    use serde::Deserialize;
    use time::macros::datetime;

    #[derive(Deserialize)]
    struct Vectors {
        cases: Vec<Case>,
    }

    #[derive(Deserialize)]
    struct Case {
        bindings: String,
        script: String,
        class: Option<String>,
        value: Option<String>,
        error: Option<String>,
    }

    /// Nashorn results that differ here, as `sbrw-script` documents; no
    /// script in the data does any of these.
    const KNOWN_DIFFERENCES: [&str; 4] = ["typeof l5", "l5 === 5", "l5 === i5", "ownedCar.isPreset"];

    fn persona() -> JavaValue {
        java(&Binding::Persona(PersonaEntity {
            persona_id: 100,
            name: Some("DRIVER1".into()),
            level: 60,
            score: 27,
            cash: 350000.0,
            ..Default::default()
        }))
    }

    fn car(expiration_date: Option<PrimitiveDateTime>) -> JavaValue {
        java(&Binding::OwnedCar(Box::new(CarEntity {
            id: 1,
            name: Some("challenger71".into()),
            is_preset: true,
            expiration_date,
            ..Default::default()
        })))
    }

    fn car_purchase(manufactor: &str) -> JavaValue {
        let mut ctx = AchievementCommerceContext::new(CommerceType::CarPurchase);
        ctx.set_car_classes_entity(CarClassesEntity { manufactor: Some(manufactor.into()), ..Default::default() });
        java(&Binding::CommerceCtx(Box::new(ctx)))
    }

    fn bundle_purchase() -> JavaValue {
        java(&Binding::CommerceCtx(Box::new(AchievementCommerceContext::new(CommerceType::BundlePurchase))))
    }

    fn persona_achievement(current_value: i64) -> JavaValue {
        java(&Binding::PersonaAchievement(PersonaAchievementEntity {
            can_progress: true,
            current_value: Some(current_value),
            ..Default::default()
        }))
    }

    #[allow(clippy::too_many_arguments)]
    fn progression(
        cash: i32,
        level: i32,
        score: i32,
        streak: i32,
        level_changed: bool,
        streak_changed: bool,
        in_event: bool,
    ) -> JavaValue {
        java(&Binding::Progression(AchievementProgressionContext::new(
            Some(cash),
            Some(0),
            Some(level),
            Some(score),
            Some(streak),
            level_changed,
            true,
            streak_changed,
            in_event,
        )))
    }

    /// The binding sets of `NashornVectors.java`.
    fn binding_set(name: &str) -> Vec<(&'static str, JavaValue)> {
        let rental = datetime!(2026-01-02 12:34:56);
        match name {
            "probe" => vec![
                ("l0", JavaValue::Long(0)),
                ("l5", JavaValue::Long(5)),
                ("l5b", JavaValue::Long(5)),
                ("i5", JavaValue::Integer(5)),
                ("d25", JavaValue::Double(2.5)),
                ("f15", JavaValue::Float(1.5)),
                ("s", JavaValue::String("DODGE".into())),
                ("n", JavaValue::Null),
                ("t", JavaValue::Boolean(true)),
                ("f", JavaValue::Boolean(false)),
                ("big", JavaValue::Long(9007199254740993)),
                ("persona", persona()),
                ("ownedCar", car(None)),
                ("rental", car(Some(rental))),
                ("commerceCtx", car_purchase("DODGE")),
                ("bundleCtx", bundle_purchase()),
                ("personaAchievement", persona_achievement(27)),
                ("progression", progression(100, 60, 27, 3, false, true, false)),
            ],
            "car_purchase" => vec![
                ("persona", persona()),
                ("ownedCar", car(None)),
                ("commerceCtx", car_purchase("DODGE")),
                ("personaAchievement", persona_achievement(0)),
            ],
            "rental_purchase" => vec![
                ("persona", persona()),
                ("ownedCar", car(Some(rental))),
                ("commerceCtx", car_purchase("DODGE")),
                ("personaAchievement", persona_achievement(3)),
            ],
            "bundle_purchase" => vec![
                ("persona", persona()),
                ("commerceCtx", bundle_purchase()),
                ("personaAchievement", persona_achievement(0)),
            ],
            "progression" => vec![
                ("persona", persona()),
                ("progression", progression(1500, 60, 27, 4, true, true, true)),
                ("personaAchievement", persona_achievement(1)),
            ],
            "progression_unchanged" => vec![
                ("persona", persona()),
                ("progression", progression(0, 60, 27, 0, false, false, false)),
                ("personaAchievement", persona_achievement(60)),
            ],
            other => panic!("unknown binding set {other}"),
        }
    }

    /// Whether our result is what Nashorn returned: numbers by value (upstream
    /// only uses their value), other Java objects by their string form.
    fn same(case: &Case, ours: &Result<ScriptValue, ScriptException>) -> bool {
        let number = |v: &ScriptValue| match v {
            ScriptValue::Integer(i) => Some(f64::from(*i)),
            ScriptValue::Long(l) => Some(*l as f64),
            ScriptValue::Float(f) => Some(f64::from(*f)),
            ScriptValue::Double(d) => Some(*d),
            _ => None,
        };
        let value = case.value.as_deref().unwrap_or_default();
        match (case.error.is_some(), case.class.as_deref(), ours) {
            (true, _, ours) => ours.is_err(),
            (false, _, Err(_)) => false,
            (false, None, Ok(v)) => *v == ScriptValue::Null,
            (false, Some("java.lang.Boolean"), Ok(v)) => *v == ScriptValue::Boolean(value == "true"),
            (false, Some("java.lang.String"), Ok(v)) => *v == ScriptValue::String(value.to_owned()),
            (false, Some("java.lang.Integer" | "java.lang.Long" | "java.lang.Float" | "java.lang.Double"), Ok(v)) => {
                let expected: f64 = value.parse().expect("a Java number");
                number(v).is_some_and(|n| n == expected || (n.is_nan() && expected.is_nan()))
            }
            (false, Some(_), Ok(ScriptValue::String(s) | ScriptValue::Object(s))) => s == value,
            (false, Some(_), Ok(_)) => false,
        }
    }

    #[test]
    fn scripts_evaluate_as_nashorn_did() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/vectors/nashorn.json");
        let vectors: Vectors = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut failures = Vec::new();
        let mut known = Vec::new();
        for case in &vectors.cases {
            let bindings = binding_set(&case.bindings);
            let ours = sbrw_script::eval(&case.script, &bindings);
            let ok = same(case, &ours);
            if KNOWN_DIFFERENCES.contains(&case.script.as_str()) && case.bindings == "probe" {
                assert!(!ok, "{} now matches Nashorn: take it off KNOWN_DIFFERENCES", case.script);
                known.push(&case.script);
            } else if !ok {
                failures.push(format!(
                    "[{}] {}: Nashorn {:?} {:?} {:?}, sbrw-script {ours:?}",
                    case.bindings, case.script, case.class, case.value, case.error
                ));
            }
        }
        assert!(failures.is_empty(), "{} of {} differ:\n{}", failures.len(), vectors.cases.len(), failures.join("\n"));
        assert_eq!(known.len(), KNOWN_DIFFERENCES.len());
    }

    #[test]
    fn bindings_reach_scripts_by_name() {
        let bindings = Bindings::from([(
            "persona".to_owned(),
            Binding::Persona(PersonaEntity { level: 60, ..Default::default() }),
        )]);
        assert_eq!(
            eval("persona.level == 60 && persona.getLevel() == 60", &bindings).unwrap(),
            Ok(ScriptValue::Boolean(true))
        );
        assert!(eval("nobody.level", &bindings).unwrap().is_err());
    }
}
