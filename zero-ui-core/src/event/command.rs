use std::{
    any::TypeId,
    cell::{Cell, RefCell},
    mem,
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::{
    context::*,
    crate_util::{FxHashMap, Handle, HandleOwner},
    gesture::CommandShortcutExt,
    handler::WidgetHandler,
    impl_ui_node,
    text::Text,
    var::{types::ReadOnlyVar, *},
    widget_info::{WidgetInfoBuilder, WidgetSubscriptions},
    window::WindowId,
    UiNode, WidgetId,
};

use super::*;

/// <span data-del-macro-root></span> Declares new [`Command`] keys.
///
/// The macro generates an [`event!`] of args type [`CommandArgs`] and added capability to track the presence of listeners enabled
/// and disabled and any other custom attached metadata.
///
/// # Conventions
///
/// Command events have the `_CMD` suffix, for example a command for the clipboard *copy* action is called `COPY_CMD`.
/// Public and user facing commands also set the [`CommandNameExt`] and [`CommandInfoExt`] with localized display text.
///
/// # Shortcuts
///
/// You can give commands one or more shortcuts using the [`CommandShortcutExt`], the [`GestureManager`] notifies commands
/// that match a pressed shortcut automatically.
///
/// # Examples
///
/// Declare two commands:
///
/// ```
/// use zero_ui_core::command::command;
///
/// command! {
///     static FOO_CMD;
///
///     /// Command docs.
///     pub(crate) static BAR_CMD;
/// }
/// ```
///
/// You can also initialize metadata:
///
/// ```
/// use zero_ui_core::{event::{command, CommandNameExt, CommandInfoExt}, gesture::{CommandShortcutExt, shortcut}};
///
/// command! {
///     /// Represents the **foo** action.
///     pub FOO_CMD = {
///         name: "Foo!",
///         info: "Does the foo! thing.",
///         shortcut: shortcut![CTRL+F],
///     };
/// }
/// ```
///
/// The initialization uses the [command extensions] pattern and runs once for each app, so usually just once.
///
/// Or you can use a custom closure to initialize the command:
///
/// ```
/// use zero_ui_core::{event::{command, CommandNameExt, CommandInfoExt}, gesture::{CommandShortcutExt, shortcut}};
///
/// command! {
///     /// Represents the **foo** action.
///     pub FOO_CMD => |cmd| {
///         cmd.init_name("Foo!");
///         cmd.init_info("Does the foo! thing.");
///         cmd.init_shortcut(shortcut![CTRL+F]);
///     };
/// }
/// ```
///
/// For the first kind of metadata initialization a documentation section is also generated with a table of metadata.
///
/// [`Command`]: crate::event::Command
/// [`CommandArgs`]: crate::event::CommandArgs
/// [`CommandNameExt`]: crate::event::CommandNameExt
/// [`CommandInfoExt`]: crate::event::CommandInfoExt
/// [`CommandShortcutExt`]: crate::gesture::CommandShortcutExt
/// [`GestureManager`]: crate::gesture::GestureManager
/// [`Event`]: crate::event::Event
/// [command extensions]: crate::event::Command#extensions
#[macro_export]
macro_rules! command {
    ($(
        $(#[$attr:meta])*
        $vis:vis static $COMMAND:ident $(=> $custom_meta_init:expr ;)? $(= { $($meta_ident:ident : $meta_init:expr),* $(,)? };)? $(;)?
    )+) => {
        $(
            $crate::__command! {
                $(#[$attr])*
                $vis static $COMMAND $(=> $custom_meta_init)? $(= {
                    $($meta_ident: $meta_init,)+
                })? ;
            }
        )+
    }
}
#[doc(inline)]
pub use command;

#[doc(hidden)]
#[macro_export]
macro_rules! __command {
    (
        $(#[$attr:meta])*
        $vis:vis static $COMMAND:ident => $meta_init:expr;
    ) => {
        $crate::paste! {
            std::thread_local! {
                #[doc(hidden)]
                static [<$COMMAND _LOCAL>]: $crate::event::EventData = $crate::event::EventData::new(std::stringify!($EVENT));
                #[doc(hidden)]
                static [<$COMMAND _DATA>]: $crate::event::CommandData = $crate::event::CommandData::new(std::boxed::Box::new($meta_init));
            }

            $(#[$attr])*
            $vis static $COMMAND: $crate::event::Command = $crate::event::Command::new(&[<$COMMAND _LOCAL>], &[<$COMMAND _DATA>]);
        }
    };
    (
        $(#[$attr:meta])*
        $vis:vis static $COMMAND:ident = { $($meta_ident:ident : $meta_init:expr),* $(,)? };
    ) => {
        $crate::paste! {
            $crate::__command! {
                $(#[$attr])*
                ///
                /// # Metadata
                ///
                /// This command initializes with the following metadata:
                ///
                /// | metadata | value |
                /// |----------|-------|
                $(#[doc = "|  `" $meta_ident "`  |  `"  "`  |"])+
                $vis static $COMMAND => |cmd| {
                    $(
                        cmd.[<init_ $meta_ident>]($meta_init);
                    )*
                };
            }
        }
    };
    (
        $(#[$attr:meta])*
        $vis:vis static $COMMAND:ident;
    ) => {
        $crate::__command! {
            $(#[$attr])*
            $vis static $COMMAND => $crate::event::__command_no_meta;
        }
    };
}
#[doc(hidden)]
pub fn __command_no_meta(_: Command) {}

/// Identifies a command event.
///
/// Use the [`command!`] to declare commands, it declares command keys with optional
/// [metadata](#metadata) initialization.
///
/// ```
/// # use zero_ui_core::command::*;
/// # pub trait CommandFooBarExt: Sized { fn init_foo(self, foo: bool) -> Self { self } fn init_bar(self, bar: bool) -> Self { self } }
/// # impl<C: Command> CommandFooBarExt for C { }
/// command! {
///     /// Foo-bar command.
///     pub FOO_BAR_CMD = {
///         foo: true,
///         bar: false,
///     };
/// }
/// ```
///
/// # Metadata
///
/// Commands can have metadata associated with then, this metadata is extendable and can be used to enable
/// command features such as command shortcuts. The metadata can be accessed using [`with_meta`], metadata
/// extensions are implemented using extension traits. See [`CommandMeta`] for more details.
///
/// # Handles
///
/// Unlike other events, commands only notify if it has at least one handler, handlers
/// must call [`new_handle`] to indicate that the command is relevant to the current app state and
/// [set its enabled] flag to indicate that the handler can fulfill command requests.
///
/// Properties that setup a handler for a command event should do this automatically and are usually
/// paired with a *can_foo* context property that sets the enabled flag. You can use [`on_command`]
/// to declare command handler properties.
///
/// # Scopes
///
/// Commands are *global* by default, meaning an enabled handle anywhere in the app enables it everywhere.
/// You can call [`scoped`] to declare *sub-commands* that are the same command event, but filtered to a scope, metadata
/// of scoped commands inherit from the app scope metadata, but setting it overrides only for the scope.
///
/// [`command!`]: macro@crate::event::command
/// [`new_handle`]: Command::new_handle
/// [set its enabled]: CommandHandle::set_enabled
/// [`with_meta`]: Command::with_meta
/// [`scoped`]: Command::scoped
#[derive(Clone, Copy)]
pub struct Command {
    event: Event<CommandArgs>,
    local: &'static LocalKey<CommandData>,
    scope: CommandScope,
}
impl fmt::Debug for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            f.debug_struct("Command")
                .field("event", &self.event)
                .field("scope", &self.scope)
                .finish_non_exhaustive()
        } else {
            write!(f, "{}", self.event.name())?;
            match self.scope {
                CommandScope::App => Ok(()),
                CommandScope::Window(id) => write!(f, "({id})"),
                CommandScope::Widget(id) => write!(f, "({id})"),
            }
        }
    }
}
impl Command {
    #[doc(hidden)]
    pub const fn new(event_local: &'static LocalKey<EventData>, command_local: &'static LocalKey<CommandData>) -> Self {
        Command {
            event: Event::new(event_local),
            local: command_local,
            scope: CommandScope::App,
        }
    }

    /// Create a new handle to this command.
    ///
    /// A handle indicates that there is an active *handler* for the event, the handle can also
    /// be used to set the [`is_enabled`](Self::is_enabled) state.
    pub fn new_handle<Evs: WithEvents>(&self, events: &mut Evs, enabled: bool) -> CommandHandle {
        events.with_events(|events| self.local.with(|l| l.new_handle(events, *self, enabled)))
    }

    /// Raw command event.
    pub fn event(&self) -> Event<CommandArgs> {
        self.event
    }

    /// Command operating scope.
    pub fn scope(&self) -> CommandScope {
        self.scope
    }

    /// Gets the command in a new `scope`.
    pub fn scoped(mut self, scope: impl Into<CommandScope>) -> Command {
        self.scope = scope.into();
        self
    }

    /// Visit the command custom metadata of the current scope.
    pub fn with_meta<R>(&self, visit: impl FnOnce(&mut CommandMeta) -> R) -> R {
        enum Meta<R, F> {
            Result(R),
            Init(Box<dyn Fn(Command)>, F),
        }
        let r = self.local.with(|l| {
            if l.meta_inited.replace(true) {
                let r = match self.scope {
                    CommandScope::App => visit(&mut CommandMeta {
                        meta: l.meta.borrow_mut().borrow_mut(),
                        scope: None,
                    }),
                    scope => {
                        let mut scopes = l.scopes.borrow_mut();
                        let scope = scopes.entry(scope).or_default();
                        visit(&mut CommandMeta {
                            meta: l.meta.borrow_mut().borrow_mut(),
                            scope: Some(scope.meta.borrow_mut()),
                        })
                    }
                };
                Meta::Result(r)
            } else {
                Meta::Init(l.meta_init.borrow_mut().take().unwrap(), visit)
            }
        });

        match r {
            Meta::Result(r) => r,
            Meta::Init(init, visit) => {
                init(*self);
                self.local.with(|l| *l.meta_init.borrow_mut() = Some(init));
                self.with_meta(visit)
            }
        }
    }

    /// Returns `true` if the update is for this command and scope.
    pub fn has(&self, update: &EventUpdate) -> bool {
        self.on(update).is_some()
    }

    /// Get the command update args if the update is for this command and scope.
    pub fn on<'a>(&self, update: &'a EventUpdate) -> Option<&'a CommandArgs> {
        self.event.on(update).filter(|a| a.scope == self.scope)
    }

    /// Get the event update args if the update is for this event and propagation is not stopped.
    pub fn on_unhandled<'a>(&self, update: &'a EventUpdate) -> Option<&'a CommandArgs> {
        self.event
            .on(update)
            .filter(|a| a.scope == self.scope && a.propagation().is_stopped())
    }

    /// Calls `handler` if the update is for this event and propagation is not stopped, after the handler is called propagation is stopped.
    pub fn handle<R>(&self, update: &EventUpdate, handler: impl FnOnce(&CommandArgs) -> R) -> Option<R> {
        if let Some(args) = self.on(update) {
            args.handle(handler)
        } else {
            None
        }
    }

    /// Gets a variable that tracks if this command has any live handlers.
    pub fn has_handlers(&self) -> ReadOnlyRcVar<bool> {
        self.local.with(|l| match self.scope {
            CommandScope::App => l.has_handlers.clone().into_read_only(),
            scope => l
                .scopes
                .borrow_mut()
                .entry(scope)
                .or_default()
                .has_handlers
                .clone()
                .into_read_only(),
        })
    }

    /// Gets a variable that tracks if this command has any enabled live handlers.
    pub fn is_enabled(&self) -> ReadOnlyRcVar<bool> {
        self.local.with(|l| match self.scope {
            CommandScope::App => l.is_enabled.clone().into_read_only(),
            scope => l.scopes.borrow_mut().entry(scope).or_default().is_enabled.clone().into_read_only(),
        })
    }

    #[cfg(test)]
    fn has_handlers_value(&self) -> bool {
        self.local.with(|l| match self.scope {
            CommandScope::App => !l.handle.is_dropped(),
            scope => l.scopes.borrow().get(&scope).map(|l| !l.handle.is_dropped()).unwrap_or(false),
        })
    }

    fn is_enabled_value(&self) -> bool {
        self.local.with(|l| match self.scope {
            CommandScope::App => !l.handle.is_dropped() && l.handle.data().enabled_count.load(Ordering::Relaxed) > 0,
            scope => l
                .scopes
                .borrow()
                .get(&scope)
                .map(|l| !l.handle.is_dropped() && l.handle.data().enabled_count.load(Ordering::Relaxed) > 0)
                .unwrap_or(false),
        })
    }

    /// Schedule a command update without param.
    pub fn notify<Ev: WithEvents>(&self, events: &mut Ev) {
        self.event
            .notify(events, CommandArgs::now(None, self.scope, self.is_enabled_value()))
    }

    /// Schedule a command update with custom `param`.
    pub fn notify_param<Ev: WithEvents>(&self, events: &mut Ev, param: impl Any) {
        self.event.notify(
            events,
            CommandArgs::now(CommandParam::new(param), self.scope, self.is_enabled_value()),
        );
    }

    /// Schedule a command update linked with an external event `propagation`.
    pub fn notify_linked<Ev: WithEvents>(&self, events: &mut Ev, propagation: EventPropagationHandle, param: Option<CommandParam>) {
        self.event.notify(
            events,
            CommandArgs::new(Instant::now(), propagation, param, self.scope, self.is_enabled_value()),
        )
    }

    pub(crate) fn update_state(&self, vars: &Vars) {
        self.local.with(|l| {
            if let CommandScope::App = self.scope {
                let has_handlers = !l.handle.is_dropped();
                l.has_handlers.set_ne(vars, has_handlers);
                l.is_enabled
                    .set_ne(vars, has_handlers && l.handle.data().enabled_count.load(Ordering::Relaxed) > 0);
            } else if let Some(scope) = l.scopes.borrow().get(&self.scope) {
                let has_handlers = !scope.handle.is_dropped();
                scope.has_handlers.set_ne(vars, has_handlers);
                scope
                    .is_enabled
                    .set_ne(vars, has_handlers && scope.handle.data().enabled_count.load(Ordering::Relaxed) > 0);
            }
        });
    }

    pub(crate) fn on_exit(&self) {
        self.local.with(|l| {
            l.registered.set(false);
            l.scopes.borrow_mut().clear();
            l.meta.borrow_mut().clear();
        });
    }
}
impl Deref for Command {
    type Target = Event<CommandArgs>;

    fn deref(&self) -> &Self::Target {
        &self.event
    }
}
impl PartialEq for Command {
    fn eq(&self, other: &Self) -> bool {
        self.event == other.event && self.scope == other.scope
    }
}
impl Eq for Command {}

/// Represents the scope of a [`Command`].
///
/// The command scope defines the targets of its event and the context of its metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandScope {
    /// Default scope, this is the scope of command types declared using [`command!`].
    App,
    /// Scope of a window.
    Window(WindowId),
    /// Scope of a widget.
    Widget(WidgetId),
}
impl From<WidgetId> for CommandScope {
    fn from(id: WidgetId) -> Self {
        CommandScope::Widget(id)
    }
}
impl From<WindowId> for CommandScope {
    fn from(id: WindowId) -> CommandScope {
        CommandScope::Window(id)
    }
}
impl<'a> From<&'a WidgetContext<'a>> for CommandScope {
    /// Widget scope from the `ctx.path.widget_id()`.
    fn from(ctx: &'a WidgetContext<'a>) -> Self {
        CommandScope::Widget(ctx.path.widget_id())
    }
}
impl<'a> From<&'a WindowContext<'a>> for CommandScope {
    /// Window scope from the `ctx.window_id`.
    fn from(ctx: &'a WindowContext<'a>) -> CommandScope {
        CommandScope::Window(*ctx.window_id)
    }
}
impl<'a> From<&'a WidgetContextMut> for CommandScope {
    /// Widget scope from the `ctx.widget_id()`.
    fn from(ctx: &'a WidgetContextMut) -> Self {
        CommandScope::Widget(ctx.widget_id())
    }
}

