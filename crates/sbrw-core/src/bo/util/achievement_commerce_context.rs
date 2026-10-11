//! `AchievementCommerceContext`: what COMMERCE achievement scripts see about
//! a purchase.

use crate::dao::car_classes::CarClassesEntity;
use crate::dao::product::ProductEntity;

/// `AchievementCommerceContext.CommerceType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommerceType {
    CarPurchase,
    BundlePurchase,
}

impl CommerceType {
    /// The constant's name.
    pub fn name(self) -> &'static str {
        match self {
            Self::CarPurchase => "CAR_PURCHASE",
            Self::BundlePurchase => "BUNDLE_PURCHASE",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AchievementCommerceContext {
    pub car_classes_entity: Option<CarClassesEntity>,
    pub product_entity: Option<ProductEntity>,
    pub commerce_type: CommerceType,
}

impl AchievementCommerceContext {
    // port: bo/util/AchievementCommerceContext#<init>(CommerceType)
    pub fn new(commerce_type: CommerceType) -> Self {
        Self { car_classes_entity: None, product_entity: None, commerce_type }
    }

    // port: bo/util/AchievementCommerceContext#getCommerceTypeVal()
    /// The type's ordinal, which the scripts compare.
    pub fn commerce_type_val(&self) -> i32 {
        match self.commerce_type {
            CommerceType::CarPurchase => 0,
            CommerceType::BundlePurchase => 1,
        }
    }

    // port: bo/util/AchievementCommerceContext#setCarClassesEntity(CarClassesEntity)
    pub fn set_car_classes_entity(&mut self, car_classes_entity: CarClassesEntity) {
        self.car_classes_entity = Some(car_classes_entity);
    }

    // port: bo/util/AchievementCommerceContext#setProductEntity(ProductEntity)
    pub fn set_product_entity(&mut self, product_entity: ProductEntity) {
        self.product_entity = Some(product_entity);
    }
}
