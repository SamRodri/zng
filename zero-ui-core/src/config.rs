//! Config manager.
//!
//! The [`ConfigManager`] is an [app extension], it
//! is included in the [default app] and manages the [`Config`] service that can be used to store and retrieve
//! state that is persisted between application runs.
//!
//! [app extension]: crate::app::AppExtension
//! [default app]: crate::app::App::default

use std::{
    cell::{Cell, RefCell},
    collections::{hash_map::Entry, HashMap, HashSet},
    error::Error,
    fmt,
    rc::Rc,
    sync::Arc,
};

use crate::{
    app::{AppEventSender, AppExtReceiver, AppExtSender, AppExtension},
    context::*,
    service::*,
    text::Text,
    var::*,
};

use serde_json::value::Value as JsonValue;

/// Application extension that manages the app configuration access point ([`Config`]).
///
/// Note that this extension does not implement a [`ConfigSource`], it just manages whatever source is installed and
/// the config variables.
#[derive(Default)]
pub struct ConfigManager {}
impl ConfigManager {}
impl AppExtension for ConfigManager {
    fn init(&mut self, ctx: &mut AppContext) {
        ctx.services.register(Config::new(ctx.updates.sender()));
    }

    fn deinit(&mut self, ctx: &mut AppContext) {
        if let Some((mut source, _)) = Config::req(ctx).source.take() {
            source.deinit();
        }
    }

    fn update_preview(&mut self, ctx: &mut AppContext) {
        Config::req(ctx.services).update(ctx.vars);
    }
}

/// Key to a persistent config in [`Config`].
pub type ConfigKey = Text;

/// A type that can be a [`Config`] value.
///
/// # Trait Alias
///
/// This trait is used like a type alias for traits and is
/// already implemented for all types it applies to.
pub trait ConfigValue: VarValue + PartialEq + serde::Serialize + serde::de::DeserializeOwned {}
impl<T: VarValue + PartialEq + serde::Serialize + serde::de::DeserializeOwned> ConfigValue for T {}

/// Return `true` to retain, `false` to drop.
type ConfigTask = Box<dyn FnMut(&Vars, &RcVar<ConfigStatus>) -> bool>;
type OnceConfigTask = Box<dyn FnOnce(&Vars, &RcVar<ConfigStatus>)>;

/// Represents the configuration of the app.
///
/// This type does not implement any config scheme, a [`ConfigSource`] must be set to enable persistence, without a source
/// only the config variables work, and only for the duration of the app process.
///
/// Note that this is a service *singleton* that represents the config in use by the app, to load other config files
/// you can use the [`Config::load_alt`].
///
/// # Examples
///
/// The example demonstrates loading a config file and binding a config to a variable that is auto saves every time it changes.
///
/// ```no_run
/// # use zero_ui_core::{app::*, window::*, config::*, units::*};
/// # macro_rules! window { ($($tt:tt)*) => { unimplemented!() } }
/// App::default().run_window(|ctx| {
///     // require the Config service, it is available in the default App.
///     let cfg = Config::req(ctx.services);
///
///     // load a ConfigSource.
///     cfg.load(ConfigFile::new("app.config.json", true, 3.secs()));
///     
///     // read the "main.count" config and bind it to a variable.
///     let count = cfg.var("main.count", || 0);
///
///     window! {
///         title = "Persistent Counter";
///         padding = 20;
///         content = button! {
///             content = text(count.map(|c| formatx!("Count: {c}")));
///             on_click = hn!(|ctx, _| {
///                 // modifying the var updates the "main.count" config.
///                 count.modify(ctx, |mut c| *c += 1).unwrap();
///             });
///         }
///     }
/// })
/// ```
#[derive(Service)]
pub struct Config {
    update: AppEventSender,
    source: Option<(Box<dyn ConfigSource>, AppExtReceiver<ConfigSourceUpdate>)>,
    vars: HashMap<ConfigKey, ConfigVar>,

    status: RcVar<ConfigStatus>,

    once_tasks: Vec<OnceConfigTask>,
    tasks: Vec<ConfigTask>,

    alts: Vec<std::rc::Weak<RefCell<Config>>>,
}
impl Config {
    fn new(update: AppEventSender) -> Self {
        Config {
            update,
            source: None,
            vars: HashMap::new(),

            status: var(ConfigStatus::default()),

            once_tasks: vec![],
            tasks: vec![],
            alts: vec![],
        }
    }