event_args! {
    /// Event args for command events.
    pub struct CommandArgs {
        /// Optional parameter for the command handler.
        pub param: Option<CommandParam>,

        /// Scope of command that notified.
        pub scope: CommandScope,

        /// If the command handle was enabled when the command notified.
        ///
        /// If `false` the command primary action must not run, but a secondary "disabled interaction"
        /// that indicates what conditions enable the command is recommended.
        pub enabled: bool,

        ..

        /// Broadcast to all widgets for [`CommandScope::App`].
        ///
        /// Broadcast to all widgets in the window for [`CommandScope::Window`].
        ///
        /// Target ancestors and widget for [`CommandScope::Widget`], if it is found.
        fn delivery_list(&self) -> EventDeliveryList {
            match self.scope {
                CommandScope::Widget(id) => EventDeliveryList::find_widget(id),
                CommandScope::Window(id) => EventDeliveryList::window(id),
                _ => EventDeliveryList::all(),
            }
        }
    }
}
impl CommandArgs {
    /// Returns a reference to a parameter of `T` if [`parameter`](#structfield.parameter) is set to a value of `T`.
    pub fn param<T: Any>(&self) -> Option<&T> {
        self.param.as_ref().and_then(|p| p.downcast_ref::<T>())
    }

    /// Returns [`param`] if is enabled interaction.
    ///
    /// [`param`]: Self::param()
    pub fn enabled_param<T: Any>(&self) -> Option<&T> {
        if self.enabled {
            self.param::<T>()
        } else {
            None
        }
    }

