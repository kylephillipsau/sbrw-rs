//! `RewardVO`: what a finished event pays, part by part.

use sbrw_xml::dto::http::{ArrayOfRewardPart, EnumRewardCategory, EnumRewardType, RewardPart};

/// The base rewards the parts are worked out from, the running totals, and
/// the parts. With economy or reputation off, parts add no cash or rep.
#[derive(Debug, Clone, PartialEq)]
pub struct RewardVO {
    base_rep: i32,
    base_cash: i32,
    rep: i32,
    cash: i32,
    array_of_reward_part: ArrayOfRewardPart,
    enable_economy: bool,
    enable_reputation: bool,
}

impl RewardVO {
    // port: bo/util/RewardVO#<init>(boolean,boolean)
    pub fn new(enable_economy: bool, enable_reputation: bool) -> Self {
        Self {
            base_rep: 0,
            base_cash: 0,
            rep: 0,
            cash: 0,
            array_of_reward_part: ArrayOfRewardPart::default(),
            enable_economy,
            enable_reputation,
        }
    }

    // port: bo/util/RewardVO#getBaseRep()
    pub fn base_rep(&self) -> i32 {
        self.base_rep
    }

    // port: bo/util/RewardVO#setBaseRep(int)
    pub fn set_base_rep(&mut self, base_rep: i32) {
        self.base_rep = base_rep;
    }

    // port: bo/util/RewardVO#getBaseCash()
    pub fn base_cash(&self) -> i32 {
        self.base_cash
    }

    // port: bo/util/RewardVO#setBaseCash(int)
    pub fn set_base_cash(&mut self, base_cash: i32) {
        self.base_cash = base_cash;
    }

    // port: bo/util/RewardVO#add(int,int,EnumRewardCategory,EnumRewardType)
    pub fn add(&mut self, mut rep: i32, mut cash: i32, category: EnumRewardCategory, reward_type: EnumRewardType) {
        if !self.enable_reputation {
            rep = 0;
        }
        if !self.enable_economy {
            cash = 0;
        }
        self.array_of_reward_part.reward_part.push(RewardPart {
            rep_part: rep,
            token_part: cash,
            reward_category: Some(category),
            reward_type: Some(reward_type),
        });
        self.rep = self.rep.wrapping_add(rep);
        self.cash = self.cash.wrapping_add(cash);
    }

    // port: bo/util/RewardVO#getRep()
    pub fn rep(&self) -> i32 {
        self.rep
    }

    // port: bo/util/RewardVO#getCash()
    pub fn cash(&self) -> i32 {
        self.cash
    }

    // port: bo/util/RewardVO#getArrayOfRewardPart()
    pub fn array_of_reward_part(&self) -> &ArrayOfRewardPart {
        &self.array_of_reward_part
    }
}