    fn update(&mut self, vars: &Vars) {
        // run once tasks
        for task in self.once_tasks.drain(..) {
            task(vars, &self.status);
        }

        // collect source updates
        let mut read = HashSet::new();
        let mut read_all = false;
        if let Some((_, source_tasks)) = &self.source {
            while let Ok(task) = source_tasks.try_recv() {
                match task {
                    ConfigSourceUpdate::Refresh(key) => {
                        if !read_all {
                            read.insert(key);
                        }
                    }
                    ConfigSourceUpdate::RefreshAll => read_all = true,
                    ConfigSourceUpdate::InternalError(e) => {
                        self.status.modify(vars, move |mut s| {
                            s.set_internal_error(e);
                        });
                    }
                }
            }
        }

        // run retained tasks
        self.tasks.retain_mut(|t| t(vars, &self.status));

        // Update config vars:
        // - Remove dropped vars.
        // - React to var assigns.
        // - Apply source requests.
        let mut var_tasks = vec![];
        self.vars.retain(|key, var| match var.upgrade(vars) {
            Some((any_var, write)) => {
                if write {
                    // var was set by the user, start a write task.
                    var_tasks.push(var.write(ConfigVarTaskArgs { vars, key, var: any_var }));
                } else if read_all || read.contains(key) {
                    // source notified a potential change, start a read task.
                    var_tasks.push(var.read(ConfigVarTaskArgs { vars, key, var: any_var }));
                }
                true // retain var
            }
            None => false, // var was dropped, remove entry
        });

        for task in var_tasks {
            task(self);
        }

        // update loaded alts.
        self.alts.retain(|alt| match alt.upgrade() {
            Some(alt) => {
                alt.borrow_mut().update(vars);
                true
            }
            None => false,
        })
    }

    /// Set the config source, replaces the previous source.
    pub fn load(&mut self, mut source: impl ConfigSource) {
        let (sender, receiver) = self.update.ext_channel();
        if !self.vars.is_empty() {
            let _ = sender.send(ConfigSourceUpdate::RefreshAll);
        }

        source.init(sender);
        self.source = Some((Box::new(source), receiver));
    }

    /// Open an alternative config source disconnected from the actual app source.
    #[must_use]
    pub fn load_alt(&mut self, source: impl ConfigSource) -> ConfigAlt {
        let e = ConfigAlt::load(self.update.clone(), source);
        self.alts.push(Rc::downgrade(&e.0));
        e
    }

    /// Gets a variable that tracks the source write tasks.
    pub fn status(&self) -> ReadOnlyRcVar<ConfigStatus> {
        self.status.clone().into_read_only()
    }

    /// Remove any errors set in the [`status`].
    ///
    /// [`status`]: Self::status
    pub fn clear_errors<Vw: WithVars>(&mut self, vars: &Vw) {
        vars.with_vars(|vars| {
            self.status.modify(vars, |mut s| {
                if s.has_errors() {
                    s.read_error = None;
                    s.write_error = None;
                    s.internal_error = None;
                }
            });
        })
    }

    /// Read the config value currently associated with the `key` if it is of the same type.
    ///
    /// Returns a [`ResponseVar`] that will update once when the value finishes reading.
    pub fn read<K, T>(&mut self, key: K) -> ResponseVar<Option<T>>
    where
        K: Into<ConfigKey>,
        T: ConfigValue,
    {
        self.read_impl(key.into())
    }
    fn read_impl<T>(&mut self, key: ConfigKey) -> ResponseVar<Option<T>>
    where
        T: ConfigValue,
    {
        // channel with the caller.
        let (responder, rsp) = response_var();

        self.read_raw(key, move |vars, r| {
            responder.respond(vars, r);
        });

        rsp
    }
    fn read_raw<T, R>(&mut self, key: ConfigKey, respond: R)
    where
        T: ConfigValue,
        R: FnOnce(&Vars, Option<T>) + 'static,
    {
        if let Some((source, _)) = &mut self.source {
            // channel with the source.
            let (sender, receiver) = self.update.ext_channel_bounded(1);
            source.read(key, sender);

            // bind two channels.
            let mut respond = Some(respond);
            self.tasks.push(Box::new(move |vars, status| {
                match receiver.try_recv() {
                    Ok(Ok(r)) => {
                        let respond = respond.take().unwrap();
                        respond(vars, r.and_then(|v| serde_json::from_value(v).ok()));
                        false
                    }
                    Err(None) => true, // retain
                    Ok(Err(e)) => {
                        status.modify(vars, move |mut s| {
                            s.set_read_error(e);
                        });

                        let respond = respond.take().unwrap();
                        respond(vars, None);
                        false
                    }
                    Err(Some(e)) => {
                        status.modify(vars, move |mut s| {
                            s.set_read_error(ConfigError::new(e));
                        });
                        let respond = respond.take().unwrap();
                        respond(vars, None);
                        false
                    }
                }
            }));
            let _ = self.update.send_ext_update();
        } else {
            // no source, just respond with `None`.
            self.once_tasks.push(Box::new(move |vars, _| {
                respond(vars, None);
            }));
            let _ = self.update.send_ext_update();
        }
    }

