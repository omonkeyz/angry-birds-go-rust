//! CObjectSpawnAbility (boss object-spawn weapon and base of the boss weapons)

use super::*;

/// STUB (replace with the ported class).
pub struct ObjectSpawnAbility {
    pub base: AbilityBase,
}

impl ObjectSpawnAbility {
    pub fn new(level: f32) -> ObjectSpawnAbility {
        ObjectSpawnAbility { base: AbilityBase::new(level) }
    }
}

impl Ability for ObjectSpawnAbility {
    fn base(&self) -> &AbilityBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut AbilityBase {
        &mut self.base
    }
    fn id(&self) -> BirdAbility {
        BirdAbility::ObjectSpawn
    }
    fn name(&self) -> &str {
        "ObjectSpawn"
    }
}
