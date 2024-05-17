#![doc(html_favicon_url = "https://raw.githubusercontent.com/zng-ui/zng/master/examples/res/image/zng-logo-icon.png")]
#![doc(html_logo_url = "https://raw.githubusercontent.com/zng-ui/zng/master/examples/res/image/zng-logo.png")]
//!
//! Hot reload service.
//!
//! # Crate
//!
#![doc = include_str!(concat!("../", std::env!("CARGO_PKG_README")))]
#![warn(unused_extern_crates)]
#![warn(missing_docs)]

mod cargo;
mod node;
mod util;
use std::{
    collections::{HashMap, HashSet},
    fmt,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

pub use cargo::BuildError;
use node::*;

use zng_app::{
    event::{event, event_args},
    update::UPDATES,
    AppExtension, DInstant, INSTANT,
};
use zng_app_context::{app_local, LocalContext};
use zng_ext_fs_watcher::WATCHER;
pub use zng_ext_hot_reload_proc_macros::hot_node;
use zng_hot_entry::HotRequest;
use zng_task::parking_lot::Mutex;
use zng_txt::Txt;
use zng_unique_id::hot_reload::HOT_STATICS;
use zng_var::{ArcVar, ReadOnlyArcVar, ResponseVar, Var as _};

#[doc(inline)]
pub use zng_unique_id::{hot_static, hot_static_ref, lazy_static};

/// Declare hot reload entry.
///
/// Must be called at the root of the crate.
#[macro_export]
macro_rules! zng_hot_entry {
    () => {
        #[doc(hidden)] // used by proc-macro
        pub use $crate::zng_hot_entry;

        #[no_mangle]
        #[doc(hidden)] // used by lib loader
        pub extern "C" fn zng_hot_entry(request: $crate::zng_hot_entry::HotRequest) -> Option<$crate::zng_hot_entry::HotNode> {
            $crate::zng_hot_entry::entry(request)
        }

        #[no_mangle]
        #[doc(hidden)]
        pub extern "C" fn zng_hot_entry_init(patch: &$crate::StaticPatch) {
            $crate::zng_hot_entry::init(patch)
        }
    };
}

#[doc(hidden)]
pub mod zng_hot_entry {
    pub use crate::node::{HotNode, HotNodeArgs, HotNodeHost};
    use crate::StaticPatch;
    use zng_app_context::LocalContext;

    pub struct HotNodeEntry {
        pub manifest_dir: &'static str,
        pub hot_node_name: &'static str,
        pub hot_node_fn: fn(HotNodeArgs) -> HotNode,
    }

    #[linkme::distributed_slice]
    pub static HOT_NODES: [HotNodeEntry];

    pub struct HotRequest {
        pub manifest_dir: String,
        pub hot_node_name: &'static str,
        pub ctx: LocalContext,
        pub args: HotNodeArgs,
    }

    pub fn entry(mut request: HotRequest) -> Option<crate::HotNode> {
        for entry in HOT_NODES.iter() {
            if request.hot_node_name == entry.hot_node_name && request.manifest_dir == entry.manifest_dir {
                return request.ctx.with_context(|| Some((entry.hot_node_fn)(request.args)));
            }
        }
        None
    }

    pub fn init(statics: &StaticPatch) {
        tracing::dispatcher::set_global_default(statics.tracing.clone()).unwrap();

        std::panic::set_hook(Box::new(|args| {
            eprintln!("PANIC IN HOT LOADED LIBRARY, ABORTING");
            crate::util::crash_handler(args);
            std::process::exit(101);
        }));

        // SAFETY: hot reload rebuilds in the same environment, so this is safe if the keys are strong enough.
        unsafe { statics.apply() }
    }
}

#[doc(hidden)]
#[derive(Default, Clone)]
pub struct StaticPatch {
    entries: HashMap<&'static dyn zng_unique_id::hot_reload::PatchKey, unsafe fn(*const ()) -> *const ()>,
    tracing: tracing::dispatcher::Dispatch,
}
impl StaticPatch {
    /// Called on the static code (host).
    fn capture_statics(&mut self) {
        if !self.entries.is_empty() {
            return;
        }
        self.entries.reserve(HOT_STATICS.len());

        for (key, val) in HOT_STATICS.iter() {
            match self.entries.entry(*key) {
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(*val);
                }
                std::collections::hash_map::Entry::Occupied(_) => {
                    panic!("repeated hot static key `{key:?}`");
                }
            }
        }
    }

    /// Called on the dynamic code (dylib).
    unsafe fn apply(&self) {
        for (key, patch) in HOT_STATICS.iter() {
            if let Some(val) = self.entries.get(key) {
                // println!("patched `{key:?}`");
                patch(val(std::ptr::null()));
            } else {
                eprintln!("did not find `{key:?}` to patch, static references may fail");
            }
        }
    }
}

