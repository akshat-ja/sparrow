extern crate core;

use std::path::Path;
use std::time::Instant;
use jagua_rs::entities::instances::instance::Instance;
use jagua_rs::entities::problems::strip_packing::SPProblem;
use jagua_rs::io::parser::Parser;
use jagua_rs::util::config::{CDEConfig, SPSurrogateConfig};
use jagua_rs::util::polygon_simplification::PolySimplConfig;
use log::warn;
use mimalloc::MiMalloc;
use once_cell::sync::Lazy;
use rand::prelude::SmallRng;
use rand::SeedableRng;
use gls_strip_packing::io;
use gls_strip_packing::io::svg_util::{SvgDrawOptions, SvgLayoutTheme};
use gls_strip_packing::opt::gls_optimizer::GLSOptimizer;

const INPUT_FILE: &str = "../jagua-rs/assets/shirts.json";



//const RNG_SEED: Option<usize> = Some(12079827122912017592);

const RNG_SEED: Option<usize> = None;

fn main() {

    if cfg!(debug_assertions) {
        io::init_logger(log::LevelFilter::Debug);
    }
    else {
        io::init_logger(log::LevelFilter::Info);
    }

    let json_instance = io::read_json_instance(Path::new(&INPUT_FILE));
    
    let cde_config = CDEConfig{
        quadtree_depth: 4,
        hpg_n_cells: 2000,
        item_surrogate_config: SPSurrogateConfig {
            pole_coverage_goal: 0.95,
            max_poles: 20,
            n_ff_poles: 2,
            n_ff_piers: 0,
        },
    };

    let parser = Parser::new(PolySimplConfig::Disabled, cde_config, true);
    let instance = parser.parse(&json_instance);

    let sp_instance = match instance.clone(){
        Instance::SP(spi) => spi,
        _ => panic!("Expected SPInstance"),
    };

    let rng = match RNG_SEED {
        Some(seed) => SmallRng::seed_from_u64(seed as u64),
        None => {
            let seed = rand::random();
            warn!("No seed provided, using: {}", seed);
            SmallRng::seed_from_u64(seed)
        }
    };

    let problem= SPProblem::new(sp_instance.clone(), 70.0, cde_config);

    let mut gls_opt = GLSOptimizer::new(problem, sp_instance, rng);

    let solution = gls_opt.solve();
    
    println!("Hello, world!");
}
