pub mod asset_graph;
pub mod cache;
pub mod diagnostic;
pub mod environment;
mod intern;
pub mod parcel_config;
pub mod request_tracker;
pub mod requests;
pub mod transformers;
pub mod types;
pub mod worker_farm;

use asset_graph::{AssetGraph, AssetGraphRequest};
use diagnostic::Diagnostic;
use environment::reset_env_interner;
use request_tracker::{FileEvent, Request, RequestTracker};
use types::ParcelOptions;
use worker_farm::WorkerFarm;

use crate::requests::parcel_config_request::ParcelConfigRequest;

struct Parcel {
  request_tracker: RequestTracker,
  entries: Vec<String>,
  farm: WorkerFarm,
  options: ParcelOptions,
}

impl Parcel {
  pub fn new(entries: Vec<String>, farm: WorkerFarm, options: ParcelOptions) -> Self {
    Parcel {
      request_tracker: RequestTracker::new(),
      entries,
      farm,
      options,
    }
  }

  pub fn build(&mut self, events: Vec<FileEvent>) -> Result<AssetGraph, Vec<Diagnostic>> {
    self.request_tracker.next_build(events);

    let config = ParcelConfigRequest {}
      .run(&self.farm, &self.options)
      .result
      .unwrap();

    let mut req = AssetGraphRequest {
      entries: &self.entries,
      transformers: &config.transformers,
      resolvers: &config.resolvers,
    };
    let asset_graph = req.build(&mut self.request_tracker, &self.farm, &self.options);

    asset_graph
  }
}

pub fn build(
  entries: Vec<String>,
  farm: WorkerFarm,
  options: ParcelOptions,
) -> Result<AssetGraph, Vec<Diagnostic>> {
  // TODO: this is a hack to fix the tests.
  // Environments don't include the source location in their hash,
  // and this results in interned envs being reused between tests.
  reset_env_interner();

  let mut parcel = Parcel::new(entries, farm, options);
  parcel.build(vec![])
}