    /// Write the config value associated with the `key`.
    pub fn write<K, T>(&mut self, key: K, value: T)
    where
        K: Into<ConfigKey>,
        T: ConfigValue,
    {
        self.write_impl(key.into(), value)
    }
    fn write_impl<T>(&mut self, key: ConfigKey, value: T)
    where
        T: ConfigValue,
    {
        // register variable update if the entry is observed.
        let key = match self.vars.entry(key) {
            Entry::Occupied(entry) => {
                let key = entry.key().clone();
                if let Some(var) = entry.get().downcast::<T>() {
                    let value = value.clone();

                    self.once_tasks.push(Box::new(move |vars, _| {
                        var.modify(vars, move |mut v| {
                            if v.value != value {
                                v.value = value;
                                v.write.set(false);
                            }
                        });
                    }));

                    let _ = self.update.send_ext_update();
                } else {
                    // not observed anymore or changed type.
                    entry.remove();
                }
                key
            }
            Entry::Vacant(e) => e.into_key(),
        };

        // serialize and request write.
        self.write_source(key, value);
    }
    fn write_source<T>(&mut self, key: ConfigKey, value: T)
    where
        T: ConfigValue,
    {
        if let Some((source, _)) = &mut self.source {
            match serde_json::value::to_value(value) {
                Ok(json) => {
                    let (sx, rx) = self.update.ext_channel_bounded(1);
                    source.write(key, json, sx);
                    self.track_write_task(rx);
                }
                Err(e) => {
                    self.once_tasks.push(Box::new(move |vars, status| {
                        status.modify(vars, move |mut s| s.set_write_error(ConfigError::new(e)));
                    }));
                    let _ = self.update.send_ext_update();
                }
            }
        }
    }

    /// Remove the `key` from the persistent storage.
    ///
    /// Note that if a variable is connected with the `key` it stays connected with the same value, and if the variable
    /// is modified the `key` is reinserted. This should be called to remove obsolete configs only.
    pub fn remove<K: Into<ConfigKey>>(&mut self, key: K) {
        self.remove_impl(key.into())
    }
    fn remove_impl(&mut self, key: ConfigKey) {
        if let Some((source, _)) = &mut self.source {
            let (sx, rx) = self.update.ext_channel_bounded(1);
            source.remove(key, sx);
            self.track_write_task(rx);
        }
    }

    fn track_write_task(&mut self, rx: AppExtReceiver<Result<(), ConfigError>>) {
        let mut count = 0;
        self.tasks.push(Box::new(move |vars, status| {
            match rx.try_recv() {
                Ok(r) => {
                    status.modify(vars, move |mut s| {
                        s.pending -= count;
                        if let Err(e) = r {
                            s.set_write_error(e);
                        }
                    });
                    false // task finished
                }
                Err(None) => {
                    if count == 0 {
                        // first try, add pending.
                        count = 1;
                        status.modify(vars, |mut s| s.pending += 1);
                    }
                    true // retain
                }
                Err(Some(e)) => {
                    status.modify(vars, move |mut s| {
                        s.pending -= count;
                        s.set_write_error(ConfigError::new(e))
                    });
                    false // task finished
                }
            }
        }));
        let _ = self.update.send_ext_update();
    }

    /// Gets a variable that updates every time the config associated with `key` changes and writes the config
    /// every time it changes. This is equivalent of a two-way binding between the config storage and the variable.
    ///
    /// If the config is not already observed the `default_value` is used to generate a variable that will update with the current value
    /// after it is read.
    pub fn var<K, T, D>(&mut self, key: K, default_value: D) -> impl Var<T>
    where
        K: Into<ConfigKey>,
        T: ConfigValue,
        D: FnOnce() -> T,
    {
        self.var_with_source(key.into(), default_value).map_ref_bidi(
            |v| &v.value,
            |v| {
                v.write.set(true);
                &mut v.value
            },
        )
    }

