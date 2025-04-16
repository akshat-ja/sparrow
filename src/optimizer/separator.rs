use crate::config::{DRAW_OPTIONS, LIVE_DIR};
use crate::optimizer::worker::{SepStats, SeparatorWorker};
use crate::optimizer::Terminator;
use crate::quantify::tracker::{CTSnapshot, CollisionTracker};
use crate::sample::search::SampleConfig;
use crate::util::assertions::tracker_matches_layout;
use crate::util::io;
use crate::util::io::layout_to_svg::{layout_to_svg, s_layout_to_svg};
use crate::{EXPORT_LIVE_SVG, EXPORT_ONLY_FINAL_SVG, FMT};
use itertools::Itertools;
use jagua_rs::entities::general::PItemKey;
use jagua_rs::entities::strip_packing::{SPInstance, SPPlacement, SPProblem, SPSolution};
use jagua_rs::geometry::DTransformation;
use log::{debug, log, Level};
use ordered_float::OrderedFloat;
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use rayon::iter::IntoParallelRefMutIterator;
use rayon::iter::ParallelIterator;
use rayon::ThreadPool;
use std::path::Path;
use std::time::Instant;
use jagua_rs::geometry::geo_traits::Shape;

pub struct SeparatorConfig {
    pub iter_no_imprv_limit: usize,
    pub strike_limit: usize,
    pub n_workers: usize,
    pub log_level: Level,
    pub sample_config: SampleConfig,
}

pub struct Separator {
    pub instance: SPInstance,
    pub rng: SmallRng,
    pub prob: SPProblem,
    pub ct: CollisionTracker,
    pub workers: Vec<SeparatorWorker>,
    pub svg_counter: usize,
    pub output_svg_folder: String,
    pub config: SeparatorConfig,
    pub pool: ThreadPool,
}

impl Separator {
    pub fn new(instance: SPInstance, prob: SPProblem, mut rng: SmallRng, output_svg_folder: String, svg_counter: usize, config: SeparatorConfig) -> Self {
        let ct = CollisionTracker::new(&prob.layout);
        let workers = (0..config.n_workers).map(|_|
            SeparatorWorker {
                instance: instance.clone(),
                prob: prob.clone(),
                ct: ct.clone(),
                rng: SmallRng::seed_from_u64(rng.random()),
                sample_config: config.sample_config.clone(),
            }).collect();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(config.n_workers).build().unwrap();

        Self {
            prob,
            instance,
            rng,
            ct,
            workers,
            svg_counter,
            output_svg_folder,
            config,
            pool,
        }
    }

