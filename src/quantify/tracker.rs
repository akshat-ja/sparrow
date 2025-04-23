use jagua_rs::collision_detection::hazards::detector::{BasicHazardDetector, HazardDetector};
use jagua_rs::collision_detection::hazards::HazardEntity;
use jagua_rs::entities::general::{Layout, PItemKey};
use crate::config::{WEIGHT_DECAY, WEIGHT_MAX_INC_RATIO, WEIGHT_MIN_INC_RATIO};
use crate::quantify::pair_matrix::PairMatrix;
use crate::quantify::{quantify_collision_poly_bin, quantify_collision_poly_poly};
use crate::util::assertions::tracker_matches_layout;
use ordered_float::Float;
use slotmap::SecondaryMap;

/// Tracker of both collisions between pair of items and collisions with the bin.
/// It also stores the weights for every pair of hazards and is used as a cache for collisions.
#[derive(Debug, Clone)]
pub struct CollisionTracker {
    pub size: usize,
    pub pk_idx_map: SecondaryMap<PItemKey, usize>,
    pub pair_collisions: PairMatrix,
    pub bin_collisions: Vec<CTEntry>,
}

pub type CTSnapshot = CollisionTracker;

impl CollisionTracker {
    pub fn new(l: &Layout) -> Self {
        let size = l.placed_items.len();

        // Create the tracker
        let mut ot = Self {
            size,
            pk_idx_map: l.placed_items.keys().enumerate()
                .map(|(i, pk)| (pk, i))
                .collect(),
            pair_collisions: PairMatrix::new(size),
            bin_collisions: vec![CTEntry { weight: 1.0, loss: 0.0 }; size],
        };

        // Recompute the loss for all items
        l.placed_items.keys().for_each(|pk| {
            ot.recompute_loss_for_item(pk, l)
        });

        debug_assert!(tracker_matches_layout(&ot, l));

        ot
    }

    fn recompute_loss_for_item(&mut self, pk: PItemKey, l: &Layout) {
        let idx = self.pk_idx_map[pk];
        let pi = &l.placed_items[pk];
        let shape = pi.shape.as_ref();

        // Reset all current loss values for the item
        for i in 0..self.size {
            self.pair_collisions[(idx, i)].loss = 0.0;
        }
        self.bin_collisions[idx].loss = 0.0;

        // Compute which hazards are currently colliding with the item
        let mut detector = BasicHazardDetector::new();
        l.cde().collect_poly_collisions(shape, &mut detector);
        // Remove the item itself from the detector
        detector.remove(&HazardEntity::from((pk, pi)));

        // For each colliding hazard, quantify the collision and store it in the tracker
        for haz in detector.iter() {
            match haz {
                HazardEntity::PlacedItem { pk: other_pk, .. } => {
                    let shape_other = &l.placed_items[*other_pk].shape;
                    let idx_other = self.pk_idx_map[*other_pk];

                    let loss = quantify_collision_poly_poly(shape, shape_other);
                    assert!(loss > 0.0, "loss for a collision should be > 0.0");
                    self.pair_collisions[(idx, idx_other)].loss = loss;
                }
                HazardEntity::BinExterior => {
                    let loss = quantify_collision_poly_bin(shape, l.bin.outer_cd.bbox);
                    assert!(loss > 0.0, "loss for a collision should be > 0.0");
                    self.bin_collisions[idx].loss = loss;
                }
                _ => unimplemented!("unsupported hazard entity"),
            }
        }
    }

    pub fn restore_but_keep_weights(&mut self, cts: &CTSnapshot, layout: &Layout) {
        //Copy the loss and keys, but keep the weights
        self.pk_idx_map = cts.pk_idx_map.clone();
        self.pair_collisions.data.iter_mut()
            .zip(cts.pair_collisions.data.iter())
            .for_each(|(a, b)| a.loss = b.loss);
        self.bin_collisions.iter_mut()
            .zip(cts.bin_collisions.iter())
            .for_each(|(a, b)| a.loss = b.loss);
        debug_assert!(tracker_matches_layout(self, layout));
    }