    /// Binds a variable that updates every time the config associated with `key` changes and writes the config
    /// every time it changes. If the `target` is dropped the binding is dropped.
    ///
    /// If the config is not already observed the `default_value` is used to generate a variable that will update with the current value
    /// after it is read.
    pub fn bind<Vw: WithVars, K: Into<ConfigKey>, T: ConfigValue, D: FnOnce() -> T, V: Var<T>>(
        &mut self,
        vars: &Vw,
        key: K,
        default_value: D,
        target: &V,
    ) -> VarBindingHandle {
        let source = self.var_with_source(key.into(), default_value);
        vars.with_vars(|vars| {
            if let Some(target) = target.actual_var(vars).downgrade() {
                vars.bind(move |vars, binding| {
                    if let Some(target) = target.upgrade() {
                        if let Some(v) = source.get_new(vars) {
                            // source updated, notify
                            let _ = target.set_ne(vars, v.value.clone());
                        }
                        if let Some(value) = target.clone_new(vars) {
                            // user updated, write
                            source.modify(vars, move |mut v| {
                                if v.value != value {
                                    Cell::set(&v.write, true);
                                    v.value = value;
                                }
                            });
                        }
                    } else {
                        // dropped target, drop binding
                        binding.unbind();
                    }
                })
            } else {
                VarBindingHandle::dummy()
            }
        })
    }

    fn var_with_source<T: ConfigValue>(&mut self, key: ConfigKey, default_value: impl FnOnce() -> T) -> RcVar<ValueWithSource<T>> {
        let refresh;

        let r = match self.vars.entry(key) {
            Entry::Occupied(mut entry) => {
                if let Some(var) = entry.get().downcast::<T>() {
                    return var; // already observed and is the same type.
                }

                // entry stale or for the wrong type:

                // re-insert observer
                let (cfg_var, var) = ConfigVar::new(default_value());
                *entry.get_mut() = cfg_var;

                // and refresh the value.
                refresh = (entry.key().clone(), var.clone());

                var
            }
            Entry::Vacant(entry) => {
                let (cfg_var, var) = ConfigVar::new(default_value());

                refresh = (entry.key().clone(), var.clone());

                entry.insert(cfg_var);

                var
            }
        };

        let (key, var) = refresh;
        let value = self.read::<_, T>(key);
        self.tasks.push(Box::new(move |vars, _| {
            if let Some(rsp) = value.rsp_clone(vars) {
                if let Some(value) = rsp {
                    var.modify(vars, move |mut v| {
                        if v.value != value {
                            v.value = value;
                            v.write.set(false);
                        }
                    });
                }
                false // task finished
            } else {
                true // retain
            }
        }));

        r
    }
}

/// Represents a loaded config source that is not the main config.
///
/// This type allows interaction with a [`ConfigSource`] just like the [`Config`] service, but without affecting the
/// actual app config, so that the same config key can be loaded  from different sources with different values.
///
/// Note that some config sources can auto-reload if their backing file is modified, so modifications using this type
/// can end-up affecting the actual [`Config`] too.
///
/// You can use the [`Config::load_alt`] method to create an instance of this type.
pub struct ConfigAlt(Rc<RefCell<Config>>);
impl ConfigAlt {
    fn load(updates: AppEventSender, source: impl ConfigSource) -> Self {
        let mut cfg = Config::new(updates);
        cfg.load(source);
        ConfigAlt(Rc::new(RefCell::new(cfg)))
    }

    /// Flush writes and unload.
    pub fn unload(self) {
        // drop
    }

    /// Gets a variable that tracks the source write tasks.
    pub fn status(&self) -> ReadOnlyRcVar<ConfigStatus> {
        self.0.borrow().status()
    }

    /// Remove any errors set in the [`status`].
    ///
    /// [`status`]: Self::status
    pub fn clear_errors<Vw: WithVars>(&mut self, vars: &Vw) {
        self.0.borrow_mut().clear_errors(vars)
    }

    /// Read the config value currently associated with the `key` if it is of the same type.
    ///
    /// Returns a [`ResponseVar`] that will update once when the value finishes reading.
    pub fn read<K, T>(&mut self, key: K) -> ResponseVar<Option<T>>
    where
        K: Into<ConfigKey>,
        T: ConfigValue,
    {
        self.0.borrow_mut().read(key)
    }

    /// Write the config value associated with the `key`.
    pub fn write<K, T>(&mut self, key: K, value: T)
    where
        K: Into<ConfigKey>,
        T: ConfigValue,
    {
        self.0.borrow_mut().write(key, value)
    }

    /// Remove the `key` from the persistent storage.
    ///
    /// Note that if a variable is connected with the `key` it stays connected with the same value, and if the variable
    /// is modified the `key` is reinserted. This should be called to remove obsolete configs only.
    pub fn remove<K: Into<ConfigKey>>(&mut self, key: K) {
        self.0.borrow_mut().remove(key)
    }

    /// Gets a variable that updates every time the config associated with `key` changes and writes the config
    /// every time it changes. This is equivalent of a two-way binding between the config storage and the variable.
    ///
    /// If the config is not already observed the `default_value` is used to generate a variable that will update with the current value
    /// after it is read.
    pub fn var<K, T, D>(&mut self, key: K, default_value: D) -> impl Var<T>
    where
        K: Into<ConfigKey>,
        T: ConfigValue,
        D: FnOnce() -> T,
    {
        self.0.borrow_mut().var(key, default_value)
    }