    pub fn separate(&mut self, term: &Terminator) -> (SPSolution, CTSnapshot) {
        let mut min_loss_sol = (self.prob.save(), self.ct.save());
        let mut min_loss = self.ct.get_total_loss();
        log!(self.config.log_level,"[SEP] separating at width: {:.3} and loss: {} ", self.prob.strip_width(), FMT.fmt2(min_loss));

        let mut n_strikes = 0;
        let mut n_iter = 0;
        let mut sep_stats = SepStats { total_moves: 0, total_evals: 0 };
        let start = Instant::now();

        'outer: while n_strikes < self.config.strike_limit && !term.is_kill() {
            let mut n_iter_no_improvement = 0;

            let initial_strike_loss = self.ct.get_total_loss();
            debug!("[SEP] [s:{n_strikes},i:{n_iter}]     init_l: {}",FMT.fmt2(initial_strike_loss));

            while n_iter_no_improvement < self.config.iter_no_imprv_limit {
                let (loss_before, w_loss_before) = (
                    self.ct.get_total_loss(),
                    self.ct.get_total_weighted_loss(),
                );
                sep_stats += self.move_colliding_items();
                let (loss, w_loss) = (
                    self.ct.get_total_loss(),
                    self.ct.get_total_weighted_loss(),
                );

                debug!("[SEP] [s:{n_strikes},i:{n_iter}] ( ) l: {} -> {}, wl: {} -> {}, (min l: {})", FMT.fmt2(loss_before), FMT.fmt2(loss), FMT.fmt2(w_loss_before), FMT.fmt2(w_loss), FMT.fmt2(min_loss));
                debug_assert!(w_loss <= w_loss_before * 1.001, "weighted loss should not increase: {} -> {}", FMT.fmt2(w_loss), FMT.fmt2(w_loss_before));

                if loss == 0.0 {
                    //layout is successfully separated
                    log!(self.config.log_level,"[SEP] [s:{n_strikes},i:{n_iter}] (S)  min_l: {}",FMT.fmt2(loss));
                    min_loss_sol = (self.prob.save(), self.ct.save());
                    break 'outer;
                } else if loss < min_loss {
                    //layout is not separated, but absolute loss is better than before
                    log!(self.config.log_level,"[SEP] [s:{n_strikes},i:{n_iter}] (*) min_l: {}",FMT.fmt2(loss));
                    self.export_svg(None, "i", true);
                    if loss < min_loss * 0.98 {
                        //only reset the iter_no_improvement counter if the loss improved significantly
                        n_iter_no_improvement = 0;
                    }
                    min_loss_sol = (self.prob.save(), self.ct.save());
                    min_loss = loss;
                } else {
                    n_iter_no_improvement += 1;
                }

                self.ct.increment_weights();
                n_iter += 1;
            }

            if initial_strike_loss * 0.98 <= min_loss {
                n_strikes += 1;
            } else {
                n_strikes = 0;
            }
            self.rollback(&min_loss_sol.0, Some(&min_loss_sol.1));
        }
        let secs = start.elapsed().as_secs_f32();
        log!(self.config.log_level, "[SEP] finished, evals/s: {}, evals/move: {}, moves/s: {}, iter/s: {}, #workers: {}, total {:.3}s",
            FMT.fmt2(sep_stats.total_evals as f32 / secs),
            FMT.fmt2(sep_stats.total_evals as f32 / sep_stats.total_moves as f32),
            FMT.fmt2(sep_stats.total_moves as f32 / secs),
            FMT.fmt2(n_iter as f32 / secs),
            self.workers.len(),
            FMT.fmt2(secs),
        );

