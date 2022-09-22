use super::*;
use crate::{
    context::InfoContext,
    handler::WidgetHandler,
    impl_ui_node,
    widget_info::{WidgetInfoBuilder, WidgetSubscriptions},
    UiNode,
};

#[doc(hidden)]
#[macro_export]
macro_rules! __event_property {
    (
        $(#[$on_event_attrs:meta])*
        $vis:vis fn $event:ident {
            event: $EVENT:path,
            args: $Args:path,
            filter: $filter:expr,
        }
    ) => { $crate::paste! {
        $(#[$on_event_attrs])*
        ///
        /// # Preview
        ///
        #[doc = "You can preview this event using [`on_pre_"$event "`](fn.on_pre_"$event ".html)."]
        /// Otherwise the handler is only called after the widget content has a chance of handling the event by stopping propagation.
        ///
        /// # Async
        ///
        /// You can use async event handlers with this property.
        #[$crate::property(event, default( $crate::handler::hn!(|_, _|{}) ))]
        $vis fn [<on_ $event>](
            child: impl $crate::UiNode,
            handler: impl $crate::handler::WidgetHandler<$Args>,
        ) -> impl $crate::UiNode {
            $crate::event::on_event(child, $EVENT, $filter, handler)
        }

        #[doc = "Preview [`on_"$event "`](fn.on_"$event ".html) event."]
        ///
        /// # Preview
        ///
        /// Preview event properties call the handler before the main event property and before the widget content, if you stop
        /// the propagation of a preview event the main event handler is not called.
        ///
        /// # Async
        ///
        /// You can use async event handlers with this property, note that only the code before the fist `.await` is *preview*,
        /// subsequent code runs in widget updates.
        #[$crate::property(event, default( $crate::handler::hn!(|_, _|{}) ))]
        $vis fn [<on_pre_ $event>](
            child: impl $crate::UiNode,
            handler: impl $crate::handler::WidgetHandler<$Args>,
        ) -> impl $crate::UiNode {
            $crate::event::on_pre_event(child, $EVENT, $filter, handler)
        }
    } };

    (
        $(#[$on_event_attrs:meta])*
        $vis:vis fn $event:ident {
            event: $EVENT:path,
            args: $Args:path,
        }
    ) => {
        $crate::__event_property! {
            $(#[$on_event_attrs])*
            $vis fn $event {
                event: $EVENT,
                args: $Args,
                filter: |ctx, args| true,
            }
        }
    };
}
///<span data-del-macro-root></span> Declare one or more event properties.
///
/// Each declaration expands to two properties `on_$event`, `on_pre_$event`.
/// The preview properties call [`on_pre_event`], the main event properties call [`on_event`].
///
/// # Examples
///
/// ```
/// # fn main() { }
/// # use zero_ui_core::event::{event_property, EventArgs};
/// # use zero_ui_core::keyboard::*;
/// event_property! {
///     /// on_key_input docs.
///     pub fn key_input {
///         event: KEY_INPUT_EVENT,
///         args: KeyInputArgs,
///         // default filter is |ctx, args| true,
///     }
///
///     pub(crate) fn key_down {
///         event: KEY_INPUT_EVENT,
///         args: KeyInputArgs,
///         // optional filter:
///         filter: |ctx, args| args.state == KeyState::Pressed,
///     }
/// }
/// ```
///
/// # Filter
///
/// App events are delivered to all `UiNode` inside all widgets in the [`UpdateDeliveryList`] and event subscribers list,
/// event properties can specialize further by defining a filter predicate.
///
/// The `filter` predicate is called if [`propagation`] is not stopped. It must return `true` if the event arguments
/// are relevant in the context of the widget and event property. If it returns `true` the `handler` closure is called.
/// See [`on_event`] and [`on_pre_event`] for more information.
///
/// If you don't provide a filter predicate the default always allows, so all app events targeting the widget and not already handled
/// are allowed by default.  Note that events that represent an *interaction* with the widget are send for both [`ENABLED`] and [`DISABLED`]
/// targets, event properties should probably distinguish if they fire on normal interactions vs on *disabled* interactions.
///
/// # Async
///
/// Async event handlers are supported by properties generated by this macro, but only the code before the first `.await` executes
/// in the event track, subsequent code runs in widget updates.
///
/// [`on_pre_event`]: crate::event::on_pre_event
/// [`on_event`]: crate::event::on_event
/// [`propagation`]: EventArgs::propagation
/// [`ENABLED`]: crate::widget_info::Interactivity::ENABLED
/// [`DISABLED`]: crate::widget_info::Interactivity::DISABLED
#[macro_export]
macro_rules! event_property {
    ($(
        $(#[$on_event_attrs:meta])*
        $vis:vis fn $event:ident {
            event: $EVENT:path,
            args: $Args:path $(,
            filter: $filter:expr)? $(,)?
        }
    )+) => {$(
        $crate::__event_property! {
            $(#[$on_event_attrs])*
            $vis fn $event {
                event: $EVENT,
                args: $Args,
                $(filter: $filter,)?
            }
        }
    )+};
}
#[doc(inline)]
pub use crate::event_property;

/// Helper for declaring event properties.
///
/// This function is used by the [`event_property!`] macro.
///
/// # Filter
///
/// The `filter` predicate is called if [`propagation`] was not stopped. It must return `true` if the event arguments are
/// relevant in the context of the widget. If it returns `true` the `handler` closure is called. Note that events that represent
/// an *interaction* with the widget are send for both [`ENABLED`] and [`DISABLED`] targets, event properties should probably distinguish
/// if they fire on normal interactions vs on *disabled* interactions.
///
/// # Route
///
/// The event `handler` is called after the [`on_pre_event`] equivalent at the same context level. If the event
/// `filter` allows more then one widget and one widget contains the other, the `handler` is called on the inner widget first.
///
/// # Async
///
/// Async event handlers are called like normal, but code after the first `.await` only runs in subsequent updates. This means
/// that [`propagation`] must be stopped before the first `.await`, otherwise you are only signaling
/// other async tasks handling the same event, if they are monitoring the propagation handle.
///
/// [`propagation`]: EventArgs::propagation
/// [`ENABLED`]: crate::widget_info::Interactivity::ENABLED
/// [`DISABLED`]: crate::widget_info::Interactivity::DISABLED
pub fn on_event<C, A, F, H>(child: C, event: Event<A>, filter: F, handler: H) -> impl UiNode
where
    C: UiNode,
    A: EventArgs,
    F: FnMut(&mut WidgetContext, &A) -> bool + 'static,
    H: WidgetHandler<A>,
{
    struct OnEventNode<C, A: EventArgs, F, H> {
        child: C,
        event: Event<A>,
        filter: F,
        handler: H,
        handle: Option<EventWidgetHandle>,
    }
    #[impl_ui_node(child)]
    impl<C, A, F, H> UiNode for OnEventNode<C, A, F, H>
    where
        C: UiNode,
        A: EventArgs,
        F: FnMut(&mut WidgetContext, &A) -> bool + 'static,
        H: WidgetHandler<A>,
    {
        fn info(&self, ctx: &mut InfoContext, widget_info: &mut WidgetInfoBuilder) {
            self.child.info(ctx, widget_info);
        }

        fn init(&mut self, ctx: &mut WidgetContext) {
            self.handle = Some(self.event.subscribe(ctx.path.widget_id()));
            self.child.init(ctx);
        }

        fn deinit(&mut self, ctx: &mut WidgetContext) {
            self.handle = None;
            self.child.deinit(ctx);
        }

        fn subscriptions(&self, ctx: &mut InfoContext, subs: &mut WidgetSubscriptions) {
            subs.handler(&self.handler);
            self.child.subscriptions(ctx, subs);
        }

        fn event(&mut self, ctx: &mut WidgetContext, update: &mut EventUpdate) {
            self.child.event(ctx, update);
            if let Some(args) = self.event.on(update) {
                if !args.propagation().is_stopped() && (self.filter)(ctx, args) {
                    self.handler.event(ctx, args);
                }
            }
        }

        fn update(&mut self, ctx: &mut WidgetContext) {
            self.child.update(ctx);
            self.handler.update(ctx);
        }
    }

    #[cfg(dyn_closure)]
    let filter: Box<dyn FnMut(&mut WidgetContext, &A) -> bool> = Box::new(filter);

    OnEventNode {
        child: child.cfg_boxed(),
        event,
        filter,
        handler: handler.cfg_boxed(),
        handle: None,
    }
    .cfg_boxed()
}

/// Helper for declaring preview event properties.
///
/// This function is used by the [`event_property!`] macro.
///
/// # Filter
///
/// The `filter` predicate is called if [`propagation`] was not stopped. It must return `true` if the event arguments are
/// relevant in the context of the widget. If it returns `true` the `handler` closure is called. Note that events that represent
/// an *interaction* with the widget are send for both [`ENABLED`] and [`DISABLED`] targets, event properties should probably distinguish
/// if they fire on normal interactions vs on *disabled* interactions.
///
/// # Route
///
/// The event `handler` is called before the [`on_event`] equivalent at the same context level. If the event
/// `filter` allows more then one widget and one widget contains the other, the `handler` is called on the inner widget first.
///
/// # Async
///
/// Async event handlers are called like normal, but code after the first `.await` only runs in subsequent event updates. This means
/// that [`propagation`] must be stopped before the first `.await`, otherwise you are only signaling
/// other async tasks handling the same event, if they are monitoring the propagation handle.
///
/// [`propagation`]: EventArgs::propagation
/// [`ENABLED`]: crate::widget_info::Interactivity::ENABLED
/// [`DISABLED`]: crate::widget_info::Interactivity::DISABLED
pub fn on_pre_event<C, A, F, H>(child: C, event: Event<A>, filter: F, handler: H) -> impl UiNode
where
    C: UiNode,
    A: EventArgs,
    F: FnMut(&mut WidgetContext, &A) -> bool + 'static,
    H: WidgetHandler<A>,
{
    struct OnPreviewEventNode<C, A: EventArgs, F, H> {
        child: C,
        event: Event<A>,
        filter: F,
        handler: H,
        handle: Option<EventWidgetHandle>,
    }
    #[impl_ui_node(child)]
    impl<C, A, F, H> UiNode for OnPreviewEventNode<C, A, F, H>
    where
        C: UiNode,
        A: EventArgs,
        F: FnMut(&mut WidgetContext, &A) -> bool + 'static,
        H: WidgetHandler<A>,
    {
        fn info(&self, ctx: &mut InfoContext, widget_info: &mut WidgetInfoBuilder) {
            self.child.info(ctx, widget_info);
        }

        fn init(&mut self, ctx: &mut WidgetContext) {
            self.handle = Some(self.event.subscribe(ctx.path.widget_id()));
            self.child.init(ctx);
        }

        fn deinit(&mut self, ctx: &mut WidgetContext) {
            self.handle = None;
            self.child.deinit(ctx);
        }

        fn subscriptions(&self, ctx: &mut InfoContext, subs: &mut WidgetSubscriptions) {
            subs.handler(&self.handler);
            self.child.subscriptions(ctx, subs);
        }

        fn event(&mut self, ctx: &mut WidgetContext, update: &mut EventUpdate) {
            if let Some(args) = self.event.on(update) {
                if !args.propagation().is_stopped() && (self.filter)(ctx, args) {
                    self.handler.event(ctx, args);
                }
            }
            self.child.event(ctx, update);
        }

        fn update(&mut self, ctx: &mut WidgetContext) {
            self.handler.update(ctx);
            self.child.update(ctx);
        }
    }

    #[cfg(dyn_closure)]
    let filter: Box<dyn FnMut(&mut WidgetContext, &A) -> bool> = Box::new(filter);

    OnPreviewEventNode {
        child: child.cfg_boxed(),
        event,
        filter,
        handler: handler.cfg_boxed(),
        handle: None,
    }
    .cfg_boxed()
}
