//! `AchievementInventoryContext`: what INVENTORY achievement scripts see
//! about an item. No achievement in the data has that category, so no script
//! reads it yet.

use crate::dao::inventory_item::InventoryItemEntity;

/// `AchievementInventoryContext.Event`, in declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Added,
    QuantityDecreased,
    QuantityIncreased,
}

impl Event {
    /// The constant's name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Added => "ADDED",
            Self::QuantityDecreased => "QUANTITY_DECREASED",
            Self::QuantityIncreased => "QUANTITY_INCREASED",
        }
    }
}

// port: bo/util/AchievementInventoryContext#getInventoryItemEntity()
// port: bo/util/AchievementInventoryContext#getEvent()
#[derive(Debug, Clone, PartialEq)]
pub struct AchievementInventoryContext {
    pub inventory_item_entity: InventoryItemEntity,
    pub event: Event,
}

impl AchievementInventoryContext {
    // port: bo/util/AchievementInventoryContext#<init>(InventoryItemEntity,Event)
    pub fn new(inventory_item_entity: InventoryItemEntity, event: Event) -> Self {
        Self { inventory_item_entity, event }
    }

    // port: bo/util/AchievementInventoryContext#getEventVal()
    /// The event's ordinal.
    pub fn event_val(&self) -> i32 {
        self.event as i32
    }
}