    /// Binds a variable that updates every time the config associated with `key` changes and writes the config
    /// every time it changes. If the `target` is dropped the binding is dropped.
    ///
    /// If the config is not already observed the `default_value` is used to generate a variable that will update with the current value
    /// after it is read.
    pub fn bind<Vw: WithVars, K: Into<ConfigKey>, T: ConfigValue, D: FnOnce() -> T, V: Var<T>>(
        &mut self,
        vars: &Vw,
        key: K,
        default_value: D,
        target: &V,
    ) -> VarBindingHandle {
        Config::bind(&mut *self.0.borrow_mut(), vars, key, default_value, target)
    }
}
impl Drop for ConfigAlt {
    fn drop(&mut self) {
        if let Some((mut s, _)) = self.0.borrow_mut().source.take() {
            s.deinit();
        }
    }
}

type VarUpdateTask = Box<dyn FnOnce(&mut Config)>;

/// ConfigVar actual value, tracks if updates need to be send to source.
#[derive(Debug, Clone, PartialEq)]
struct ValueWithSource<T: ConfigValue> {
    value: T,
    write: Rc<Cell<bool>>,
}

struct ConfigVar {
    var: Box<dyn AnyWeakVar>,
    write: Rc<Cell<bool>>,
    run_task: Box<dyn Fn(ConfigVarTask, ConfigVarTaskArgs) -> VarUpdateTask>,
}
impl ConfigVar {
    fn new<T: ConfigValue>(initial_value: T) -> (Self, RcVar<ValueWithSource<T>>) {
        let write = Rc::new(Cell::new(false));
        let var = var(ValueWithSource {
            value: initial_value,
            write: write.clone(),
        });
        let r = ConfigVar {
            var: var.downgrade().into_any(),
            write,
            run_task: Box::new(ConfigVar::run_task_impl::<T>),
        };
        (r, var)
    }

    /// Returns var and if it needs to write.
    fn upgrade(&mut self, vars: &Vars) -> Option<(Box<dyn AnyVar>, bool)> {
        self.var.upgrade_any().map(|v| {
            let write = self.write.get() && v.is_new_any(vars);
            (v, write)
        })
    }

    fn downcast<T: ConfigValue>(&self) -> Option<RcVar<ValueWithSource<T>>> {
        self.var.as_any().downcast_ref::<types::WeakRcVar<ValueWithSource<T>>>()?.upgrade()
    }

    fn read(&self, args: ConfigVarTaskArgs) -> VarUpdateTask {
        (self.run_task)(ConfigVarTask::Read, args)
    }
    fn write(&self, args: ConfigVarTaskArgs) -> VarUpdateTask {
        (self.run_task)(ConfigVarTask::Write, args)
    }
    fn run_task_impl<T: ConfigValue>(task: ConfigVarTask, args: ConfigVarTaskArgs) -> VarUpdateTask {
        if let Some(var) = args.var.as_any().downcast_ref::<RcVar<ValueWithSource<T>>>() {
            match task {
                ConfigVarTask::Read => {
                    let key = args.key.clone();
                    let var = var.clone();
                    Box::new(move |config| {
                        config.read_raw::<T, _>(key, move |vars, value| {
                            if let Some(value) = value {
                                var.modify(vars, move |mut v| {
                                    if v.value != value {
                                        v.value = value;
                                        v.write.set(false);
                                    }
                                });
                            }
                        });
                    })
                }
                ConfigVarTask::Write => {
                    let key = args.key.clone();
                    let value = var.get_clone(args.vars).value;
                    Box::new(move |config| {
                        config.write_source(key, value);
                    })
                }
            }
        } else {
            Box::new(|_| {})
        }
    }
}

struct ConfigVarTaskArgs<'a> {
    vars: &'a Vars,
    key: &'a ConfigKey,
    var: Box<dyn AnyVar>,
}

enum ConfigVarTask {
    Read,
    Write,
}

/// Current [`Config`] status.
#[derive(Debug, Clone, Default)]
pub struct ConfigStatus {
    /// Number of pending writes.
    pub pending: usize,

    /// Last error during a read operation.
    pub read_error: Option<ConfigError>,
    /// Number of read errors.
    pub read_errors: u32,

    /// Last error during a write operation.
    pub write_error: Option<ConfigError>,
    /// Number of write errors.
    pub write_errors: u32,