/// Status of a monitored dynamic library crate.
#[derive(Clone, PartialEq, Debug)]
pub struct HotStatus {
    /// Dynamic library crate directory.
    ///
    /// Any file changes inside this directory triggers a rebuild.
    pub manifest_dir: Txt,

    /// Build start time if is rebuilding.
    pub building: Option<DInstant>,

    /// Last rebuild result.
    ///
    /// is `Ok(build_duration)` or `Err(build_error)`.
    pub last_build: Result<Duration, BuildError>,

    /// Number of times the dynamically library was rebuild (successfully and with error).
    pub rebuild_count: usize,
}

/// Hot reload app extension.
///
/// # Events
///
/// Events this extension provides.
///
/// * [`HOT_RELOAD_EVENT`]
///
/// # Services
///
/// Services this extension provides.
///
/// * [`HOT_RELOAD`]
#[derive(Default)]
pub struct HotReloadManager {
    libs: HashMap<&'static str, WatchedLib>,
    static_patch: StaticPatch,
}
impl AppExtension for HotReloadManager {
    fn init(&mut self) {
        // capture global tracing dispatcher early.
        self.static_patch.tracing = tracing::dispatcher::get_default(|d| d.clone());

        // watch all hot libraries.
        let mut status = vec![];
        for entry in crate::zng_hot_entry::HOT_NODES.iter() {
            if let std::collections::hash_map::Entry::Vacant(e) = self.libs.entry(entry.manifest_dir) {
                e.insert(WatchedLib::default());
                WATCHER.watch_dir(entry.manifest_dir, true).perm();

                status.push(HotStatus {
                    manifest_dir: entry.manifest_dir.into(),
                    building: None,
                    last_build: Ok(Duration::MAX),
                    rebuild_count: 0,
                });
            }
        }
        HOT_RELOAD_SV.read().status.set(status);
    }

    fn event_preview(&mut self, update: &mut zng_app::update::EventUpdate) {
        if let Some(args) = zng_ext_fs_watcher::FS_CHANGES_EVENT.on(update) {
            for (manifest_dir, watched) in self.libs.iter_mut() {
                if args.changes_for_path(manifest_dir.as_ref()).next().is_some() {
                    self.static_patch.capture_statics();

                    watched.rebuild((*manifest_dir).into(), &self.static_patch);
                }
            }
        }
    }

    fn update_preview(&mut self) {
        for (manifest_dir, watched) in self.libs.iter_mut() {
            if let Some(b) = &watched.building {
                if let Some(r) = b.process.rsp() {
                    let build_time = b.start_time.elapsed();
                    let mut lib = None;
                    let status_r = match r {
                        Ok(l) => {
                            lib = Some(l);
                            Ok(build_time)
                        }
                        Err(e) => {
                            tracing::error!("failed rebuild `{manifest_dir}`, {e}");
                            Err(e)
                        }
                    };
                    if let Some(lib) = lib {
                        tracing::info!("rebuilt and reloaded `{manifest_dir}` in {build_time:?}");
                        HOT_RELOAD.set(lib.clone());
                        HOT_RELOAD_EVENT.notify(HotReloadArgs::now(lib));
                    }

                    watched.building = None;

                    let manifest_dir = *manifest_dir;
                    HOT_RELOAD_SV.read().status.modify(move |s| {
                        let s = s.to_mut().iter_mut().find(|s| s.manifest_dir == manifest_dir).unwrap();
                        s.building = None;
                        s.last_build = status_r;
                        s.rebuild_count += 1;
                    });
                }
            }
        }

        let mut sv = HOT_RELOAD_SV.write();
        let requests: HashSet<Txt> = sv.rebuild_requests.drain(..).collect();
        for r in requests {
            if let Some(watched) = self.libs.get_mut(r.as_str()) {
                self.static_patch.capture_statics();
                watched.rebuild(r, &self.static_patch);
            } else {
                tracing::error!("cannot rebuild `{r}`, unknown");
            }
        }
    }
}

type RebuildVar = ResponseVar<Result<PathBuf, BuildError>>;

type RebuildLoadVar = ResponseVar<Result<HotLib, BuildError>>;

