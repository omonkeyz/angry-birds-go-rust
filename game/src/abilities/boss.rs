//! Other boss weapons: CBlueBossAbility, CChuckBossAbility

use super::*;

/// STUB (replace with the ported class).
pub struct BlueBossAbility {
    pub base: AbilityBase,
}

impl BlueBossAbility {
    pub fn new(level: f32) -> BlueBossAbility {
        BlueBossAbility { base: AbilityBase::new(level) }
    }
}

impl Ability for BlueBossAbility {
    fn base(&self) -> &AbilityBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut AbilityBase {
        &mut self.base
    }
    fn id(&self) -> BirdAbility {
        BirdAbility::BlueBoss
    }
    fn name(&self) -> &str {
        "BlueBossAbility"
    }
}

/// STUB (replace with the ported class).
pub struct ChuckBossAbility {
    pub base: AbilityBase,
}

impl ChuckBossAbility {
    pub fn new(level: f32) -> ChuckBossAbility {
        ChuckBossAbility { base: AbilityBase::new(level) }
    }
}

impl Ability for ChuckBossAbility {
    fn base(&self) -> &AbilityBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut AbilityBase {
        &mut self.base
    }
    fn id(&self) -> BirdAbility {
        BirdAbility::ChuckBoss
    }
    fn name(&self) -> &str {
        "ChuckBossAbility"
    }
}