    /// Returns [`param`] if is disabled interaction.
    ///
    /// [`param`]: Self::param()
    pub fn disabled_param<T: Any>(&self) -> Option<&T> {
        if !self.enabled {
            self.param::<T>()
        } else {
            None
        }
    }

    /// Stops propagation and call `handler` if the command and local handler are enabled and was not handled.
    ///
    /// This is the default behavior of commands, when a command has a handler it is *relevant* in the context, and overwrites
    /// lower priority handlers, but if the handler is disabled the command primary action is not run.
    ///
    /// Returns the `handler` result if it was called.
    #[allow(unused)]
    pub fn handle_enabled<F, R>(&self, local_handle: &CommandHandle, handler: F) -> Option<R>
    where
        F: FnOnce(&Self) -> R,
    {
        let mut result = None;
        self.handle(|args| {
            if args.enabled && local_handle.is_enabled() {
                result = Some(handler(args));
            }
        });
        result
    }
}

/// A handle to a [`Command`].
///
/// Holding the command handle indicates that the command is relevant in the current app state.
/// The handle needs to be enabled to indicate that the command primary action can be executed.
///
/// You can use the [`Command::new_handle`] method in a command type to create a handle.
pub struct CommandHandle {
    handle: Handle<CommandHandleData>,
    local_enabled: Cell<bool>,
}
impl CommandHandle {
    /// Sets if the command event handler is active.
    ///
    /// When at least one [`CommandHandle`] is enabled the command is [`is_enabled`](Command::is_enabled).
    pub fn set_enabled(&self, enabled: bool) {
        if self.local_enabled.get() != enabled {
            UpdatesTrace::log_var::<bool>();

            self.local_enabled.set(enabled);
            let data = self.handle.data();

            if enabled {
                let check = data.enabled_count.fetch_add(1, Ordering::Relaxed);
                if check == usize::MAX {
                    data.enabled_count.store(usize::MAX, Ordering::Relaxed);
                    panic!("CommandHandle reached usize::MAX")
                }
            } else {
                data.enabled_count.fetch_sub(1, Ordering::Relaxed);
            };
        }
    }

