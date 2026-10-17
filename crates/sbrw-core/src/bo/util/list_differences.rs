//! `ListDifferences`: what a list gained and lost, by the elements' `equals`.

use sbrw_xml::dto::http::{
    CustomPaintTrans, CustomVinylTrans, PerformancePartTrans, SkillModPartTrans, VisualPartTrans,
};

/// An element's `equals`. Upstream's part classes override it: a paint and
/// a vinyl compare every field, a skill mod its hash and whether it's fixed,
/// and a performance or visual part its hash alone (a visual part's slot
/// doesn't count).
pub trait JavaEquals {
    fn java_equals(&self, other: &Self) -> bool;
}

impl JavaEquals for CustomPaintTrans {
    fn java_equals(&self, other: &Self) -> bool {
        self == other
    }
}

impl JavaEquals for CustomVinylTrans {
    fn java_equals(&self, other: &Self) -> bool {
        self == other
    }
}

impl JavaEquals for PerformancePartTrans {
    fn java_equals(&self, other: &Self) -> bool {
        self.performance_part_attrib_hash == other.performance_part_attrib_hash
    }
}

impl JavaEquals for SkillModPartTrans {
    fn java_equals(&self, other: &Self) -> bool {
        self.is_fixed == other.is_fixed && self.skill_mod_part_attrib_hash == other.skill_mod_part_attrib_hash
    }
}

impl JavaEquals for VisualPartTrans {
    fn java_equals(&self, other: &Self) -> bool {
        self.part_hash == other.part_hash
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListDifferences<T> {
    pub kept: Vec<T>,
    pub added: Vec<T>,
    pub removed: Vec<T>,
}

impl<T: JavaEquals + Clone> ListDifferences<T> {
    // port: bo/util/ListDifferences#<init>(Collection,Collection,Collection)
    // port: bo/util/ListDifferences#getDifferences(Collection,Collection)
    // port: bo/util/ListDifferences#clone(Collection)
    /// What `second` kept of `first`, added and removed, by `retainAll` and
    /// `removeAll`: every copy of an element counts as the element, so a
    /// part added twice beside one kept isn't added at all.
    pub fn get_differences(first: &[T], second: &[T]) -> Self {
        let contains = |list: &[T], x: &T| list.iter().any(|y| x.java_equals(y));
        let already_there: Vec<T> = second.iter().filter(|x| contains(first, x)).cloned().collect();
        let added = second.iter().filter(|x| !contains(&already_there, x)).cloned().collect();
        let removed = first.iter().filter(|x| !contains(&already_there, x)).cloned().collect();
        Self { kept: already_there, added, removed }
    }

    // port: bo/util/ListDifferences#hasDifferences()
    pub fn has_differences(&self) -> bool {
        !self.added.is_empty() || !self.removed.is_empty()
    }
}

// port: bo/util/ListDifferences#getAdded()
// port: bo/util/ListDifferences#getKept()
// port: bo/util/ListDifferences#getRemoved()
// port: bo/util/ListDifferences#toString()
// The getters are the public fields; `toString` is `Debug`.

#[cfg(test)]
mod tests {
    use super::*;

    fn part(hash: i32) -> PerformancePartTrans {
        PerformancePartTrans { performance_part_attrib_hash: hash }
    }

    fn hashes(parts: &[PerformancePartTrans]) -> Vec<i32> {
        parts.iter().map(|p| p.performance_part_attrib_hash).collect()
    }

    #[test]
    fn finds_what_was_kept_added_and_removed() {
        let d = ListDifferences::get_differences(&[part(1), part(2)], &[part(2), part(3), part(3)]);
        assert_eq!(hashes(&d.kept), [2]);
        assert_eq!(hashes(&d.added), [3, 3]);
        assert_eq!(hashes(&d.removed), [1]);
        assert!(d.has_differences());
        assert!(!ListDifferences::get_differences(&[part(1)], &[part(1)]).has_differences());
    }

    #[test]
    fn a_copy_of_a_kept_part_is_not_added() {
        let d = ListDifferences::get_differences(&[part(1)], &[part(1), part(1)]);
        assert_eq!(hashes(&d.kept), [1, 1]);
        assert!(d.added.is_empty());
        assert!(d.removed.is_empty());
    }

    #[test]
    fn a_visual_part_compares_by_its_hash_alone() {
        let a = VisualPartTrans { part_hash: 5, slot_hash: 1 };
        let b = VisualPartTrans { part_hash: 5, slot_hash: 2 };
        assert!(!ListDifferences::get_differences(&[a], &[b]).has_differences());
        let fixed = SkillModPartTrans { is_fixed: true, skill_mod_part_attrib_hash: 7 };
        let loose = SkillModPartTrans { is_fixed: false, skill_mod_part_attrib_hash: 7 };
        assert!(ListDifferences::get_differences(&[fixed], &[loose]).has_differences());
    }
}
