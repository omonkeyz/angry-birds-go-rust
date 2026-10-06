//! In-track items of the original game (event-definition `TrackItem`s): coins, boost pads, seed rush tokens, smackable blocks, slalom gates ...
//! Standalone: only depends on `abgtool`, `glam`, `roxmltree`; no dependency on the drive / car simulation code.
#![allow(dead_code)]
pub mod envobjs;
pub mod eventdef;
pub mod ground;
pub mod models;
pub mod place;
pub mod slalom;
pub mod smackables;
pub mod smackdefs;
pub mod spline;
pub mod structures;
pub mod world;

pub use slalom::{Gate, GateResult};
pub use world::{DrawInstance, Item, ItemEvent, ItemKind, ItemWorld, KartState, ModelNeed};
#[cfg(test)]
mod tests;