    /// Returns if this handle has enabled the command.
    pub fn is_enabled(&self) -> bool {
        self.local_enabled.get()
    }

    /// Returns a dummy [`CommandHandle`] that is not connected to any command.
    pub fn dummy() -> Self {
        CommandHandle {
            handle: Handle::dummy(CommandHandleData::default()),
            local_enabled: Cell::new(false),
        }
    }
}
impl fmt::Debug for CommandHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandHandle")
            .field("handle", &self.handle)
            .field("local_enabled", &self.local_enabled)
            .finish()
    }
}
impl Drop for CommandHandle {
    fn drop(&mut self) {
        if self.local_enabled.get() {
            self.handle.data().enabled_count.fetch_sub(1, Ordering::Relaxed);
        }
    }
}
#[derive(Default)]
struct CommandHandleData {
    enabled_count: AtomicUsize,
}

/// Represents a reference counted `dyn Any` object.
#[derive(Clone)]
pub struct CommandParam(pub Rc<dyn Any>);
impl CommandParam {
    /// New param.
    pub fn new(param: impl Any + 'static) -> Self {
        CommandParam(Rc::new(param))
    }

    /// Gets the [`TypeId`] of the parameter.
    pub fn type_id(&self) -> TypeId {
        self.0.type_id()
    }

    /// Gets a typed reference to the parameter if it is of type `T`.
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.0.downcast_ref()
    }

    /// Returns `true` if the parameter type is `T`.
    pub fn is<T: Any>(&self) -> bool {
        self.0.is::<T>()
    }
}
impl fmt::Debug for CommandParam {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CommandParam").field(&self.0.type_id()).finish()
    }
}

unique_id_64! {
    /// Unique identifier of a command metadata state variable.
    ///
    /// This type is very similar to [`StateId`], but `T` is the value type of the metadata variable.
    pub struct CommandMetaVarId<T: (StateValue + VarValue)>: StateId;
}
impl<T: StateValue + VarValue> CommandMetaVarId<T> {
    fn app(self) -> StateId<RcVar<T>> {
        let id = self.get();
        // SAFETY:
        // id: We "inherit" from `StateId` so there is no repeated IDs.
        // type: only our private code can get this ID and we only use it in the app level state-map.
        unsafe { StateId::from_raw(id) }
    }

