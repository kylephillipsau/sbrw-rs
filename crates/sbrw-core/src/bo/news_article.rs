//! `NewsArticleBO`: the game's news feed.

use sbrw_xml::dto::http::{ArrayOfNewsArticleTrans, NewsArticleTrans};

use crate::bo::util::time_converter;
use crate::dao::news_article;
use crate::{Fault, World};

// port: bo/NewsArticleBO#getNewsArticles(Long)
/// The news for a persona (0 when none is selected) and for everyone.
pub fn get_news_articles(w: &World, persona_id: i64) -> Result<ArrayOfNewsArticleTrans, Fault> {
    let null = |what| move || Fault::null_pointer(what);
    let mut array = ArrayOfNewsArticleTrans::default();
    for entity in news_article::find_all_by_persona(w.db(), persona_id)? {
        array.news_article_trans.push(NewsArticleTrans {
            news_id: entity.id,
            persona_id,
            timestamp: time_converter::get_ticks_of(entity.timestamp.ok_or_else(null("NewsArticleEntity.timestamp"))?),
            parameters: entity.parameters,
            sticky: entity.sticky,
            r#type: entity.r#type.ok_or_else(null("NewsArticleEntity.type"))?.type_id(),
            short_text_hal_id: entity.short_hal_id,
            long_text_hal_id: entity.long_hal_id,
            filters: entity.filters.ok_or_else(null("NewsArticleEntity.filters"))?.filter_mask(),
            icon_type: entity.icon_type,
            ..Default::default()
        });
    }
    Ok(array)
}
