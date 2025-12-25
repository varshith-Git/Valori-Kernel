// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
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
pub mod index;
pub mod quant;
pub mod graph;
pub mod state;
pub mod snapshot;
pub mod verify;
pub mod proof;
pub mod replay;

#[cfg(test)]
pub mod tests;