    fn scope(self) -> StateId<RcCowVar<T, RcVar<T>>> {
        let id = self.get();
        // SAFETY:
        // id: We "inherit" from `StateId` so there is no repeated IDs.
        // type: only our private code can get this ID and we only use it in the scope level state-map.
        unsafe { StateId::from_raw(id) }
    }
}

impl<T: StateValue + VarValue> fmt::Debug for CommandMetaVarId<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[cfg(debug_assertions)]
        let t = std::any::type_name::<T>();
        #[cfg(not(debug_assertions))]
        let t = "$T";

        if f.alternate() {
            writeln!(f, "CommandMetaVarId<{t} {{")?;
            writeln!(f, "   id: {},", self.get())?;
            writeln!(f, "   sequential: {}", self.sequential())?;
            writeln!(f, "}}")
        } else {
            write!(f, "CommandMetaVarId<{t}>({})", self.sequential())
        }
    }
}

/// Access to metadata of a command.
///
/// The metadata storage can be accessed using the [`Command::with_meta`]
/// method, implementers must declare and extension trait that adds methods that return [`CommandMetaVar`] or
/// [`ReadOnlyCommandMetaVar`] that are stored in the [`CommandMeta`]. An initialization builder method for
/// each value also must be provided to integrate with the [`command!`] macro.
///
/// # Examples
///
/// /// The [`command!`] initialization transforms `foo: true,` to `command.init_foo(true);`, to support that, the command extension trait
/// must has `foo` and `init_foo` methods.
///
/// ```
/// use zero_ui_core::{command::*, var::*};
///
/// static COMMAND_FOO_ID: StaticCommandMetaVarId<bool> = StaticCommandMetaVarId::new_unique();
/// static COMMAND_BAR_ID: StaticCommandMetaVarId<bool> = StaticCommandMetaVarId::new_unique();
///
/// /// FooBar command values.
/// pub trait CommandFooBarExt {
///     /// Gets read/write *foo*.
///     fn foo(self) -> CommandMetaVar<bool>;
///
///     /// Gets read-only *bar*.
///     fn bar(self) -> ReadOnlyCommandMetaVar<bool>;
///
///     /// Gets a read-only var derived from other metadata.
///     fn foo_and_bar(self) -> BoxedVar<bool>;
///
///     /// Init *foo*.
///     fn init_foo(self, foo: bool) -> Self;
///
///     /// Init *bar*.
///     fn init_bar(self, bar: bool) -> Self;
/// }
///
/// impl CommandFooBarExt for Command {
///     fn foo(self) -> CommandMetaVar<bool> {
///         self.with_meta(|m| m.get_var_or_default(&COMMAND_FOO_ID))
///     }
///
///     fn bar(self) -> ReadOnlyCommandMetaVar<bool> {
///         self.with_meta(|m| m.get_var_or_insert(&COMMAND_BAR_ID, ||true)).into_read_only()
///     }
///
///     fn foo_and_bar(self) -> BoxedVar<bool> {
///         merge_var!(self.foo(), self.bar(), |f, b| *f && *b).boxed()
///     }
///
///     fn init_foo(self, foo: bool) -> Self {
///         self.with_meta(|m| m.init_var(&COMMAND_FOO_ID, foo));
///         self
///     }
///
///     fn init_bar(self, bar: bool) -> Self {
///         self.with_meta(|m| m.init_var(&COMMAND_BAR_ID, bar));
///         self
///     }
/// }
/// ```
///
/// [`command!`]: macro@crate::event::command
pub struct CommandMeta<'a> {
    meta: StateMapMut<'a, CommandMetaState>,
    scope: Option<StateMapMut<'a, CommandMetaState>>,
}
impl<'a> CommandMeta<'a> {
    /// Clone a meta value identified by a [`StateId`].
    ///
    /// If the key is not set in the app, insert it using `init` to produce a value.
    pub fn get_or_insert<T, F>(&mut self, id: impl Into<StateId<T>>, init: F) -> T
    where
        T: StateValue + Clone,
        F: FnOnce() -> T,
    {
        let id = id.into();
        if let Some(scope) = &mut self.scope {
            if let Some(value) = scope.get(id) {
                value.clone()
            } else if let Some(value) = self.meta.get(id) {
                value.clone()
            } else {
                let value = init();
                let r = value.clone();
                scope.set(id, value);
                r
            }
        } else {
            self.meta.entry(id).or_insert_with(init).clone()
        }
    }

    /// Clone a meta value identified by a [`StateId`].
    ///
    /// If the key is not set, insert the default value and returns a clone of it.
    pub fn get_or_default<T>(&mut self, id: impl Into<StateId<T>>) -> T
    where
        T: StateValue + Clone + Default,
    {
        self.get_or_insert(id, Default::default)
    }

    /// Set the meta value associated with the [`StateId`].
    ///
    /// Returns the previous value if any was set.
    pub fn set<T>(&mut self, id: impl Into<StateId<T>>, value: impl Into<T>)
    where
        T: StateValue + Clone,
    {
        if let Some(scope) = &mut self.scope {
            scope.set(id, value);
        } else {
            self.meta.set(id, value);
        }
    }

    /// Set the metadata value only if it was not set.
    ///
    /// This does not set the scoped override, only the command type metadata.
    pub fn init<T>(&mut self, id: impl Into<StateId<T>>, value: impl Into<T>)
    where
        T: StateValue + Clone,
    {
        self.meta.entry(id).or_insert(value);
    }

