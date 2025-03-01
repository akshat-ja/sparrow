use float_cmp::approx_eq;
use crate::overlap::overlap_proxy::{bin_overlap_proxy, poly_overlap_proxy};
use crate::overlap::tracker::OverlapTracker;
use crate::sample::eval::{SampleEval, SampleEvaluator};
use jagua_rs::collision_detection::hazard_helpers::{HazardDetector, DetectionMap};
use jagua_rs::collision_detection::hazard::HazardEntity;
use jagua_rs::entities::item::Item;
use jagua_rs::entities::layout::Layout;
use jagua_rs::entities::placed_item::PItemKey;
use jagua_rs::fsize;
use jagua_rs::geometry::d_transformation::DTransformation;
use jagua_rs::geometry::geo_traits::TransformableFrom;
use jagua_rs::geometry::primitives::simple_polygon::SimplePolygon;

pub struct SeparationSampleEvaluator<'a> {
    layout: &'a Layout,
    item: &'a Item,
    current_pk: PItemKey,
    ot: &'a OverlapTracker,
    detection_map: DetectionMap,
    shape_buff: SimplePolygon,
    n_evals: usize,
}

impl<'a> SeparationSampleEvaluator<'a> {
    pub fn new(
        layout: &'a Layout,
        item: &'a Item,
        current_pk: PItemKey,
        ot: &'a OverlapTracker,
    ) -> Self {
        Self {
            layout,
            item,
            current_pk,
            ot,
            detection_map: DetectionMap::new(),
            shape_buff: item.shape.as_ref().clone(),
            n_evals: 0,
        }
    }
}

impl<'a> SampleEvaluator for SeparationSampleEvaluator<'a> {
    fn eval(&mut self, dt: DTransformation, upper_bound: Option<SampleEval>) -> SampleEval {
        self.n_evals += 1;
        let cde = self.layout.cde();

        self.detection_map.clear();
        let transf = dt.compose();
        self.shape_buff.transform_from(&self.item.shape, &transf);
        let pi = &self.layout.placed_items[self.current_pk];
        let irrel_haz = HazardEntity::from((self.current_pk,pi));

        //do a check with the surrogate, calculate overlap
        cde.collect_surrogate_collisions_in_detector(&self.item.shape.surrogate(), &transf, &[irrel_haz], &mut self.detection_map);

        //calculate weighted overlap for all hazards detected by the surrogate
        let surr_w_overlap = self.calc_overlap_cost(self.detection_map.iter());

        //if this already exceeds the upperbound, return
        if let Some(SampleEval::Colliding(upper_bound)) = upper_bound {
            if surr_w_overlap > upper_bound {
                debug_assert!(self.eval(dt, None) >= SampleEval::Colliding(upper_bound ), "upper bound violated: {:?} < {:?}", self.eval(dt, None), SampleEval::Colliding(upper_bound));
                return SampleEval::Invalid;
            }
        }

        //If not, move onto a full collision check
        let dm_index_counter = self.detection_map.index_counter();
        cde.collect_poly_collisions_in_detector(&self.shape_buff, &[irrel_haz], &mut self.detection_map);

        //By now, the buffer should contain all hazards
        if self.detection_map.len() == 0 {
            SampleEval::Valid(0.0)
        } else {
            //Calculate the remaining weighted overlap for the hazards not detected by the surrogate
            let add_hazards = self.detection_map.iter_with_index()
                .filter(|(_, idx)| *idx >= dm_index_counter)
                .map(|(h, _)| h);

            let add_w_overlap = self.calc_overlap_cost(add_hazards);
            let full_w_overlap = surr_w_overlap + add_w_overlap;

            debug_assert!(approx_eq!(fsize, full_w_overlap, self.calc_overlap_cost(self.detection_map.iter())));

            SampleEval::Colliding(full_w_overlap)
        }
    }

    fn n_evals(&self) -> usize {
        self.n_evals
    }
}

impl<'a> SeparationSampleEvaluator<'a> {
    pub fn calc_overlap_cost(&self, colliding: impl Iterator<Item=&'a HazardEntity>) -> fsize {
        colliding.map(|haz| match haz {
            HazardEntity::PlacedItem { pk: other_pk, .. } => {
                let other_shape = &self.layout.placed_items[*other_pk].shape;
                let overlap = poly_overlap_proxy(&self.shape_buff, other_shape);
                let weight = self.ot.get_pair_weight(self.current_pk, *other_pk);
                overlap * weight
            }
            HazardEntity::BinExterior => {
                let overlap = bin_overlap_proxy(&self.shape_buff, self.layout.bin.bbox());
                let weight = self.ot.get_bin_weight(self.current_pk);
                2.0 * overlap * weight
            }
            _ => unimplemented!("unsupported hazard entity"),
        }).sum()
    }
}