/// Arguments for custom rebuild runners.
///
/// See [`HOT_RELOAD.rebuilder`] for more details.
///
/// [`HOT_RELOAD.rebuilder`]: HOT_RELOAD::rebuilder
#[derive(Clone, Debug, PartialEq)]
pub struct BuildArgs {
    /// Crate that changed.
    pub manifest_dir: Txt,
}
impl BuildArgs {
    /// Calls `cargo build [--package {package}] --message-format json` and cancels it as soon as the dylib is rebuilt.
    pub fn build(&self, package: Option<&str>) -> Option<RebuildVar> {
        Some(cargo::build(&self.manifest_dir, package.unwrap_or(""), "", ""))
    }

    /// Calls `cargo build [--package {package}] --example {example} --message-format json` and cancels
    /// it as soon as the dylib is rebuilt.
    pub fn build_example(&self, package: Option<&str>, example: &str) -> Option<RebuildVar> {
        Some(cargo::build(&self.manifest_dir, package.unwrap_or(""), "--example", example))
    }

    /// Calls `cargo build [--package {package}] --bin {bin}  --message-format json` and cancels it as
    /// soon as the dylib is rebuilt.
    pub fn build_bin(&self, package: Option<&str>, bin: &str) -> Option<RebuildVar> {
        Some(cargo::build(&self.manifest_dir, package.unwrap_or(""), "--bin", bin))
    }
}

/// Hot reload service.
#[allow(non_camel_case_types)]
pub struct HOT_RELOAD;
impl HOT_RELOAD {
    /// Hot reload status, libs that are rebuilding, errors.
    pub fn status(&self) -> ReadOnlyArcVar<Vec<HotStatus>> {
        HOT_RELOAD_SV.read().status.read_only()
    }

    /// Register a handler that can override the hot library rebuild.
    ///
    /// The command should rebuild using the same features used to run the program (not just rebuild the dylib).
    /// By default it is just `cargo build`, that works if the program was started using only `cargo run`, but
    /// an example program needs a custom runner.
    ///
    /// If `rebuilder` wants to handle the rebuild it must return a response var that updates when the rebuild is finished with
    /// the path to the rebuilt dylib. The [`BuildArgs`] also provides helper methods to rebuild common workspace setups.
    ///
    /// Note that unlike most services the `rebuilder` is registered immediately, not after an update cycle.
    pub fn rebuilder(&self, rebuilder: impl FnMut(BuildArgs) -> Option<RebuildVar> + Send + 'static) {
        HOT_RELOAD_SV.write().rebuilders.get_mut().push(Box::new(rebuilder));
    }

    /// Request a rebuild, if `manifest_dir` is a hot library.
    ///
    /// Note that changes inside the directory already trigger a rebuild automatically.
    pub fn rebuild(&self, manifest_dir: impl Into<Txt>) {
        HOT_RELOAD_SV.write().rebuild_requests.push(manifest_dir.into());
        UPDATES.update(None);
    }

    pub(crate) fn lib(&self, manifest_dir: &'static str) -> Option<HotLib> {
        HOT_RELOAD_SV
            .read()
            .libs
            .iter()
            .rev()
            .find(|l| l.manifest_dir() == manifest_dir)
            .cloned()
    }

    fn set(&self, lib: HotLib) {
        // we never unload HotLib because hot nodes can pass &'static references (usually inside `Txt`) to the
        // program that will remain being used after.
        HOT_RELOAD_SV.write().libs.push(lib);
    }
}
app_local! {
    static HOT_RELOAD_SV: HotReloadService = {
        HotReloadService {
            libs: vec![],
            rebuilders: Mutex::new(vec![]),
            status: zng_var::var(vec![]),
            rebuild_requests: vec![] ,
        }
    };
}
struct HotReloadService {
    libs: Vec<HotLib>,
    // mutex for Sync only
    #[allow(clippy::type_complexity)]
    rebuilders: Mutex<Vec<Box<dyn FnMut(BuildArgs) -> Option<RebuildVar> + Send + 'static>>>,

