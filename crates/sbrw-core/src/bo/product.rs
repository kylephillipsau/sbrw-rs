//! `ProductBO`: the shop catalogue. Ported so far: everything but the random
//! drops events give, which come with event rewards.

use sbrw_xml::dto::http::{ArrayOfProductTrans, ProductTrans};

use crate::dao::category::{self, CategoryEntity};
use crate::dao::product::{self, ProductEntity};
use crate::dao::{persona, user, vinyl_product};
use crate::{Fault, World};

// port: bo/ProductBO#getProductTransList(List)
pub fn get_product_trans_list(w: &World, product_entities: &[ProductEntity]) -> Result<Vec<ProductTrans>, Fault> {
    product_entities.iter().map(|p| product_entity_to_product_trans(w, p)).collect()
}

// port: bo/ProductBO#productEntityToProductTrans(ProductEntity)
/// A product as the shop shows it, with its bundled products (always none in
/// the data).
fn product_entity_to_product_trans(w: &World, product_entity: &ProductEntity) -> Result<ProductTrans, Fault> {
    let mut bundle_items = ArrayOfProductTrans::default();
    for bundled_product_entity in product::find_bundle_items(w.db(), product_entity.id)? {
        bundle_items.product_trans.push(product_entity_to_product_trans(w, &bundled_product_entity)?);
    }
    Ok(ProductTrans {
        bundle_items: Some(bundle_items),
        currency: Some(product_entity.currency.clone()),
        duration_minute: product_entity.duration_minute,
        description: product_entity.description.clone(),
        hash: product_entity.hash.ok_or_else(|| Fault::null_pointer("ProductEntity.hash"))?,
        icon: Some(product_entity.icon.clone()),
        secondary_icon: product_entity.secondary_icon.clone(),
        level: product_entity.level,
        long_description: product_entity.long_description.clone(),
        price: f64::from(product_entity.price),
        priority: product_entity.priority,
        product_id: Some(product_entity.product_id.clone()),
        product_title: product_entity.product_title.clone(),
        product_type: Some(product_entity.product_type.clone()),
        use_count: product_entity.use_count,
        visual_style: product_entity.visual_style.clone(),
        web_icon: product_entity.web_icon.clone(),
        web_location: product_entity.web_location.clone(),
        category_id: None,
    })
}

/// The level and premium status a persona shops with: level 1 and not
/// premium without one (an id of null or 0).
fn shopper(w: &World, persona_id: Option<i64>) -> Result<(bool, i32), Fault> {
    let Some(persona_id) = persona_id.filter(|&id| id != 0) else { return Ok((false, 1)) };
    let persona_entity = persona::find(w.db(), persona_id)?.ok_or_else(|| Fault::null_pointer("PersonaEntity"))?;
    let user_id = persona_entity.user_id.ok_or_else(|| Fault::null_pointer("PersonaEntity.user"))?;
    let user_entity = user::find(w.db(), user_id)?.ok_or_else(|| Fault::null_pointer("PersonaEntity.user"))?;
    Ok((user_entity.premium, persona_entity.level))
}

// port: bo/ProductBO#productsInCategory(String,String,Long)
/// The enabled products of a category and type the persona may buy.
pub fn products_in_category(
    w: &World,
    category_name: Option<&str>,
    product_type: Option<&str>,
    persona_id: Option<i64>,
) -> Result<Vec<ProductEntity>, Fault> {
    let (premium, level) = shopper(w, persona_id)?;
    // `categoryName = null` matches nothing.
    let (Some(category_name), Some(product_type)) = (category_name, product_type) else { return Ok(Vec::new()) };
    Ok(product::find_by_level_enabled(w.db(), category_name, product_type, level, true, premium)?)
}

// port: bo/ProductBO#categories()
pub fn categories(w: &World) -> Result<Vec<CategoryEntity>, Fault> {
    Ok(category::get_all(w.db())?)
}

// port: bo/ProductBO#getVinylByCategory(CategoryEntity,Long)
/// The enabled vinyls of a category the persona may buy.
pub fn get_vinyl_by_category(
    w: &World,
    category_entity: &CategoryEntity,
    persona_id: Option<i64>,
) -> Result<ArrayOfProductTrans, Fault> {
    let (premium, level) = shopper(w, persona_id)?;
    let mut array_of_product_trans = ArrayOfProductTrans::default();
    let vinyls =
        vinyl_product::find_by_category_level_enabled(w.db(), category_entity.idcategory, level, true, premium)?;
    for entity in vinyls {
        array_of_product_trans.product_trans.push(ProductTrans {
            currency: entity.currency,
            duration_minute: entity.duration_minute,
            hash: entity.hash.ok_or_else(|| Fault::null_pointer("VinylProductEntity.hash"))?,
            icon: entity.icon,
            secondary_icon: entity.secondary_icon,
            level: entity.level,
            price: f64::from(entity.price),
            priority: entity.priority,
            product_id: entity.product_id,
            product_title: entity.product_title,
            product_type: entity.product_type,
            use_count: entity.use_count,
            ..Default::default()
        });
    }
    Ok(array_of_product_trans)
}
