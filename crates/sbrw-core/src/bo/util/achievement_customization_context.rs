//! `AchievementCustomizationContext`: what CUSTOMIZATION achievement scripts
//! see about a visit to the shops. No achievement in the data has that
//! category, so no script reads it yet.

use anyhow::anyhow;
use sbrw_xml::dto::http::{
    CustomPaintTrans, CustomVinylTrans, PerformancePartTrans, SkillModPartTrans, VisualPartTrans,
};

use crate::Fault;
use crate::dao::product::ProductEntity;

/// `AchievementCustomizationContext.Type`, in declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    Skills,
    Perf,
    Vinyls,
    Aftermarket,
    Paint,
}

impl Type {
    /// The constant's name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Skills => "SKILLS",
            Self::Perf => "PERF",
            Self::Vinyls => "VINYLS",
            Self::Aftermarket => "AFTERMARKET",
            Self::Paint => "PAINT",
        }
    }
}

/// `WrappedPart`: a part, and the product it is.
#[derive(Debug, Clone, PartialEq)]
pub struct WrappedPart<T> {
    pub part: T,
    pub product_entity: ProductEntity,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AchievementCustomizationContext {
    r#type: Type,
    paints_added: Option<Vec<CustomPaintTrans>>,
    performance_parts_added: Option<Vec<WrappedPart<PerformancePartTrans>>>,
    visual_parts_added: Option<Vec<VisualPartTrans>>,
    skill_mod_parts_added: Option<Vec<WrappedPart<SkillModPartTrans>>>,
    vinyls_added: Option<Vec<CustomVinylTrans>>,
}

/// A list's getter and setter, which only its context type may use.
macro_rules! accessors {
    ($get:ident, $set:ident, $field:ident, $ty:ty, $kind:expr) => {
        pub fn $get(&self) -> Result<Option<&[$ty]>, Fault> {
            self.check_type($kind)?;
            Ok(self.$field.as_deref())
        }

        pub fn $set(&mut self, value: Vec<$ty>) -> Result<(), Fault> {
            self.check_type($kind)?;
            self.$field = Some(value);
            Ok(())
        }
    };
}

// port: bo/util/AchievementCustomizationContext#getPaintsAdded()
// port: bo/util/AchievementCustomizationContext#setPaintsAdded(Collection)
// port: bo/util/AchievementCustomizationContext#getPerformancePartsAdded()
// port: bo/util/AchievementCustomizationContext#setPerformancePartsAdded(Collection)
// port: bo/util/AchievementCustomizationContext#getVisualPartsAdded()
// port: bo/util/AchievementCustomizationContext#setVisualPartsAdded(Collection)
// port: bo/util/AchievementCustomizationContext#getSkillModPartsAdded()
// port: bo/util/AchievementCustomizationContext#setSkillModPartsAdded(Collection)
// port: bo/util/AchievementCustomizationContext#getVinylsAdded()
// port: bo/util/AchievementCustomizationContext#setVinylsAdded(Collection)
impl AchievementCustomizationContext {
    // port: bo/util/AchievementCustomizationContext#<init>(Type)
    pub fn new(r#type: Type) -> Self {
        Self {
            r#type,
            paints_added: None,
            performance_parts_added: None,
            visual_parts_added: None,
            skill_mod_parts_added: None,
            vinyls_added: None,
        }
    }

    // port: bo/util/AchievementCustomizationContext#getTypeVal()
    /// The type's ordinal.
    pub fn type_val(&self) -> i32 {
        self.r#type as i32
    }

    // port: bo/util/AchievementCustomizationContext#getType()
    pub fn r#type(&self) -> Type {
        self.r#type
    }

    // port: bo/util/AchievementCustomizationContext#countPerfPartsWithRating(Integer)
    // port: bo/util/AchievementCustomizationContext#countPerfPartsWithRating(Integer,boolean)
    /// Added performance parts of a rarity (`exact`) or at least it; upstream's
    /// one-argument overload is exact.
    pub fn count_perf_parts_with_rating(&self, rating: Option<i32>, exact: bool) -> Result<i64, Fault> {
        count_parts_with_rating(self.performance_parts_added.as_deref(), rating, exact)
    }

    // port: bo/util/AchievementCustomizationContext#countSkillModsWithRating(Integer)
    // port: bo/util/AchievementCustomizationContext#countSkillModsWithRating(Integer,boolean)
    pub fn count_skill_mods_with_rating(&self, rating: Option<i32>, exact: bool) -> Result<i64, Fault> {
        count_parts_with_rating(self.skill_mod_parts_added.as_deref(), rating, exact)
    }

    // port: bo/util/AchievementCustomizationContext#countPerfPartsWithRatingAndType(Integer,String)
    // port: bo/util/AchievementCustomizationContext#countPerfPartsWithRatingAndType(Integer,String,boolean)
    pub fn count_perf_parts_with_rating_and_type(
        &self,
        rating: Option<i32>,
        sub_type: Option<&str>,
        exact: bool,
    ) -> Result<i64, Fault> {
        count_parts_with_rating_and_type(self.performance_parts_added.as_deref(), rating, sub_type, exact)
    }

    // port: bo/util/AchievementCustomizationContext#countSkillModsWithRatingAndType(Integer,String)
    // port: bo/util/AchievementCustomizationContext#countSkillModsWithRatingAndType(Integer,String,boolean)
    pub fn count_skill_mods_with_rating_and_type(
        &self,
        rating: Option<i32>,
        sub_type: Option<&str>,
        exact: bool,
    ) -> Result<i64, Fault> {
        count_parts_with_rating_and_type(self.skill_mod_parts_added.as_deref(), rating, sub_type, exact)
    }

