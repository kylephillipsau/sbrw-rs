//! `DropBO`: random droppable products, and products as lucky draw items.

use md5::{Digest, Md5};
use sbrw_xml::dto::http::LuckyDrawItem;

use crate::bo::product;
use crate::dao::product::ProductEntity;
use crate::{Fault, World};

// port: bo/DropBO#getRandomProductItem()
/// A product of a type drawn evenly from performance parts, powerups,
/// skill mods and visual parts.
pub fn get_random_product_item(w: &mut World) -> Result<ProductEntity, Fault> {
    const PRODUCT_TYPE_ARR: [&str; 4] = ["PERFORMANCEPART", "POWERUP", "SKILLMODPART", "VISUALPART"];
    let number = w.env.random(|r| r.next_int_bounded(PRODUCT_TYPE_ARR.len() as i32));
    product::get_random_drop(w, PRODUCT_TYPE_ARR[number as usize])
}

// port: bo/DropBO#copyProduct2LuckyDraw(ProductEntity)
pub fn copy_product2_lucky_draw(product_entity: &ProductEntity) -> Result<LuckyDrawItem, Fault> {
    let hash = product_entity.hash.ok_or_else(|| Fault::null_pointer("ProductEntity.hash"))?;
    Ok(LuckyDrawItem {
        description: product_entity.product_title.clone(),
        hash,
        icon: Some(product_entity.icon.clone()),
        remaining_use_count: product_entity.use_count,
        resell_price: product_entity.resale_price,
        virtual_item: Some(hex::encode(Md5::digest(hash.to_string().as_bytes()))),
        virtual_item_type: Some(product_entity.product_type.clone()),
        was_sold: false,
    })
}