    status: ArcVar<Vec<HotStatus>>,
    rebuild_requests: Vec<Txt>,
}
impl HotReloadService {
    fn rebuild_reload(&mut self, manifest_dir: Txt, static_patch: StaticPatch) -> RebuildLoadVar {
        let rebuild = self.rebuild(manifest_dir.clone());
        zng_task::respond(async move {
            let mut path = rebuild.wait_into_rsp().await?;

            // copy dylib to not block the next rebuild
            let file_name = match path.file_name() {
                Some(f) => f.to_string_lossy(),
                None => return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "dylib path does not have a file name").into()),
            };
            for i in 0..1000 {
                let mut unblocked_path = path.clone();
                unblocked_path.set_file_name(format!("zng-hot-{i}-{file_name}"));
                if unblocked_path.exists() {
                    // try free for next use
                    let _ = std::fs::remove_file(unblocked_path);
                } else {
                    std::fs::copy(&path, &unblocked_path)?;
                    path = unblocked_path;
                    break;
                }
            }

            let dylib = HotLib::new(&static_patch, manifest_dir, path)?;
            Ok(dylib)
        })
    }

    fn rebuild(&mut self, manifest_dir: Txt) -> RebuildVar {
        let args = BuildArgs { manifest_dir };
        for r in self.rebuilders.get_mut() {
            if let Some(r) = r(args.clone()) {
                return r;
            }
        }
        args.build(None).unwrap()
    }
}

event_args! {
    /// Args for [`HOT_RELOAD_EVENT`].
    pub struct HotReloadArgs {
        /// Reloaded library.
        pub(crate) lib: HotLib,

        ..

        fn delivery_list(&self, list: &mut UpdateDeliveryList) {
            list.search_all();
        }
    }
}
impl HotReloadArgs {
    /// Crate directory that changed and caused the rebuild.
    pub fn manifest_dir(&self) -> &Txt {
        self.lib.manifest_dir()
    }
}

event! {
    /// Event notifies when a new version of a hot reload dynamic library has finished rebuild and has loaded.
    ///
    /// This event is used internally by hot nodes to reinit.
    pub static HOT_RELOAD_EVENT: HotReloadArgs;
}

#[derive(Default)]
struct WatchedLib {
    building: Option<BuildingLib>,
}
impl WatchedLib {
    fn rebuild(&mut self, manifest_dir: Txt, static_path: &StaticPatch) {
        if self.building.is_none() {
            let start_time = INSTANT.now();
            tracing::info!("rebuilding `{manifest_dir}`");

            let mut sv = HOT_RELOAD_SV.write();

            self.building = Some(BuildingLib {
                start_time,
                process: sv.rebuild_reload(manifest_dir.clone(), static_path.clone()),
            });

            sv.status.modify(move |s| {
                s.to_mut().iter_mut().find(|s| s.manifest_dir == manifest_dir).unwrap().building = Some(start_time);
            });
        } else {
            // !!: TODO, cancel?
        }
    }
}

struct BuildingLib {
    start_time: DInstant,
    process: RebuildLoadVar,
}

/// Dynamically loaded library.
#[derive(Clone)]
pub(crate) struct HotLib {
    manifest_dir: Txt,
    lib: Arc<libloading::Library>,
    hot_entry: unsafe fn(HotRequest) -> Option<HotNode>,
}
impl PartialEq for HotLib {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.lib, &other.lib)
    }
}
impl fmt::Debug for HotLib {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HotLib")
            .field("manifest_dir", &self.manifest_dir)
            .finish_non_exhaustive()
    }
}
impl HotLib {
    pub fn new(patch: &StaticPatch, manifest_dir: Txt, lib: impl AsRef<std::ffi::OsStr>) -> Result<Self, libloading::Error> {
        unsafe {
            // SAFETY: assuming the the hot lib was setup as the documented, this works,
            // even the `linkme` stuff does not require any special care.
            //
            // If the hot lib developer add some "ctor/dtor" stuff and that fails they will probably
            // know why, hot reloading should only run in dev machines.
            let lib = libloading::Library::new(lib)?;

            // SAFETY: thats the signature.
            let init: unsafe fn(&StaticPatch) = *lib.get(b"zng_hot_entry_init")?;
            init(patch);

            Ok(Self {
                manifest_dir,
                hot_entry: *lib.get(b"zng_hot_entry")?,
                lib: Arc::new(lib),
            })
        }
    }

    /// Lib identifier.
    pub fn manifest_dir(&self) -> &Txt {
        &self.manifest_dir
    }

    pub fn instantiate(&self, hot_node_name: &'static str, ctx: LocalContext, args: HotNodeArgs) -> Option<HotNode> {
        let request = HotRequest {
            manifest_dir: self.manifest_dir.to_string(),
            hot_node_name,
            ctx,
            args,
        };
        // SAFETY: lib is still loaded and will remain until all HotNodes are dropped.
        let mut r = unsafe { (self.hot_entry)(request) };
        if let Some(n) = &mut r {
            n._lib = Some(self.lib.clone());
        }
        r
    }
}
