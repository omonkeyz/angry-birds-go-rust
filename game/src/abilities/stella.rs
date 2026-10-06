//! Stella: CStellaDefenceAbility (player bubble) and CStellaBossAbility (boss)

use super::*;

/// STUB (replace with the ported class).
pub struct StellaDefenceAbility {
    pub base: AbilityBase,
}

impl StellaDefenceAbility {
    pub fn new(level: f32) -> StellaDefenceAbility {
        StellaDefenceAbility { base: AbilityBase::new(level) }
    }
}

impl Ability for StellaDefenceAbility {
    fn base(&self) -> &AbilityBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut AbilityBase {
        &mut self.base
    }
    fn id(&self) -> BirdAbility {
        BirdAbility::StellaDefence
    }
    fn name(&self) -> &str {
        "StellaDefence"
    }
}

/// STUB (replace with the ported class).
pub struct StellaBossAbility {
    pub base: AbilityBase,
}

impl StellaBossAbility {
    pub fn new(level: f32) -> StellaBossAbility {
        StellaBossAbility { base: AbilityBase::new(level) }
    }
}

impl Ability for StellaBossAbility {
    fn base(&self) -> &AbilityBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut AbilityBase {
        &mut self.base
    }
    fn id(&self) -> BirdAbility {
        BirdAbility::StellaBossAbility
    }
    fn name(&self) -> &str {
        "StellaBossAbility"
    }
}
