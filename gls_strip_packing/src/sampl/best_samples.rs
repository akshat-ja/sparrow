use jagua_rs::fsize;
use jagua_rs::geometry::d_transformation::DTransformation;
use jagua_rs::geometry::transformation::Transformation;
use log::debug;
use std::fmt::Debug;
use crate::sampl::eval::SampleEval;
use crate::sampl::search;

//datastructure that stores the N best samples, automatically keeps them sorted and evicts the worst
#[derive(Debug, Clone)]
pub struct BestSamples {
    pub samples: Vec<(DTransformation, SampleEval)>,
    pub capacity: usize,
    pub unique_threshold: fsize,
}

impl BestSamples {
    pub fn new(capacity: usize, unique_threshold: fsize) -> Self {
        Self {
            samples: Vec::with_capacity(capacity),
            capacity,
            unique_threshold,
        }
    }

    pub fn report(&mut self, dt: DTransformation, eval: SampleEval) -> bool {
        let mut modified = false;
        if self.samples.iter().all(|(d, _)| search::d_transfs_are_unique(*d, dt, self.unique_threshold)){
            if self.samples.len() < self.capacity {
                debug!("sample added to bests: {:?}", &eval);
                self.samples.push((dt, eval));
                modified = true;
            } else {
                let worst = self.samples.last().unwrap();
                if eval < worst.1 {
                    debug!("sample added to bests: {:?}", &eval);
                    self.samples.pop();
                    self.samples.push((dt, eval));
                    modified = true;
                }
            }
            if modified {
                self.samples
                    .sort_by(|a, b| a.1.cmp(&b.1));
            } else {
                //debug!("sample not added to bests");
            }
        }
        modified
    }

    pub fn best(&self) -> Option<&(DTransformation, SampleEval)> {
        self.samples.first()
    }

    pub fn take_best(self) -> Option<(DTransformation, SampleEval)> {
        self.samples.into_iter().next()
    }

    pub fn worst(&self) -> Option<&(DTransformation, SampleEval)> {
        self.samples.last()
    }

    pub fn upper_bound(&self) -> Option<SampleEval> {
        if self.samples.len() == self.capacity {
            self.samples.last().map(|x| x.1.clone())
        } else {
            None
        }
    }
}