    /// Clone a meta variable identified by a [`CommandMetaVarId`].
    ///
    /// The variable is read-write and is clone-on-write if the command is scoped,
    /// call [`into_read_only`] to make it read-only.
    ///
    /// [`into_read_only`]: Var::into_read_only
    pub fn get_var_or_insert<T, F>(&mut self, id: impl Into<CommandMetaVarId<T>>, init: F) -> CommandMetaVar<T>
    where
        T: StateValue + VarValue,
        F: FnOnce() -> T,
    {
        let id = id.into();
        if let Some(scope) = &mut self.scope {
            let meta = &mut self.meta;
            scope
                .entry(id.scope())
                .or_insert_with(|| {
                    let var = meta.entry(id.app()).or_insert_with(|| var(init())).clone();
                    CommandMetaVar::new(var)
                })
                .clone()
        } else {
            let var = self.meta.entry(id.app()).or_insert_with(|| var(init())).clone();
            CommandMetaVar::pass_through(var)
        }
    }

    /// Clone a meta variable identified by a [`CommandMetaVarId`].
    ///
    /// Inserts a variable with the default value if no variable is in the metadata.
    pub fn get_var_or_default<T>(&mut self, id: impl Into<CommandMetaVarId<T>>) -> CommandMetaVar<T>
    where
        T: StateValue + VarValue + Default,
    {
        self.get_var_or_insert(id, Default::default)
    }

    /// Set the metadata variable if it was not set.
    ///
    /// This does not set the scoped override, only the command type metadata.
    pub fn init_var<T>(&mut self, id: impl Into<CommandMetaVarId<T>>, value: impl Into<T>)
    where
        T: StateValue + VarValue,
    {
        self.meta.entry(id.into().app()).or_insert_with(|| var(value.into()));
    }
}

/// Read-write command metadata variable.
///
/// If you get this variable from a not scoped command, setting it sets
/// the value for all scopes. If you get this variable using a scoped command,
/// setting it overrides only the value for the scope.
///
/// The aliased type is an [`RcVar`] wrapped in a [`RcCowVar`], for not scoped commands the
/// [`RcCowVar::pass_through`] is used so that the wrapped [`RcVar`] is set directly on assign
/// but the variable type matches that from a scoped command.
pub type CommandMetaVar<T> = RcCowVar<T, RcVar<T>>;

/// Read-only command metadata variable.
///
/// To convert a [`CommandMetaVar<T>`] into this var call [`into_read_only`].
///
/// [`into_read_only`]: Var::into_read_only
pub type ReadOnlyCommandMetaVar<T> = ReadOnlyVar<T, CommandMetaVar<T>>;

/// Adds the [`name`](CommandNameExt) metadata.
pub trait CommandNameExt {
    /// Gets a read-write variable that is the display name for the command.
    fn name(self) -> CommandMetaVar<Text>;

    /// Sets the initial name if it is not set.
    fn init_name(self, name: impl Into<Text>) -> Self;

    /// Gets a read-only variable that formats the name and first shortcut in the following format: name (first_shortcut)
    /// Note: If no shortcuts are available this method returns the same as [`name`](Self::name)
    fn name_with_shortcut(self) -> BoxedVar<Text>
    where
        Self: crate::gesture::CommandShortcutExt;
}
static COMMAND_NAME_ID: StaticCommandMetaVarId<Text> = StaticCommandMetaVarId::new_unique();
impl CommandNameExt for Command {
    fn name(self) -> CommandMetaVar<Text> {
        self.with_meta(|m| {
            m.get_var_or_insert(&COMMAND_NAME_ID, || {
                let name = self.event.name();
                let name = name.strip_suffix("_CMD").unwrap_or(name);
                let mut title = String::with_capacity(name.len());
                let mut lower = false;
                for c in name.chars() {
                    if c == '_' {
                        if !title.ends_with(' ') {
                            title.push(' ');
                        }
                        lower = false;
                    } else if lower {
                        for l in c.to_lowercase() {
                            title.push(l);
                        }
                    } else {
                        title.push(c);
                        lower = true;
                    }
                }
                Text::from(title)
            })
        })
    }

    fn init_name(self, name: impl Into<Text>) -> Self {
        self.with_meta(|m| m.init_var(&COMMAND_NAME_ID, name.into()));
        self
    }

    fn name_with_shortcut(self) -> BoxedVar<Text>
    where
        Self: crate::gesture::CommandShortcutExt,
    {
        crate::merge_var!(self.name(), self.shortcut(), |name, shortcut| {
            if shortcut.is_empty() {
                name.clone()
            } else {
                crate::formatx!("{name} ({})", shortcut[0])
            }
        })
        .boxed()
    }
}

/// Adds the [`info`](CommandInfoExt) metadata.
pub trait CommandInfoExt {
    /// Gets a read-write variable that is a short informational string about the command.
    fn info(self) -> CommandMetaVar<Text>;

    /// Sets the initial info if it is not set.
    fn init_info(self, info: impl Into<Text>) -> Self;
}
static COMMAND_INFO_ID: StaticCommandMetaVarId<Text> = StaticCommandMetaVarId::new_unique();
impl CommandInfoExt for Command {
    fn info(self) -> CommandMetaVar<Text> {
        self.with_meta(|m| m.get_var_or_insert(&COMMAND_INFO_ID, Text::empty))
    }

    fn init_info(self, info: impl Into<Text>) -> Self {
        self.with_meta(|m| m.init_var(&COMMAND_INFO_ID, info.into()));
        self
    }
}