        (min_loss_sol.0, min_loss_sol.1)
    }

    fn move_colliding_items(&mut self) -> SepStats {
        let master_sol = self.prob.save();

        // Use the local thread pool (instead of global one) to maximize cache locality
        let sep_report = self.pool.install(|| {
            self.workers.par_iter_mut().map(|worker| {
                // Sync the workers with the master
                worker.load(&master_sol, &self.ct);
                // Let them modify
                worker.separate()
            }).sum()
        });

        debug!("[MOD] optimizers w_o's: {:?}",self.workers.iter().map(|opt| opt.ct.get_total_weighted_loss()).collect_vec());

        // Check which worker has the lowest total weighted loss
        let best_opt = self.workers.iter_mut()
            .min_by_key(|opt| OrderedFloat(opt.ct.get_total_weighted_loss()))
            .map(|opt| (opt.prob.save(), &opt.ct))
            .unwrap();

        // Sync the master with the best optimizer
        self.prob.restore(&best_opt.0);
        self.ct = best_opt.1.clone();

        sep_report
    }

    pub fn rollback(&mut self, sol: &SPSolution, ots: Option<&CTSnapshot>) {
        debug_assert!(sol.strip_width == self.prob.strip_width());
        self.prob.restore(sol);

        match ots {
            Some(ots) => {
                //if a snapshot of the tracker was provided, restore it
                self.ct.restore_but_keep_weights(ots, &self.prob.layout);
            }
            None => {
                //otherwise, rebuild it
                self.ct = CollisionTracker::new(&self.prob.layout);
            }
        }
    }

    pub fn move_item(&mut self, pk: PItemKey, d_transf: DTransformation) -> PItemKey {
        debug_assert!(tracker_matches_layout(&self.ct, &self.prob.layout));

        let item_id = self.prob.layout.placed_items()[pk].item_id;

        let old_loss = self.ct.get_loss(pk);
        let old_weighted_loss = self.ct.get_weighted_loss(pk);

        //Remove the item from the problem
        self.prob.remove_item(pk, true);

        //Place the item again but with a new transformation
        let new_pk = self.prob.place_item(SPPlacement{d_transf,item_id});

        self.ct.register_item_move(&self.prob.layout, pk, new_pk);

        let new_loss = self.ct.get_loss(new_pk);
        let new_weighted_loss = self.ct.get_weighted_loss(new_pk);

        debug!("[MV] moved item {} from from l: {}, wl: {} to l+1: {}, wl+1: {}"
            ,item_id,FMT.fmt2(old_loss),FMT.fmt2(old_weighted_loss),FMT.fmt2(new_loss),FMT.fmt2(new_weighted_loss));

        debug_assert!(tracker_matches_layout(&self.ct, &self.prob.layout));

        new_pk
    }

    pub fn change_strip_width(&mut self, new_width: f32, split_position: Option<f32>) {
        //if no split position is provided, use the center of the strip
        let split_position = split_position.unwrap_or(self.prob.strip_width() / 2.0);
        let delta = new_width - self.prob.strip_width();

        //shift all items right of the split position
        let items_to_shift = self.prob.layout.placed_items().iter()
            .filter(|(_, pi)| pi.shape.centroid().0 > split_position)
            .map(|(k, pi)| (k, pi.d_transf))
            .collect_vec();

        for (pik, dtransf) in items_to_shift {
            let existing_transf = dtransf.compose();
            let new_transf = existing_transf.translate((delta, 0.0));
            self.move_item(pik, new_transf.decompose());
        }

        self.prob.change_strip_width(new_width);

        //rebuild the collision tracker
        self.ct = CollisionTracker::new(&self.prob.layout);

        //rebuild the workers
        self.workers.iter_mut().for_each(|opt| {
            *opt = SeparatorWorker {
                instance: self.instance.clone(),
                prob: self.prob.clone(),
                ct: self.ct.clone(),
                rng: SmallRng::seed_from_u64(self.rng.random()),
                sample_config: self.config.sample_config.clone(),
            };
        });
        debug!("[SEP] changed strip width to {:.3}", new_width);
    }

    pub fn export_svg(&mut self, solution: Option<SPSolution>, suffix: &str, only_live: bool) {
        if !EXPORT_ONLY_FINAL_SVG {
            if self.svg_counter == 0 {
                std::fs::create_dir_all(&self.output_svg_folder).unwrap();
                //remove all svg files from the directory. ONLY SVG FILES
                for file in std::fs::read_dir(&self.output_svg_folder).unwrap().flatten() {
                    if file.path().extension().unwrap_or_default() == "svg" {
                        std::fs::remove_file(file.path()).unwrap();
                    }
                }
            }

            let file_name = format!("{}_{:.3}_{suffix}", self.svg_counter, self.prob.strip_width());
            let svg = match solution {
                Some(sol) => s_layout_to_svg(&sol.layout_snapshot, &self.instance, DRAW_OPTIONS, file_name.as_str()),
                None => layout_to_svg(&self.prob.layout, &self.instance, DRAW_OPTIONS, file_name.as_str()),
            };

            if EXPORT_LIVE_SVG {
                if !Path::new(LIVE_DIR).exists() {
                    std::fs::create_dir_all(LIVE_DIR).unwrap();
                }
                io::write_svg(&svg, Path::new(&*format!("{}/.live_solution.svg", LIVE_DIR)), Level::Trace);
            }

            if !only_live {
                let file_path = &*format!("{}/{}.svg", &self.output_svg_folder, file_name);
                io::write_svg(&svg, Path::new(file_path), self.config.log_level);
                self.svg_counter += 1;
            }
        }
    }
}
