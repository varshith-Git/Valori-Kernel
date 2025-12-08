#![no_std]

//! valori-kernel: A deterministic, no_std, fixed-point vector + knowledge graph engine.

#[cfg(test)]
#[macro_use]
extern crate std;

pub mod config;
pub mod error;
pub mod fxp;
pub mod types;
pub mod math;
pub mod storage;
pub mod graph;
pub mod index;
pub mod state;
pub mod snapshot;

#[cfg(test)]
pub mod tests;