enum CommandMetaState {}

#[doc(hidden)]
pub struct CommandData {
    meta_init: RefCell<Option<Box<dyn Fn(Command)>>>,
    meta_inited: Cell<bool>,
    meta: RefCell<OwnedStateMap<CommandMetaState>>,

    handle: HandleOwner<CommandHandleData>,
    registered: Cell<bool>,

    has_handlers: RcVar<bool>,
    is_enabled: RcVar<bool>,

    scopes: RefCell<FxHashMap<CommandScope, ScopedValue>>,
}
impl CommandData {
    pub fn new(meta_init: Box<dyn Fn(Command)>) -> Self {
        CommandData {
            meta_init: RefCell::new(Some(meta_init)),
            meta_inited: Cell::new(false),
            meta: RefCell::default(),

            handle: HandleOwner::dropped(CommandHandleData::default()),
            registered: Cell::new(false),

            has_handlers: var(false),
            is_enabled: var(false),

            scopes: RefCell::default(),
        }
    }

    fn new_handle(&self, events: &mut Events, command: Command, enabled: bool) -> CommandHandle {
        let handle = match command.scope {
            CommandScope::App => {
                if !self.registered.replace(true) {
                    events.register_command(command);
                }

                self.handle.reanimate()
            }
            scope => {
                let mut scopes = self.scopes.borrow_mut();
                let scope = scopes.entry(scope).or_default();

                if !mem::replace(&mut scope.registered, true) {
                    events.register_command(command);
                }

                scope.handle.reanimate()
            }
        };

        let handle = CommandHandle {
            handle,
            local_enabled: Cell::new(false),
        };

        if enabled {
            handle.set_enabled(true);
        }

        handle
    }
}

struct ScopedValue {
    handle: HandleOwner<CommandHandleData>,
    is_enabled: RcVar<bool>,
    has_handlers: RcVar<bool>,
    meta: OwnedStateMap<CommandMetaState>,
    registered: bool,
}
impl Default for ScopedValue {
    fn default() -> Self {
        ScopedValue {
            is_enabled: var(false),
            has_handlers: var(false),
            handle: HandleOwner::dropped(CommandHandleData::default()),
            meta: OwnedStateMap::default(),
            registered: false,
        }
    }
}

/// Helper for declaring command handlers.
pub fn on_command<U, CB, E, EB, H>(child: U, command_builder: CB, enabled_builder: EB, handler: H) -> impl UiNode
where
    U: UiNode,
    CB: FnMut(&mut WidgetContext) -> Command + 'static,
    E: Var<bool>,
    EB: FnMut(&mut WidgetContext) -> E + 'static,
    H: WidgetHandler<CommandArgs>,
{
    struct OnCommandNode<U, CB, E, EB, H> {
        child: U,
        command: Option<Command>,
        command_builder: CB,
        enabled: Option<E>,
        enabled_builder: EB,
        handler: H,
        handle: Option<CommandHandle>,
    }
    #[impl_ui_node(child)]
    impl<U, CB, E, EB, H> UiNode for OnCommandNode<U, CB, E, EB, H>
    where
        U: UiNode,
        CB: FnMut(&mut WidgetContext) -> Command + 'static,
        E: Var<bool>,
        EB: FnMut(&mut WidgetContext) -> E + 'static,
        H: WidgetHandler<CommandArgs>,
    {
        fn info(&self, ctx: &mut InfoContext, widget_builder: &mut WidgetInfoBuilder) {
            self.child.info(ctx, widget_builder);
        }

        fn subscriptions(&self, ctx: &mut InfoContext, subs: &mut WidgetSubscriptions) {
            subs.event(&self.command.expect("OnCommandNode not initialized").event())
                .var(ctx, self.enabled.as_ref().unwrap())
                .handler(&self.handler);

            self.child.subscriptions(ctx, subs);
        }

        fn init(&mut self, ctx: &mut WidgetContext) {
            self.child.init(ctx);

            let enabled = (self.enabled_builder)(ctx);
            let is_enabled = enabled.copy(ctx);
            self.enabled = Some(enabled);

            let command = (self.command_builder)(ctx);
            self.command = Some(command);

            self.handle = Some(command.new_handle(ctx, is_enabled));
        }

        fn event(&mut self, ctx: &mut WidgetContext, update: &EventUpdate) {
            self.child.event(ctx, update);
            if let Some(args) = self.command.expect("OnCommandNode not initialized").on_unhandled(update) {
                self.handler.event(ctx, args);
            }
        }

        fn update(&mut self, ctx: &mut WidgetContext) {
            self.child.update(ctx);

            self.handler.update(ctx);

            if let Some(enabled) = self.enabled.as_ref().expect("OnCommandNode not initialized").copy_new(ctx) {
                self.handle.as_ref().unwrap().set_enabled(enabled);
            }
        }

        fn deinit(&mut self, ctx: &mut WidgetContext) {
            self.child.deinit(ctx);
            self.handle = None;
            self.command = None;
            self.enabled = None;
        }
    }

    OnCommandNode {
        child: child.cfg_boxed(),
        command: None,
        command_builder,
        enabled: None,
        enabled_builder,
        handler,
        handle: None,
    }
    .cfg_boxed()
}

