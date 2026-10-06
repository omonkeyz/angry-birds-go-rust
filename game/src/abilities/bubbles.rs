//! Bubbles: CBubblesInflateAbility (player) and CBubblesBossAbility (boss)

use super::*;

/// STUB (replace with the ported class).
pub struct BubblesInflateAbility {
    pub base: AbilityBase,
}

impl BubblesInflateAbility {
    pub fn new(level: f32) -> BubblesInflateAbility {
        BubblesInflateAbility { base: AbilityBase::new(level) }
    }
}

impl Ability for BubblesInflateAbility {
    fn base(&self) -> &AbilityBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut AbilityBase {
        &mut self.base
    }
    fn id(&self) -> BirdAbility {
        BirdAbility::BubblesInflate
    }
    fn name(&self) -> &str {
        "BubblesInflateAbility"
    }
}

/// STUB (replace with the ported class).
pub struct BubblesBossAbility {
    pub base: AbilityBase,
}

impl BubblesBossAbility {
    pub fn new(level: f32) -> BubblesBossAbility {
        BubblesBossAbility { base: AbilityBase::new(level) }
    }
}

impl Ability for BubblesBossAbility {
    fn base(&self) -> &AbilityBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut AbilityBase {
        &mut self.base
    }
    fn id(&self) -> BirdAbility {
        BirdAbility::BubblesBoss
    }
    fn name(&self) -> &str {
        "BubblesBossAbility"
    }
}
