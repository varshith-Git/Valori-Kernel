// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
#![no_std]

//! valori-kernel: A deterministic, no_std, fixed-point vector + knowledge graph engine.

extern crate alloc;

#[cfg(test)]
#[macro_use]
extern crate std;

pub mod config;
pub mod error;
pub mod fxp;
pub mod types;
pub mod math;
pub mod storage;
pub mod index;
pub mod quant;
pub mod graph;
pub mod state;
pub mod snapshot;
pub mod verify;
pub mod proof;
pub mod replay;
pub mod event;
pub mod replay_events;

#[cfg(test)]
pub mod tests;