    /// Last internal error.
    pub internal_error: Option<ConfigError>,
    /// Number of internal errors.
    pub internal_errors: u32,
}
impl ConfigStatus {
    /// Returns `true` if there are any errors currently in the status.
    ///
    /// The errors can be cleared using [`Config::clear_errors`].
    pub fn has_errors(&self) -> bool {
        self.read_error.is_some() || self.write_error.is_some() || self.internal_error.is_some()
    }

    fn set_read_error(&mut self, e: ConfigError) {
        self.read_error = Some(e);
        self.read_errors += 1;
    }

    fn set_write_error(&mut self, e: ConfigError) {
        self.write_error = Some(e);
        self.write_errors += 1;
    }

    fn set_internal_error(&mut self, e: ConfigError) {
        self.internal_error = Some(e);
        self.internal_errors += 1;
    }
}
impl fmt::Display for ConfigStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use std::cmp::Ordering::*;
        match self.pending.cmp(&1) {
            Equal => writeln!(f, "{} update pending…", self.pending)?,
            Greater => writeln!(f, "{} updates pending…", self.pending)?,
            Less => {}
        }

        if let Some(e) = &self.internal_error {
            write!(f, "internal error: ")?;
            fmt::Display::fmt(e, f)?;
            writeln!(f)?;
        }
        if let Some(e) = &self.read_error {
            write!(f, "read error: ")?;
            fmt::Display::fmt(e, f)?;
            writeln!(f)?;
        }
        if let Some(e) = &self.write_error {
            write!(f, "write error: ")?;
            fmt::Display::fmt(e, f)?;
            writeln!(f)?;
        }
        Ok(())
    }
}

/// Error in a [`ConfigSource`].
#[derive(Debug, Clone)]
pub struct ConfigError(pub Arc<dyn Error + Send + Sync>);
impl ConfigError {
    /// New error.
    pub fn new(error: impl Error + Send + Sync + 'static) -> Self {
        Self(Arc::new(error))
    }

    /// New error from string.
    pub fn new_str(error: impl Into<String>) -> Self {
        struct StringError(String);
        impl fmt::Debug for StringError {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Debug::fmt(&self.0, f)
            }
        }
        impl fmt::Display for StringError {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }
        impl Error for StringError {}
        Self::new(StringError(error.into()))
    }
}
impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.0)
    }

    fn cause(&self) -> Option<&dyn Error> {
        self.0.source()
    }
}
impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}
impl From<std::io::Error> for ConfigError {
    fn from(e: std::io::Error) -> Self {
        ConfigError::new(e)
    }
}
impl From<serde_json::Error> for ConfigError {
    fn from(e: serde_json::Error) -> Self {
        ConfigError::new(e)
    }
}

/// Represents an implementation of [`Config`].
pub trait ConfigSource: 'static {
    /// Called once when the source is installed.
    fn init(&mut self, observer: AppExtSender<ConfigSourceUpdate>);

    /// Called once when the app is shutdown.
    ///
    /// Sources should block and flush all pending writes here.
    fn deinit(&mut self);

    /// Send a read request for the most recent value associated with `key` in the persistent storage.
    ///
    /// The `rsp` channel must be used once to send back the result.
    fn read(&mut self, key: ConfigKey, rsp: AppExtSender<Result<Option<JsonValue>, ConfigError>>);
    /// Send a write request to set the `value` for `key` on the persistent storage.
    ///
    /// The `rsp` channel must be used once to send back the result.
    fn write(&mut self, key: ConfigKey, value: JsonValue, rsp: AppExtSender<Result<(), ConfigError>>);

    /// Send a request to remove the `key` from the persistent storage.
    fn remove(&mut self, key: ConfigKey, rsp: AppExtSender<Result<(), ConfigError>>);
}

/// External updates in a [`ConfigSource`].
#[derive(Clone, Debug)]
pub enum ConfigSourceUpdate {
    /// Value associated with the key may have changed from an external event, **not** a write operation.
    Refresh(ConfigKey),
    /// All values may have changed.
    RefreshAll,
    /// Error not directly related to a read or write operation.
    ///
    /// If a full refresh is required after this a `RefreshAll` is send.
    InternalError(ConfigError),
}

mod file_source {
    use super::*;
    use crate::{crate_util::panic_str, units::*};
    use std::{
        fs,
        io::{BufReader, BufWriter},
        path::PathBuf,
        thread::{self, JoinHandle},
        time::{Duration, Instant},
    };

    /// Simple [`ConfigSource`] that writes all settings to a JSON file.
    pub struct ConfigFile {
        file: PathBuf,
        thread: Option<(JoinHandle<()>, flume::Sender<Request>)>,
        update: Option<AppExtSender<ConfigSourceUpdate>>,
        pretty: bool,
        write_delay: Duration,
        last_panic: Option<Instant>,
        panic_count: usize,
        is_shutdown: bool,