/// Helper for declaring command preview handlers.
pub fn on_pre_command<U, CB, E, EB, H>(child: U, command_builder: CB, enabled_builder: EB, handler: H) -> impl UiNode
where
    U: UiNode,
    CB: FnMut(&mut WidgetContext) -> Command + 'static,
    E: Var<bool>,
    EB: FnMut(&mut WidgetContext) -> E + 'static,
    H: WidgetHandler<CommandArgs>,
{
    struct OnPreCommandNode<U, CB, E, EB, H> {
        child: U,
        command: Option<Command>,
        command_builder: CB,
        enabled: Option<E>,
        enabled_builder: EB,
        handler: H,
        handle: Option<CommandHandle>,
    }
    #[impl_ui_node(child)]
    impl<U, CB, E, EB, H> UiNode for OnPreCommandNode<U, CB, E, EB, H>
    where
        U: UiNode,
        CB: FnMut(&mut WidgetContext) -> Command + 'static,
        E: Var<bool>,
        EB: FnMut(&mut WidgetContext) -> E + 'static,
        H: WidgetHandler<CommandArgs>,
    {
        fn init(&mut self, ctx: &mut WidgetContext) {
            self.child.init(ctx);

            let enabled = (self.enabled_builder)(ctx);
            let is_enabled = enabled.copy(ctx);
            self.enabled = Some(enabled);

            let command = (self.command_builder)(ctx);
            self.command = Some(command);

            self.handle = Some(command.new_handle(ctx, is_enabled));
        }

        fn info(&self, ctx: &mut InfoContext, widget_builder: &mut WidgetInfoBuilder) {
            self.child.info(ctx, widget_builder);
        }

        fn subscriptions(&self, ctx: &mut InfoContext, subs: &mut WidgetSubscriptions) {
            subs.event(&self.command.expect("OnPreCommandNode not initialized").event())
                .var(ctx, self.enabled.as_ref().unwrap())
                .handler(&self.handler);

            self.child.subscriptions(ctx, subs);
        }

        fn event(&mut self, ctx: &mut WidgetContext, update: &EventUpdate) {
            if let Some(args) = self.command.expect("OnPreCommandNode not initialized").on_unhandled(update) {
                self.handler.event(ctx, args);
            }
            self.child.event(ctx, update);
        }

        fn update(&mut self, ctx: &mut WidgetContext) {
            self.handler.update(ctx);

            if let Some(enabled) = self.enabled.as_ref().expect("OnPreCommandNode not initialized").copy_new(ctx) {
                self.handle.as_ref().unwrap().set_enabled(enabled);
            }

            self.child.update(ctx);
        }

        fn deinit(&mut self, ctx: &mut WidgetContext) {
            self.child.deinit(ctx);
            self.handle = None;
            self.command = None;
            self.enabled = None;
        }
    }
    OnPreCommandNode {
        child: child.cfg_boxed(),
        command: None,
        command_builder,
        enabled: None,
        enabled_builder,
        handler,
        handle: None,
    }
    .cfg_boxed()
}

#[cfg(test)]
mod tests {
    use crate::context::TestWidgetContext;

    use super::*;

    command! {
        static FOO_CMD;
    }

    #[test]
    fn parameter_none() {
        let _ = CommandArgs::now(None, CommandScope::App, true);
    }

    #[test]
    fn enabled() {
        let mut ctx = TestWidgetContext::new();
        assert!(!FOO_CMD.has_handlers_value());

        let handle = FOO_CMD.new_handle(&mut ctx, true);
        assert!(FOO_CMD.is_enabled_value());

        handle.set_enabled(false);
        assert!(FOO_CMD.has_handlers_value());
        assert!(!FOO_CMD.is_enabled_value());

        handle.set_enabled(true);
        assert!(FOO_CMD.is_enabled_value());

        drop(handle);
        assert!(!FOO_CMD.has_handlers_value());
    }

    #[test]
    fn enabled_scoped() {
        let mut ctx = TestWidgetContext::new();

        let cmd = FOO_CMD;
        let cmd_scoped = FOO_CMD.scoped(ctx.window_id);
        assert!(!cmd.has_handlers_value());
        assert!(!cmd_scoped.has_handlers_value());

        let handle_scoped = cmd_scoped.new_handle(&mut ctx, true);
        assert!(!cmd.has_handlers_value());
        assert!(cmd_scoped.is_enabled_value());

        handle_scoped.set_enabled(false);
        assert!(!cmd.has_handlers_value());
        assert!(!cmd_scoped.is_enabled_value());
        assert!(cmd_scoped.has_handlers_value());

        handle_scoped.set_enabled(true);
        assert!(!cmd.has_handlers_value());
        assert!(cmd_scoped.is_enabled_value());

        drop(handle_scoped);
        assert!(!cmd.has_handlers_value());
        assert!(!cmd_scoped.has_handlers_value());
    }

    #[test]
    fn has_handlers() {
        let mut ctx = TestWidgetContext::new();
        assert!(!FOO_CMD.has_handlers_value());

        let handle = FOO_CMD.new_handle(&mut ctx, false);
        assert!(FOO_CMD.has_handlers_value());

        drop(handle);
        assert!(!FOO_CMD.has_handlers_value());
    }

    #[test]
    fn has_handlers_scoped() {
        let mut ctx = TestWidgetContext::new();

        let cmd = FOO_CMD;
        let cmd_scoped = FOO_CMD.scoped(ctx.window_id);

        assert!(!cmd.has_handlers_value());
        assert!(!cmd_scoped.has_handlers_value());

        let handle = cmd_scoped.new_handle(&mut ctx, false);

        assert!(!cmd.has_handlers_value());
        assert!(cmd_scoped.has_handlers_value());

        drop(handle);

        assert!(!cmd.has_handlers_value());
        assert!(!cmd_scoped.has_handlers_value());
    }

    // there are also integration tests in tests/command.rs
}
