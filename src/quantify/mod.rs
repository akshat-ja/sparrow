use jagua_rs::geometry::geo_traits::{DistanceTo, Shape};
use jagua_rs::geometry::primitives::{AARectangle, SimplePolygon};
use crate::config::OVERLAP_PROXY_EPSILON_DIAM_RATIO;
use crate::quantify::overlap_proxy::overlap_area_proxy;

pub mod overlap_proxy;
mod pair_matrix;
pub mod tracker;
#[cfg(feature = "simd")]
pub mod simd;

/// Quantifies a collision between two simple polygons.
#[inline(always)]
pub fn quantify_collision_poly_poly(s1: &SimplePolygon, s2: &SimplePolygon) -> f32 {
    let epsilon = f32::max(s1.diameter(), s2.diameter()) * OVERLAP_PROXY_EPSILON_DIAM_RATIO;

    let overlap_proxy = overlap_area_proxy(&s1.surrogate(), &s2.surrogate(), epsilon) + epsilon.powi(2);

    debug_assert!(overlap_proxy.is_normal());

    let penalty = calc_shape_penalty(s1, s2);

    overlap_proxy.sqrt() * penalty
}

pub fn calc_shape_penalty(s1: &SimplePolygon, s2: &SimplePolygon) -> f32 {
    let p1 = f32::sqrt(s1.surrogate().convex_hull_area);
    let p2 = f32::sqrt(s2.surrogate().convex_hull_area);
    (p1 * p2).sqrt() //geometric mean
}

/// Quantifies a collision between a simple polygon and the exterior of the bin.
#[inline(always)]
pub fn quantify_collision_poly_bin(s: &SimplePolygon, bin_bbox: AARectangle) -> f32 {
    let s_bbox = s.bbox();
    let overlap = match AARectangle::from_intersection(&s_bbox, &bin_bbox) {
        Some(r) => {
            //intersection exist, calculate the area of the intersection (+ a small value to ensure it is never zero)
            let negative_area = (s_bbox.area() - r.area()) + 0.001 * s_bbox.area();
            negative_area
        }
        None => {
            //no intersection, guide towards intersection with bin
            s_bbox.area() + s_bbox.centroid().distance_to(&bin_bbox.centroid())
        }
    };
    debug_assert!(overlap.is_normal());

    let penalty = s.surrogate().convex_hull_area;

    10.0 * (overlap * penalty).sqrt()
}