        #[cfg(any(test, doc, feature = "test_util"))]
        read_delay: Option<Duration>,
    }
    impl ConfigFile {
        /// New with the path to the JSON config file.
        ///
        /// # Parameters
        ///
        /// * `json_file`: The configuration file, path and file are created if it does not exist.
        /// * `pretty`: If the JSON is formatted.
        /// * `write_delay`: Debounce delay, write requests made inside the time window all become a single write operation, all pending
        ///            writes are also written on shutdown.
        pub fn new(json_file: impl Into<PathBuf>, pretty: bool, write_delay: Duration) -> Self {
            ConfigFile {
                file: json_file.into(),
                thread: None,
                update: None,
                pretty,
                write_delay,
                last_panic: None,
                panic_count: 0,
                is_shutdown: false,

                #[cfg(any(test, doc, feature = "test_util"))]
                read_delay: None,
            }
        }

        /// Awaits the delay for each read request.
        #[cfg(any(test, doc, feature = "test_util"))]
        #[cfg_attr(doc_nightly, doc(cfg(feature = "test_util")))]
        pub fn with_read_delay(mut self, read_delay: Duration) -> Self {
            assert!(self.thread.is_none(), "worker already spawned");
            self.read_delay = Some(read_delay);
            self
        }

        fn send(&mut self, request: Request) {
            if self.is_shutdown {
                // worker thread is permanently shutdown, can happen in case of repeated panics, or
                match request {
                    Request::Read { rsp, .. } => {
                        let _ = rsp.send(Err(ConfigError::new_str("config worker is shutdown")));
                    }
                    Request::Write { rsp, .. } => {
                        let _ = rsp.send(Err(ConfigError::new_str("config worker is shutdown")));
                    }
                    Request::Remove { rsp, .. } => {
                        let _ = rsp.send(Err(ConfigError::new_str("config worker is shutdown")));
                    }
                    Request::Shutdown => {}
                }
            } else if let Some((_, sx)) = &self.thread {
                // worker thread is running, send request

                if sx.send(request).is_err() {
                    // worker thread disconnected, can only be due to panic.

                    // get panic.
                    let thread = self.thread.take().unwrap().0;
                    let panic = thread.join().unwrap_err();

                    // respawn 5 times inside 1 minute, in case the error is recoverable.
                    let now = Instant::now();
                    if let Some(last) = self.last_panic {
                        if now.duration_since(last) < 1.minutes() {
                            self.panic_count += 1;
                        } else {
                            self.panic_count = 1;
                        }
                    } else {
                        self.panic_count = 1;
                    }
                    self.last_panic = Some(now);

                    if self.panic_count > 5 {
                        self.is_shutdown = true;
                        let update = self.update.as_ref().unwrap();
                        update
                            .send(ConfigSourceUpdate::InternalError(ConfigError::new_str(format!(
                                "config thread panic 5 times in 1 minute, deactivating\nlast panic: {:?}",
                                panic_str(&panic)
                            ))))
                            .unwrap();
                    } else {
                        let update = self.update.as_ref().unwrap();
                        update
                            .send(ConfigSourceUpdate::InternalError(ConfigError::new_str(format!(
                                "config thread panic, {:?}",
                                panic_str(&panic)
                            ))))
                            .unwrap();
                        update.send(ConfigSourceUpdate::RefreshAll).unwrap();
                    }
                }
            } else {
                // spawn worker thread

                let (sx, rx) = flume::unbounded();
                sx.send(request).unwrap();
                let file = self.file.clone();
                let pretty = self.pretty;
                let write_delay = self.write_delay;
                #[cfg(any(test, doc, feature = "test_util"))]
                let read_delay = self.read_delay;

                let handle = thread::Builder::new()
                    .name("ConfigFile".to_owned())
                    .spawn(move || {
                        if let Some(dir) = file.parent() {
                            if let Err(e) = fs::create_dir_all(dir) {
                                if e.kind() != std::io::ErrorKind::AlreadyExists {
                                    panic!("failed to create missing config dir")
                                }
                            }
                        }

                        // load
                        let mut data: HashMap<Text, JsonValue> = {
                            let mut file = fs::OpenOptions::new()
                                .read(true)
                                .write(true)
                                .create(true)
                                .open(&file)
                                .expect("failed to crate or open config file");

                            if file.metadata().unwrap().len() == 0 {
                                HashMap::new()
                            } else {
                                serde_json::from_reader(&mut BufReader::new(&mut file)).unwrap()
                            }
                        };

                        let mut oldest_pending = Instant::now();
                        let mut pending_writes = vec![];
                        let mut write_fails = 0;
                        let mut run = true;

                        while run {
                            match rx.recv_timeout(if write_fails > 0 {
                                1.secs()
                            } else if pending_writes.is_empty() {
                                30.minutes()
                            } else {
                                write_delay
                            }) {
                                Ok(request) => match request {
                                    Request::Read { key, rsp } => {
                                        let r = data.get(&key).cloned();

                                        #[cfg(any(test, doc, feature = "test_util"))]
                                        if let Some(delay) = read_delay {
                                            crate::task::spawn(async move {
                                                crate::task::deadline(delay).await;
                                                rsp.send(Ok(r)).unwrap();
                                            });
                                        } else {
                                            rsp.send(Ok(r)).unwrap()
                                        }

                                        #[cfg(not(any(test, doc, feature = "test_util")))]
                                        rsp.send(Ok(r)).unwrap();
                                    }
                                    Request::Write { key, value, rsp } => {
                                        // update entry, but wait for next debounce write.
                                        let write = match data.entry(key) {
                                            Entry::Occupied(mut e) => {
                                                if e.get() != &value {
                                                    *e.get_mut() = value;
                                                    true
                                                } else {
                                                    false
                                                }
                                            }
                                            Entry::Vacant(e) => {
                                                e.insert(value);
                                                true
                                            }
                                        };
                                        if write {
                                            if pending_writes.is_empty() {
                                                oldest_pending = Instant::now();
                                            }
                                            pending_writes.push(rsp);
                                        } else {
                                            rsp.send(Ok(())).unwrap();
                                        }
                                    }
                                    Request::Remove { key, rsp } => {
                                        if data.remove(&key).is_some() {
                                            if pending_writes.is_empty() {
                                                oldest_pending = Instant::now();
                                            }
                                            pending_writes.push(rsp);
                                        } else {
                                            rsp.send(Ok(())).unwrap();
                                        }
                                    }
                                    Request::Shutdown => {
                                        // stop running will flush
                                        run = false;
                                    }
                                },
                                Err(flume::RecvTimeoutError::Timeout) => {}
                                Err(flume::RecvTimeoutError::Disconnected) => panic!("disconnected"),
                            }

                            if (!pending_writes.is_empty() || write_fails > 0) && (!run || (oldest_pending.elapsed()) >= write_delay) {
                                // debounce elapsed, or is shutting-down, or is trying to recover from write error.

                                // try write
                                let write_result: Result<(), ConfigError> = (|| {
                                    let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).open(&file)?;
                                    let file = BufWriter::new(&mut file);
                                    if pretty {
                                        serde_json::to_writer_pretty(file, &data)?;
                                    } else {
                                        serde_json::to_writer(file, &data)?;
                                    };

                                    Ok(())
                                })();

                                // notify write listeners
                                for request in pending_writes.drain(..) {
                                    let _ = request.send(write_result.clone());
                                }

                                // track error recovery
                                if write_result.is_err() {
                                    write_fails += 1;
                                    if write_fails > 5 {
                                        // causes a respawn or worker shutdown.
                                        panic!("write failed 5 times in 5 seconds");
                                    }
                                } else {
                                    write_fails = 0;
                                }
                            }
                        }
                    })
                    .expect("failed to spawn ConfigFile worker thread");

                self.thread = Some((handle, sx));
            }
        }
    }
    impl ConfigSource for ConfigFile {
        fn init(&mut self, sender: AppExtSender<ConfigSourceUpdate>) {
            self.update = Some(sender);
        }

        fn deinit(&mut self) {
            if let Some((thread, sender)) = self.thread.take() {
                self.is_shutdown = true;
                let _ = sender.send(Request::Shutdown);
                let _ = thread.join();
            }
        }

        fn read(&mut self, key: ConfigKey, rsp: AppExtSender<Result<Option<JsonValue>, ConfigError>>) {
            self.send(Request::Read { key, rsp })
        }

        fn write(&mut self, key: ConfigKey, value: JsonValue, rsp: AppExtSender<Result<(), ConfigError>>) {
            self.send(Request::Write { key, value, rsp })
        }

        fn remove(&mut self, key: ConfigKey, rsp: AppExtSender<Result<(), ConfigError>>) {
            self.send(Request::Remove { key, rsp })
        }
    }

    enum Request {
        Read {
            key: ConfigKey,
            rsp: AppExtSender<Result<Option<JsonValue>, ConfigError>>,
        },
        Write {
            key: ConfigKey,
            value: JsonValue,
            rsp: AppExtSender<Result<(), ConfigError>>,
        },
        Remove {
            key: ConfigKey,
            rsp: AppExtSender<Result<(), ConfigError>>,
        },
        Shutdown,
    }
}
pub use file_source::ConfigFile;