    // port: bo/util/AchievementCustomizationContext#checkType(Type)
    /// Each list belongs to one type; reaching for another is an error.
    fn check_type(&self, expected: Type) -> Result<(), Fault> {
        if self.r#type != expected {
            return Err(anyhow!("UnsupportedOperationException: Context type is not {}", expected.name()).into());
        }
        Ok(())
    }

    accessors!(paints_added, set_paints_added, paints_added, CustomPaintTrans, Type::Paint);
    accessors!(
        performance_parts_added,
        set_performance_parts_added,
        performance_parts_added,
        WrappedPart<PerformancePartTrans>,
        Type::Perf
    );
    accessors!(visual_parts_added, set_visual_parts_added, visual_parts_added, VisualPartTrans, Type::Aftermarket);
    accessors!(
        skill_mod_parts_added,
        set_skill_mod_parts_added,
        skill_mod_parts_added,
        WrappedPart<SkillModPartTrans>,
        Type::Skills
    );
    accessors!(vinyls_added, set_vinyls_added, vinyls_added, CustomVinylTrans, Type::Vinyls);
}

// port: bo/util/AchievementCustomizationContext#countPartsWithRating(Collection,Integer)
// port: bo/util/AchievementCustomizationContext#countPartsWithRating(Collection,Integer,boolean)
/// Parts whose product has the rarity (`exact`) or at least it; a part
/// without a rarity never counts. The list is null for another context type.
pub fn count_parts_with_rating<T>(
    collection: Option<&[WrappedPart<T>]>,
    rating: Option<i32>,
    exact: bool,
) -> Result<i64, Fault> {
    let rating = rating.ok_or_else(|| Fault::null_pointer("rating"))?;
    let collection = collection.ok_or_else(|| Fault::null_pointer("the parts"))?;
    Ok(collection
        .iter()
        .filter(|p| p.product_entity.rarity.is_some_and(|r| if exact { r == rating } else { rating <= r }))
        .count() as i64)
}

// port: bo/util/AchievementCustomizationContext#countPartsWithRatingAndType(Collection,Integer,String)
// port: bo/util/AchievementCustomizationContext#countPartsWithRatingAndType(Collection,Integer,String,boolean)
/// As [`count_parts_with_rating`], among parts of a sub-type (ignoring case).
pub fn count_parts_with_rating_and_type<T>(
    collection: Option<&[WrappedPart<T>]>,
    rating: Option<i32>,
    sub_type: Option<&str>,
    exact: bool,
) -> Result<i64, Fault> {
    let rating = rating.ok_or_else(|| Fault::null_pointer("rating"))?;
    let sub_type = sub_type.ok_or_else(|| Fault::null_pointer("subType"))?;
    let collection = collection.ok_or_else(|| Fault::null_pointer("the parts"))?;
    Ok(collection
        .iter()
        .filter(|p| p.product_entity.sub_type.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(sub_type)))
        .filter(|p| p.product_entity.rarity.is_some_and(|r| if exact { r == rating } else { rating <= r }))
        .count() as i64)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::dao::product;
    use crate::store;

    fn wrapped(product_id: &str) -> WrappedPart<PerformancePartTrans> {
        let db = store::open(None, &BTreeMap::new()).unwrap();
        let product_entity = product::find_by_product_id(&db, product_id).unwrap();
        let part = PerformancePartTrans { performance_part_attrib_hash: product_entity.hash.unwrap() };
        WrappedPart { part, product_entity }
    }

    #[test]
    fn counts_added_parts_by_rarity_and_type() {
        let mut ctx = AchievementCustomizationContext::new(Type::Perf);
        assert_eq!(ctx.type_val(), 1);
        // Engines of rarity 1 and 4 (from grey to elite), and a race engine of rarity 1.
        let parts = vec![wrapped("SRV-PERF-2EB805EA"), wrapped("SRV-PERF-B0F38419"), wrapped("SRV-PERF-DB2240AC")];
        let rarities: Vec<_> = parts.iter().map(|p| p.product_entity.rarity).collect();
        ctx.set_performance_parts_added(parts).unwrap();
        assert_eq!(ctx.performance_parts_added().unwrap().map(<[_]>::len), Some(3));
        let at = |r: i32, exact| ctx.count_perf_parts_with_rating(Some(r), exact).unwrap();
        let expected = |r: i32, exact: bool| {
            rarities.iter().filter(|x| x.is_some_and(|x| if exact { x == r } else { r <= x })).count() as i64
        };
        for r in 0..=5 {
            assert_eq!(at(r, true), expected(r, true));
            assert_eq!(at(r, false), expected(r, false));
        }
        assert_eq!(ctx.count_perf_parts_with_rating_and_type(Some(1), Some("PPART_ENGINE"), false).unwrap(), 3);
        assert_eq!(ctx.count_perf_parts_with_rating_and_type(Some(1), Some("ppart_tires"), false).unwrap(), 0);
        assert!(ctx.count_perf_parts_with_rating(None, true).is_err());
        assert!(ctx.count_skill_mods_with_rating(Some(1), true).is_err(), "a PERF context has no skill mods");
        assert!(ctx.paints_added().is_err());
        assert!(ctx.set_vinyls_added(Vec::new()).is_err());
    }
}
