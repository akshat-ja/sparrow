use crate::overlap::overlap_proxy;
use crate::overlap::tracker::OverlapTracker;
use crate::util::io;
use crate::util::io::layout_to_svg::layout_to_svg_2;
use crate::util::io::svg_util::SvgDrawOptions;
use float_cmp::{approx_eq, assert_approx_eq};
use itertools::Itertools;
use jagua_rs::collision_detection::hazard::HazardEntity;
use jagua_rs::entities::layout::Layout;
use jagua_rs::fsize;
use jagua_rs::util::assertions;
use log::warn;
use std::path::Path;

pub fn tracker_matches_layout(ot: &OverlapTracker, l: &Layout) -> bool {
    assert!(l.placed_items.keys().all(|k| ot.pk_idx_map.contains_key(k)));
    assert!(assertions::layout_qt_matches_fresh_qt(l));

    for (pk1, pi1) in l.placed_items.iter() {
        let collisions = l.cde().collect_poly_collisions(&pi1.shape, &[pi1.into()]);
        assert_eq!(ot.get_pair_overlap(pk1, pk1), 0.0);
        for (pk2, pi2) in l.placed_items.iter().filter(|(k, _)| *k != pk1) {
            let stored_overlap = ot.get_pair_overlap(pk1, pk2);
            match collisions.contains(&(pi2.into())) {
                true => {
                    let calc_overlap = overlap_proxy::poly_overlap_proxy(&pi1.shape, &pi2.shape);
                    let calc_overlap2 = overlap_proxy::poly_overlap_proxy(&pi2.shape, &pi1.shape);
                    if !approx_eq!(
                        fsize,
                        calc_overlap,
                        stored_overlap,
                        epsilon = 0.001 * stored_overlap
                    ) {
                        let opposite_collisions =
                            l.cde().collect_poly_collisions(&pi2.shape, &[pi2.into()]);
                        if opposite_collisions.contains(&(pi1.into())) {
                            dbg!(&pi1.shape.points, &pi2.shape.points);
                            dbg!(
                                stored_overlap,
                                calc_overlap,
                                calc_overlap2,
                                opposite_collisions,
                                HazardEntity::from(pi1),
                                HazardEntity::from(pi2)
                            );
                            panic!("overlap tracker error");
                        } else {
                            //detecting collisions is not symmetrical (in edge cases)
                            warn!("inconsistent overlap");
                            warn!(
                                "collisions: pi_1 {:?} -> {:?}",
                                HazardEntity::from(pi1),
                                collisions
                            );
                            warn!(
                                "opposite collisions: pi_2 {:?} -> {:?}",
                                HazardEntity::from(pi2),
                                opposite_collisions
                            );

                            warn!(
                                "pi_1: {:?}",
                                pi1.shape
                                    .points
                                    .iter()
                                    .map(|p| format!("({},{})", p.0, p.1))
                                    .collect_vec()
                            );
                            warn!(
                                "pi_2: {:?}",
                                pi2.shape
                                    .points
                                    .iter()
                                    .map(|p| format!("({},{})", p.0, p.1))
                                    .collect_vec()
                            );

                            {
                                let mut svg_draw_options = SvgDrawOptions::default();
                                svg_draw_options.quadtree = true;
                                let svg = layout_to_svg_2(l, svg_draw_options);
                                io::write_svg(&svg, &*Path::new("debug.svg"), log::Level::Warn);
                                panic!("overlap tracker error");
                            }
                        }
                    }
                }
                false => {
                    if stored_overlap != 0.0 {
                        let calc_overlap =
                            overlap_proxy::poly_overlap_proxy(&pi1.shape, &pi2.shape);
                        let opposite_collisions =
                            l.cde().collect_poly_collisions(&pi2.shape, &[pi2.into()]);
                        if !opposite_collisions.contains(&(pi1.into())) {
                            dbg!(&pi1.shape.points, &pi2.shape.points);
                            dbg!(
                                stored_overlap,
                                calc_overlap,
                                opposite_collisions,
                                HazardEntity::from(pi1),
                                HazardEntity::from(pi2)
                            );
                            panic!("overlap tracker error");
                        } else {
                            //detecting collisions is not symmetrical (in edge cases)
                            warn!("inconsistent overlap");
                            warn!(
                                "collisions: {:?} -> {:?}",
                                HazardEntity::from(pi1),
                                collisions
                            );
                            warn!(
                                "opposite collisions: {:?} -> {:?}",
                                HazardEntity::from(pi2),
                                opposite_collisions
                            );
                        }
                    }
                }
            }
        }
        if collisions.contains(&HazardEntity::BinExterior) {
            let bin_overlap = ot.get_bin_overlap(pk1);
            let calc_overlap = overlap_proxy::bin_overlap_proxy(&pi1.shape, l.bin.bbox());
            assert_approx_eq!(fsize, calc_overlap, bin_overlap, ulps = 5);
        } else {
            assert_eq!(ot.get_bin_overlap(pk1), 0.0);
        }
    }

    true
}
