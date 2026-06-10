//! Deterministic render layering for table items.
//!
//! vpinball sorts its transparent parts by `depth_bias - center.z` (the bounding
//! sphere centre, see `RenderDevice::DrawMesh`) and draws them back to front, with
//! the stable submission order breaking ties. Bevy's transparent 2D pass sorts by
//! the entity transform z only, so we port that sort key to a z value: higher item
//! centres draw on top, a positive depth bias pushes an item down, and items with
//! an equal key stack in their gameitem order via a tiny per-index offset.
//!
//! Within one mesh the entity z cannot order overlapping faces; meshes that can
//! self-overlap (ramps, projected primitives) emit their triangles sorted low to
//! high instead, so e.g. the high end of a looping ramp draws over its low start.

use vpin::vpx::units::vpu_to_m;

/// Spacing between same-height items, in metres per gameitem index. Small enough
/// that a full table (hundreds of items) drifts well below one vpx unit (~0.5 mm).
const ITEM_ORDER_EPSILON_M: f32 = 1e-7;

/// The render z for a table item: its centre height minus its depth bias (both in
/// vpx units), offset by the item's position in the gameitem list.
pub(super) fn render_z(center_height_vpu: f32, depth_bias_vpu: f32, item_index: usize) -> f32 {
    vpu_to_m(center_height_vpu - depth_bias_vpu) + item_index as f32 * ITEM_ORDER_EPSILON_M
}