    pub fn save(&self) -> CTSnapshot {
        self.clone()
    }

    pub fn register_item_move(&mut self, l: &Layout, old_pk: PItemKey, new_pk: PItemKey) {
        //swap the keys in the pk_idx_map
        let idx = self.pk_idx_map.remove(old_pk).unwrap();
        self.pk_idx_map.insert(new_pk, idx);

        self.recompute_loss_for_item(new_pk, l);

        debug_assert!(tracker_matches_layout(self, l));
    }

    pub fn increment_weights(&mut self) {
        let max_o = self.pair_collisions.data.iter()
            .map(|e| e.loss)
            .fold(0.0, |a, b| a.max(b));

        for e in self.pair_collisions.data.iter_mut() {
            let multiplier = match e.loss == 0.0 {
                true => WEIGHT_DECAY, // no collision
                false => WEIGHT_MIN_INC_RATIO + (WEIGHT_MAX_INC_RATIO - WEIGHT_MIN_INC_RATIO) * (e.loss / max_o),
            };
            e.weight = (e.weight * multiplier).max(1.0);
        }

        for e in self.bin_collisions.iter_mut() {
            let multiplier = match e.loss == 0.0 {
                true => WEIGHT_DECAY, // no collision
                false => WEIGHT_MAX_INC_RATIO,
            };
            e.weight = (e.weight * multiplier).max(1.0);
        }
    }

    pub fn get_pair_weight(&self, pk1: PItemKey, pk2: PItemKey) -> f32 {
        let (idx1, idx2) = (self.pk_idx_map[pk1], self.pk_idx_map[pk2]);
        self.pair_collisions[(idx1, idx2)].weight
    }

    pub fn get_bin_weight(&self, pk: PItemKey) -> f32 {
        let idx = self.pk_idx_map[pk];
        self.bin_collisions[idx].weight
    }

    pub fn get_pair_loss(&self, pk1: PItemKey, pk2: PItemKey) -> f32 {
        let (idx1, idx2) = (self.pk_idx_map[pk1], self.pk_idx_map[pk2]);
        self.pair_collisions[(idx1, idx2)].loss
    }

    pub fn get_bin_loss(&self, pk: PItemKey) -> f32 {
        let idx = self.pk_idx_map[pk];
        self.bin_collisions[idx].loss
    }

    pub fn get_loss(&self, pk: PItemKey) -> f32 {
        let idx = self.pk_idx_map[pk];

        let pair_loss = (0..self.size)
            .map(|i| self.pair_collisions[(idx, i)].loss)
            .sum::<f32>();

        self.bin_collisions[idx].loss + pair_loss
    }

    pub fn get_weighted_loss(&self, pk: PItemKey) -> f32 {
        let idx = self.pk_idx_map[pk];

        let w_pair_loss = (0..self.size)
            .map(|i| self.pair_collisions[(idx, i)].weighted_loss())
            .sum::<f32>();

        self.bin_collisions[idx].weighted_loss() + w_pair_loss
    }

    pub fn get_total_loss(&self) -> f32 {
        let bin_o = self.bin_collisions.iter().map(|e| e.loss).sum::<f32>();

        let pair_o = self.pair_collisions.data.iter()
            .map(|e| e.loss)
            .sum::<f32>();

        bin_o + pair_o
    }

    pub fn get_total_weighted_loss(&self) -> f32 {
        let bin_w_o = self.bin_collisions.iter()
            .map(|e| e.weighted_loss())
            .sum::<f32>();

        let pair_w_o = self.pair_collisions.data.iter()
            .map(|e| e.weighted_loss())
            .sum::<f32>();

        bin_w_o + pair_w_o
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CTEntry {
    pub loss: f32,
    pub weight: f32,
}

impl CTEntry {
    pub fn weighted_loss(&self) -> f32 {
        self.weight * self.loss
    }
}