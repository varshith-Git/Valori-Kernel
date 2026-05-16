# Valori Kernel: Module Analysis - Fixed-Point (FXP) & Math Core

This report is the first in our module-by-module deep dive into the Valori Kernel. We start at the foundation: the **Fixed-Point (FXP) & Vector Math** module. Because Valori is a strictly deterministic, `no_std` kernel designed to produce bit-exact results across all CPU architectures, floating-point arithmetic (`f32`/`f64`) is entirely outlawed in the core engine.

---

## 1. Q-Format & Scalar Representation

**Location**: `src/fxp/qformat.rs` and `src/types/scalar.rs`

At the lowest level, every numerical value in Valori is stored as a 32-bit signed integer (`i32`), wrapping a fixed-point representation.

### `FxpScalar`
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[repr(transparent)]
pub struct FxpScalar(pub i32);
```
- **Representation**: Valori uses the **Q16.16** format. This means 16 bits are dedicated to the integer part, and 16 bits to the fractional part.
- **Scale**: `SCALE = 1 << 16` (65,536).
- **Constants**: Provides `FxpScalar::ZERO = FxpScalar(0)` and `FxpScalar::ONE = FxpScalar(65536)`.
- **Use Case**: This ensures that distances, similarities, and vector dimensions are exact across ARM, x86, or WebAssembly, avoiding unpredictable floating-point rounding errors.

---

## 2. Fixed-Point Operations (Addition, Subtraction, Multiplication)

**Location**: `src/fxp/ops.rs`

The operations must guarantee that numbers never panic on overflow, which would crash the engine.

### `fxp_add` and `fxp_sub`
```rust
pub fn fxp_add(a: FxpScalar, b: FxpScalar) -> FxpScalar
pub fn fxp_sub(a: FxpScalar, b: FxpScalar) -> FxpScalar
```
- **Working**: Relies on Rust's `saturating_add` and `saturating_sub`.
- **Edge Cases**: If an addition exceeds `i32::MAX` (representing roughly `32767.99`), the result is pinned to `i32::MAX`. If it drops below `i32::MIN`, it is pinned to `i32::MIN`. This prevents wrapping overflow which would completely corrupt vector distances.

### `fxp_mul`
```rust
pub fn fxp_mul(a: FxpScalar, b: FxpScalar) -> FxpScalar
```
- **Working**: 
  1. Casts both `i32` values up to `i64` to prevent overflow during intermediate multiplication: `(a.0 as i64) * (b.0 as i64)`.
  2. The product is then shifted down by `FRAC_BITS` (16) to re-normalize the Q16.16 decimal position.
  3. **Saturation**: The resulting `i64` is manually clamped back to the `[i32::MIN, i32::MAX]` range before casting down to `i32`.

### Float-to-Fixed Conversions (`from_f32`, `to_f32`)
- **Important Rule**: These are *only* available under `#[cfg(any(test, feature = "std"))]`. The actual core kernel does not have access to these. The Node layer must call `from_f32` (or its equivalent scaling math) before passing vectors to the kernel.
- **`from_f32(f: f32)`**: Multiplies `f` by `SCALE` (65536.0), uses `.round()`, and clamps to `[i32::MIN, i32::MAX]`.

---

## 3. Dynamic Vector Representation

**Location**: `src/types/vector.rs`

### `FxpVector`
```rust
pub struct FxpVector {
    pub data: alloc::vec::Vec<FxpScalar>,
}
```
- **Design**: Wraps a heap-allocated `Vec` (using the `alloc` crate, valid in `no_std`) of `FxpScalar`. 
- **Serialization**: Implements a custom `Serialize` and `Deserialize` using a `Visitor`. It flattens the vector into a raw sequence of `FxpScalars`, bypassing the standard `Vec` struct wrapper in `bincode` for tighter and more predictable byte packing.
- **Utilities**: Provides `new_zeros(dim)`, array indexing, and mutable slices.

---

## 4. Deterministic Distance Metrics (Math)

**Location**: `src/math/dot.rs` and `src/math/l2.rs`

### Dot Product (`fxp_dot`)
```rust
pub fn fxp_dot(a: &FxpVector, b: &FxpVector) -> FxpScalar
```
- **Working**: Iterates through matching dimensions of `a` and `b`. 
- **Accumulation**: Uses an `i64` accumulator (`sum`). For every element, it calculates the Q16.16 product (`i64 * i64 >> 16`), and `saturating_add`s it to `sum`.
- **Edge Cases**:
  - *Mismatched lengths*: Iterates up to `a.len().min(b.len())` safely without out-of-bounds panics.
  - *Final Saturation*: After the entire loop, the `i64` sum is manually saturated to the `i32` boundaries. If two massive vectors are dotted, the max score they can achieve is `32767.99` in float terms.

### L2 Squared Distance (`fxp_l2_sq`)
```rust
pub fn fxp_l2_sq(a: &FxpVector, b: &FxpVector) -> FxpScalar
```
- **Working**: Computes `||a - b||^2`. 
- Loops through elements, uses `fxp_sub` to find the difference, `fxp_mul` to square it, and `fxp_add` to accumulate the distance.
- **Safety**: Because it reuses `fxp_add` and `fxp_mul`, it inherits the `i32` saturation constraints automatically at every single step of the loop.

---

## 5. The Quantizer Interface

**Location**: `src/quant/mod.rs`

```rust
pub trait Quantizer {
    type Code;
    fn encode(&self, v: &FxpVector) -> Self::Code;
    fn decode(&self, code: &Self::Code) -> FxpVector;
}
```
- **Purpose**: Exposes a trait for potential compression (like Product Quantization - PQ or Scalar Quantization).
- **`NoQuantizer`**: The default implementation is a pure pass-through. `encode` clones the `FxpVector`, and `decode` returns it as-is.

---

### Summary of Module Guarantees
1. **Crash Immunity**: By heavily utilizing `saturating_add`, `saturating_sub`, and manual `i64` clamping bounds, it is mathematically impossible for Valori's distance calculations to trigger a Rust `panic!` due to integer overflow.
2. **Platform Consistency**: The strict usage of bitwise shifting (`>> 16`) and standard integer sizing means an embedding processed on a Raspberry Pi (ARM32) will yield the exact same 32-bit score as an AWS Graviton or Intel Xeon.
