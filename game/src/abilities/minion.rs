//! Minion (helmet pig) shield: CMinionDefenceAbility

use super::*;

/// STUB (replace with the ported class).
pub struct MinionDefenceAbility {
    pub base: AbilityBase,
}

impl MinionDefenceAbility {
    pub fn new(level: f32) -> MinionDefenceAbility {
        MinionDefenceAbility { base: AbilityBase::new(level) }
    }
}

impl Ability for MinionDefenceAbility {
    fn base(&self) -> &AbilityBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut AbilityBase {
        &mut self.base
    }
    fn id(&self) -> BirdAbility {
        BirdAbility::MinionDefence
    }
    fn name(&self) -> &str {
        "MinionDefence"
    }
}
