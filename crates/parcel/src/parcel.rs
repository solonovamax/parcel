use std::path::PathBuf;
use std::sync::Arc;

use parcel_config::parcel_rc_config_loader::LoadConfigOptions;
use parcel_config::parcel_rc_config_loader::ParcelRcConfigLoader;
use parcel_core::config_loader::ConfigLoader;
use parcel_core::plugin::PluginContext;
use parcel_core::plugin::PluginLogger;
use parcel_core::plugin::PluginOptions;
use parcel_core::types::ParcelOptions;
use parcel_filesystem::os_file_system::OsFileSystem;
use parcel_filesystem::FileSystemRef;
use parcel_package_manager::NodePackageManager;
use parcel_package_manager::PackageManagerRef;
use parcel_plugin_rpc::RpcHostRef;
use parcel_plugin_rpc::RpcWorkerRef;

use crate::plugins::config_plugins::ConfigPlugins;
use crate::project_root::infer_project_root;
use crate::request_tracker::RequestTracker;

pub struct Parcel {
  pub fs: FileSystemRef,
  pub options: ParcelOptions,
  pub package_manager: PackageManagerRef,
  pub project_root: PathBuf,
  pub rpc: Option<RpcHostRef>,
}

impl Parcel {
  pub fn new(
    fs: Option<FileSystemRef>,
    options: ParcelOptions,
    package_manager: Option<PackageManagerRef>,
    rpc: Option<RpcHostRef>,
  ) -> Result<Self, anyhow::Error> {
    let fs = fs.unwrap_or_else(|| Arc::new(OsFileSystem::default()));
    let project_root = infer_project_root(Arc::clone(&fs), options.entries.clone())?;

    let package_manager = package_manager
      .unwrap_or_else(|| Arc::new(NodePackageManager::new(project_root.clone(), fs.clone())));

    Ok(Self {
      fs,
      options,
      package_manager,
      project_root,
      rpc,
    })
  }
}

pub struct BuildResult;

impl Parcel {
  pub fn build(&self) -> anyhow::Result<BuildResult> {
    let mut _rpc_connection = None::<RpcWorkerRef>;

    if let Some(rpc_host) = &self.rpc {
      _rpc_connection = Some(rpc_host.start()?);
    }

    let (config, _files) =
      ParcelRcConfigLoader::new(Arc::clone(&self.fs), Arc::clone(&self.package_manager)).load(
        &self.project_root,
        LoadConfigOptions {
          additional_reporters: vec![], // TODO
          config: self.options.config.as_deref(),
          fallback_config: self.options.fallback_config.as_deref(),
        },
      )?;

    let config_loader = Arc::new(ConfigLoader {
      fs: Arc::clone(&self.fs),
      project_root: self.project_root.clone(),
      search_path: self.project_root.join("index"),
    });

    let plugins = ConfigPlugins::new(
      config,
      PluginContext {
        config: Arc::clone(&config_loader),
        options: Arc::new(PluginOptions {
          mode: self.options.mode.clone(),
          project_root: self.project_root.clone(),
        }),
        // TODO Initialise actual logger
        logger: PluginLogger::default(),
      },
    );

    // TODO Reinstate this when we are in a full build
    // plugins.reporter().report(&ReporterEvent::BuildStart)?;

    let _request_tracker = RequestTracker::new(
      Arc::clone(&config_loader),
      Arc::clone(&self.fs),
      Arc::new(self.options.clone()),
      Arc::new(plugins),
      self.project_root.clone(),
    );

    // TODO: Run asset graph request

    Ok(BuildResult {})
  }
}
