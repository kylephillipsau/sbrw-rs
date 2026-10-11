//! `NewsArticleEntity` (an item in the game's news feed) and the parts of
//! `NewsArticleDAO` ported so far.

// port: dao/NewsArticleDAO#<init>()

use anyhow::anyhow;
use rusqlite::Connection;
use time::PrimitiveDateTime;

use crate::{Fault, datetime};

/// `NewsArticleFilters`: who a news item is for, stored by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewsArticleFilters {
    None,
    Me,
    Friends,
    System,
    ItemCount,
    Crew,
    All,
}

impl NewsArticleFilters {
    // port: jpa/util/NewsArticleFilters#getFilterMask()
    pub fn filter_mask(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Me => 1,
            Self::Friends => 2,
            Self::System => 4,
            Self::ItemCount => 6,
            Self::Crew => 8,
            Self::All => 2147483647,
        }
    }

    /// `NewsArticleFilters.valueOf(name)`.
    fn value_of(name: &str) -> Option<Self> {
        Some(match name {
            "None" => Self::None,
            "Me" => Self::Me,
            "Friends" => Self::Friends,
            "System" => Self::System,
            "Item_Count" => Self::ItemCount,
            "Crew" => Self::Crew,
            "All" => Self::All,
            _ => return None,
        })
    }
}

/// `NewsArticleType`, stored by name; the game knows it by id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewsArticleType {
    name: &'static str,
    type_id: i32,
}

const NEWS_ARTICLE_TYPES: &[(&str, i32)] = &[
    ("NullNews", 0),
    ("FriendRequestReceived", 1),
    ("FriendLeveledUp", 2),
    ("TopSpeedEclipsed", 3),
    ("LongestPursuitEclipsed", 4),
    ("TotalCostToStateEclipsed", 5),
    ("WelcomeMessage", 6),
    ("AutoRejectedFriendRequest", 7),
    ("DynamicNews", 9),
    ("RareItemWonLuckyDraw", 10),
    ("RareItemWonCardPack", 11),
    ("RepAmplifierExpiring", 12),
    ("RepAmplifierExpired", 13),
    ("CashAmplifierExpiring", 14),
    ("CashAmplifierExpired", 15),
    ("CarRentalExpired", 16),
    ("CarInsuranceExpiring", 17),
    ("CarInsuranceExpired", 18),
    ("UnopenedGift", 19),
    ("RareItemWonStreak", 20),
    ("StreakMilestone", 21),
    ("CarClassChanged", 22),
    ("ReferralBoostGiftAwarded", 23),
];

impl NewsArticleType {
    // port: jpa/util/NewsArticleType#getTypeId()
    pub fn type_id(self) -> i32 {
        self.type_id
    }

    pub fn name(self) -> &'static str {
        self.name
    }

    /// `NewsArticleType.valueOf(name)`.
    fn value_of(name: &str) -> Option<Self> {
        NEWS_ARTICLE_TYPES.iter().find(|(n, _)| *n == name).map(|&(name, type_id)| Self { name, type_id })
    }
}

// port: jpa/convert/NewsArticleFilterConverter#convertToEntityAttribute(String)
fn filters_from_column(column: Option<String>) -> Result<Option<NewsArticleFilters>, Fault> {
    column
        .map(|name| {
            NewsArticleFilters::value_of(&name).ok_or_else(|| {
                anyhow!("No enum constant com.soapboxrace.core.jpa.util.NewsArticleFilters.{name}").into()
            })
        })
        .transpose()
}

// port: jpa/convert/NewsArticleTypeConverter#convertToEntityAttribute(String)
fn type_from_column(column: Option<String>) -> Result<Option<NewsArticleType>, Fault> {
    column
        .map(|name| {
            NewsArticleType::value_of(&name)
                .ok_or_else(|| anyhow!("No enum constant com.soapboxrace.core.jpa.util.NewsArticleType.{name}").into())
        })
        .transpose()
}

/// `NewsArticleEntity`. Its personas are held by id.
#[derive(Debug, Clone, PartialEq)]
pub struct NewsArticleEntity {
    pub id: i64,
    pub parameters: Option<String>,
    pub filters: Option<NewsArticleFilters>,
    pub icon_type: i32,
    pub persona_id: Option<i64>,
    pub referenced_persona_id: Option<i64>,
    pub sticky: i32,
    pub timestamp: Option<PrimitiveDateTime>,
    pub r#type: Option<NewsArticleType>,
    pub short_hal_id: Option<String>,
    pub long_hal_id: Option<String>,
}

// port: dao/NewsArticleDAO#findAllByPersona(Long)
// port: jpa/NewsArticleEntity@NewsArticleEntity.findAllByPersona
/// The persona's news and the news for everyone. MySQL answers through the
/// persona index (`ref_or_null`): the persona's rows first, then the rows with
/// no persona, each by primary key.
pub fn find_all_by_persona(db: &Connection, persona_id: i64) -> Result<Vec<NewsArticleEntity>, Fault> {
    let mut stmt = db.prepare_cached(
        "SELECT id, parameters, filters, iconType, persona_id, referenced_persona_id, sticky, timestamp, type,
                shortHALId, longHALId
         FROM news_article WHERE persona_id = ?1 OR persona_id IS NULL ORDER BY persona_id IS NULL, id",
    )?;
    let rows = stmt.query_map([persona_id], |r| {
        Ok((
            NewsArticleEntity {
                id: r.get(0)?,
                parameters: r.get(1)?,
                filters: None,
                // Primitive ints: Hibernate reads SQL NULL as 0.
                icon_type: r.get::<_, Option<i32>>(3)?.unwrap_or(0),
                persona_id: r.get(4)?,
                referenced_persona_id: r.get(5)?,
                sticky: r.get::<_, Option<i32>>(6)?.unwrap_or(0),
                timestamp: datetime::column(r, "timestamp")?,
                r#type: None,
                short_hal_id: r.get(9)?,
                long_hal_id: r.get(10)?,
            },
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(8)?,
        ))
    })?;
    rows.map(|row| {
        let (mut article, filters, kind) = row?;
        article.filters = filters_from_column(filters)?;
        article.r#type = type_from_column(kind)?;
        Ok(article)
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converters_read_enum_names() {
        assert_eq!(filters_from_column(Some("Item_Count".into())).unwrap(), Some(NewsArticleFilters::ItemCount));
        assert_eq!(NewsArticleFilters::All.filter_mask(), i32::MAX);
        assert_eq!(type_from_column(Some("CarClassChanged".into())).unwrap().unwrap().type_id(), 22);
        assert!(type_from_column(Some("carClassChanged".into())).is_err());
        assert!(filters_from_column(Some("ALL".into())).is_err());
        assert_eq!(type_from_column(None).unwrap(), None);
    }
}
