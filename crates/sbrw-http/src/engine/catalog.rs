//! `api/Catalog`: the shops' products and categories. As upstream, each call
//! into a BO is its own world call.

use axum::http::StatusCode;
use axum::response::Response;
use sbrw_core::Fault;
use sbrw_core::bo::product;
use sbrw_xml::ToXml;
use sbrw_xml::dto::http::{ArrayOfCategoryTrans, ArrayOfProductTrans, CategoryTrans};

use super::{Call, entity};
use crate::AppState;

// port: api/Catalog#productsInCategory(String,String,String)
/// The products of a category and type for the selected driver.
pub(super) async fn products_in_category(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let category_name = call.query("categoryName");
    let client_product_type = call.query("clientProductType");
    let active_persona_id = call.session().active_persona_id;
    let world = state.world();
    let products_in_category = world
        .call(move |w| {
            product::products_in_category(
                w,
                category_name.as_deref(),
                client_product_type.as_deref(),
                active_persona_id,
            )
        })
        .await?;
    let product_trans_list = world.call(move |w| product::get_product_trans_list(w, &products_in_category)).await?;
    let array_of_product_trans = ArrayOfProductTrans { product_trans: product_trans_list };
    Ok(entity(call, StatusCode::OK, array_of_product_trans.to_xml()))
}

// port: api/Catalog#categories(String)
/// Every category, with the vinyls the selected driver may buy in it.
pub(super) async fn categories(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let active_persona_id = call.session().active_persona_id;
    let world = state.world();
    let mut array_of_category_trans = ArrayOfCategoryTrans::default();
    for entity_ in world.call(|w| product::categories(w)).await? {
        let catalog_version = match &entity_.catalog_version {
            Some(v) => v.parse().map_err(|_| anyhow::anyhow!("For input string: \"{v}\""))?,
            None => return Err(anyhow::anyhow!("NumberFormatException: null").into()),
        };
        let id = entity_.identifiant.ok_or_else(|| Fault::null_pointer("CategoryEntity.id"))?.to_string();
        let filter_type = entity_.filter_type.ok_or_else(|| Fault::null_pointer("CategoryEntity.filterType"))?;
        let priority = entity_.priority.ok_or_else(|| Fault::null_pointer("CategoryEntity.priority"))?;
        let category = entity_.clone();
        let products = world.call(move |w| product::get_vinyl_by_category(w, &category, active_persona_id)).await?;
        let show_in_navigation_pane = entity_
            .show_in_navigation_pane
            .ok_or_else(|| Fault::null_pointer("CategoryEntity.showInNavigationPane"))?;
        let show_promo_page =
            entity_.show_promo_page.ok_or_else(|| Fault::null_pointer("CategoryEntity.showPromoPage"))?;
        array_of_category_trans.category_trans.push(CategoryTrans {
            catalog_version,
            display_name: entity_.display_name,
            filter_type,
            icon: entity_.icon,
            id: Some(id),
            long_description: entity_.long_description,
            name: entity_.name,
            priority: i32::from(priority),
            products: Some(products),
            short_description: entity_.short_description,
            show_in_navigation_pane,
            show_promo_page,
            web_icon: entity_.web_icon,
            categories: None,
        });
    }
    Ok(entity(call, StatusCode::OK, array_of_category_trans.to_xml()))
}
