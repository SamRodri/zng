use super::units::{LayoutPoint, LayoutSize};
#[allow(unused)] // used in docs.
use super::UiNode;
use super::{
    context::{LayoutContext, LazyStateMap, WidgetContext},
    render::{FrameBuilder, FrameUpdate},
    ui_vec, UiVec, Widget, WidgetId,
};

/// A generic view over a list of [`Widget`] UI nodes.
///
/// Layout widgets should use this to abstract the children list type if possible.
pub trait UiList: 'static {
    /// Number of widgets in the list.
    fn len(&self) -> usize;

    /// If the list is empty.
    fn is_empty(&self) -> bool;

    /// Boxes all widgets and moved then to a [`UiVec`].
    fn box_all(self) -> UiVec;

    /// Creates a new list that consists of this list followed by the `other` list.
    fn chain<U>(self, other: U) -> UiListChain<Self, U>
    where
        Self: Sized,
        U: UiList,
    {
        UiListChain(self, other)
    }

    /// Gets the id of the widget at the `index`.
    ///
    /// The index is zero-based.
    fn widget_id(&self, index: usize) -> WidgetId;

    /// Reference the state of the widget at the `index`.
    fn widget_state(&self, index: usize) -> &LazyStateMap;

    /// Exclusive reference the state of the widget at the `index`.
    fn widget_state_mut(&mut self, index: usize) -> &mut LazyStateMap;

    /// Gets the last arranged size of the widget at the `index`.
    fn widget_size(&self, index: usize) -> LayoutSize;

    /// Calls [`UiNode::init`] in all widgets in the list, sequentially.
    fn init_all(&mut self, ctx: &mut WidgetContext);

    /// Calls [`UiNode::deinit`] in all widgets in the list, sequentially.
    fn deinit_all(&mut self, ctx: &mut WidgetContext);

    /// Calls [`UiNode::update`] in all widgets in the list, sequentially.
    fn update_all(&mut self, ctx: &mut WidgetContext);

    /// Calls [`UiNode::update_hp`] in all widgets in the list, sequentially.
    fn update_hp_all(&mut self, ctx: &mut WidgetContext);

    /// Calls [`UiNode::measure`] in all widgets in the list, sequentially.
    ///
    /// # `available_size`
    ///
    /// The `available_size` parameter is a function that takes a widget index and the `ctx` and returns
    /// the available size for the widget.
    ///
    /// The index is zero-based, `0` is the first widget, `len() - 1` is the last.
    ///
    /// # `desired_size`
    ///
    /// The `desired_size` parameter is a function is called with the widget index, the widget measured size and the `ctx`.
    ///
    /// This is how you get the widget desired size.
    fn measure_all<A, D>(&mut self, available_size: A, desired_size: D, ctx: &mut LayoutContext)
    where
        A: FnMut(usize, &mut LayoutContext) -> LayoutSize,
        D: FnMut(usize, LayoutSize, &mut LayoutContext);

    /// Calls [`UiNode::arrange`] in all widgets in the list, sequentially.
    ///
    /// # `final_size`
    ///
    /// The `final size` parameter is a function that takes a widget index and the `ctx` and returns the
    /// final size the widget must use.
    fn arrange_all<F>(&mut self, final_size: F, ctx: &mut LayoutContext)
    where
        F: FnMut(usize, &mut LayoutContext) -> LayoutSize;

    /// Calls [`UiNode::render`] in all widgets in the list, sequentially. Uses a reference frame
    /// to offset each widget.
    ///
    /// # `origin`
    ///
    /// The `origin` parameter is a function that takes a widget index and returns the offset that must
    /// be used to render it.
    fn render_all<O>(&self, origin: O, frame: &mut FrameBuilder)
    where
        O: FnMut(usize) -> LayoutPoint;

    /// Calls [`UiNode::render_update`] in all widgets in the list, sequentially.
    fn render_update_all(&self, update: &mut FrameUpdate);
}

/// Two [`UiList`] lists chained.
///
/// See [`UiList::chain`] for more information.
pub struct UiListChain<A: UiList, B: UiList>(A, B);

impl<A: UiList, B: UiList> UiList for UiListChain<A, B> {
    fn len(&self) -> usize {
        self.0.len() + self.1.len()
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty() && self.1.is_empty()
    }

    fn box_all(self) -> UiVec {
        let mut a = self.0.box_all();
        a.extend(self.1.box_all());
        a
    }

    fn init_all(&mut self, ctx: &mut WidgetContext) {
        self.0.init_all(ctx);
        self.1.init_all(ctx);
    }

    fn deinit_all(&mut self, ctx: &mut WidgetContext) {
        self.0.deinit_all(ctx);
        self.1.deinit_all(ctx);
    }

    fn update_all(&mut self, ctx: &mut WidgetContext) {
        self.0.update_all(ctx);
        self.1.update_all(ctx);
    }

    fn update_hp_all(&mut self, ctx: &mut WidgetContext) {
        self.0.update_hp_all(ctx);
        self.1.update_hp_all(ctx);
    }

    fn measure_all<AS, D>(&mut self, mut available_size: AS, mut desired_size: D, ctx: &mut LayoutContext)
    where
        AS: FnMut(usize, &mut LayoutContext) -> LayoutSize,
        D: FnMut(usize, LayoutSize, &mut LayoutContext),
    {
        self.0
            .measure_all(|i, c| available_size(i, c), |i, l, c| desired_size(i, l, c), ctx);
        let offset = self.0.len();
        self.1
            .measure_all(|i, c| available_size(i - offset, c), |i, l, c| desired_size(i - offset, l, c), ctx);
    }

    fn arrange_all<F>(&mut self, mut final_size: F, ctx: &mut LayoutContext)
    where
        F: FnMut(usize, &mut LayoutContext) -> LayoutSize,
    {
        self.0.arrange_all(|i, c| final_size(i, c), ctx);
        let offset = self.0.len();
        self.1.arrange_all(|i, c| final_size(i - offset, c), ctx);
    }

    fn render_all<O>(&self, mut origin: O, frame: &mut FrameBuilder)
    where
        O: FnMut(usize) -> LayoutPoint,
    {
        self.0.render_all(|i| origin(i), frame);
        let offset = self.0.len();
        self.1.render_all(|i| origin(i - offset), frame);
    }

    fn render_update_all(&self, update: &mut FrameUpdate) {
        self.0.render_update_all(update);
        self.1.render_update_all(update);
    }

    fn widget_id(&self, index: usize) -> WidgetId {
        let a_len = self.0.len();
        if index < a_len {
            self.0.widget_id(index)
        } else {
            self.1.widget_id(index - a_len)
        }
    }

    fn widget_state(&self, index: usize) -> &LazyStateMap {
        let a_len = self.0.len();
        if index < a_len {
            self.0.widget_state(index)
        } else {
            self.1.widget_state(index - a_len)
        }
    }

    fn widget_state_mut(&mut self, index: usize) -> &mut LazyStateMap {
        let a_len = self.0.len();
        if index < a_len {
            self.0.widget_state_mut(index)
        } else {
            self.1.widget_state_mut(index - a_len)
        }
    }

    fn widget_size(&self, index: usize) -> LayoutSize {
        let a_len = self.0.len();
        if index < a_len {
            self.0.widget_size(index)
        } else {
            self.1.widget_size(index - a_len)
        }
    }
}

macro_rules! impl_iter {
    () => {
        #[inline]
        fn init_all(&mut self, ctx: &mut WidgetContext) {
            for w in self {
                w.init(ctx);
            }
        }

        #[inline]
        fn deinit_all(&mut self, ctx: &mut WidgetContext) {
            for w in self {
                w.deinit(ctx);
            }
        }

        #[inline]
        fn update_all(&mut self, ctx: &mut WidgetContext) {
            for w in self {
                w.update(ctx);
            }
        }

        #[inline]
        fn update_hp_all(&mut self, ctx: &mut WidgetContext) {
            for w in self {
                w.update_hp(ctx);
            }
        }

        fn measure_all<A, D>(&mut self, mut available_size: A, mut desired_size: D, ctx: &mut LayoutContext)
        where
            A: FnMut(usize, &mut LayoutContext) -> LayoutSize,
            D: FnMut(usize, LayoutSize, &mut LayoutContext),
        {
            for (i, w) in self.iter_mut().enumerate() {
                let available_size = available_size(i, ctx);
                let r = w.measure(available_size, ctx);
                desired_size(i, r, ctx);
            }
        }

        fn arrange_all<F>(&mut self, mut final_size: F, ctx: &mut LayoutContext)
        where
            F: FnMut(usize, &mut LayoutContext) -> LayoutSize,
        {
            for (i, w) in self.iter_mut().enumerate() {
                let final_size = final_size(i, ctx);
                w.arrange(final_size, ctx);
            }
        }

        fn render_all<O>(&self, mut origin: O, frame: &mut FrameBuilder)
        where
            O: FnMut(usize) -> LayoutPoint,
        {
            for (i, w) in self.iter().enumerate() {
                let origin = origin(i);
                frame.push_reference_frame(origin, |frame| w.render(frame));
            }
        }

        #[inline]
        fn render_update_all(&self, update: &mut FrameUpdate) {
            for w in self {
                w.render_update(update);
            }
        }

        fn widget_id(&self, index: usize) -> WidgetId {
            self[index].id()
        }

        fn widget_state(&self, index: usize) -> &LazyStateMap {
            self[index].state()
        }

        fn widget_state_mut(&mut self, index: usize) -> &mut LazyStateMap {
            self[index].state_mut()
        }

        fn widget_size(&self, index: usize) -> LayoutSize {
            self[index].size()
        }
    };
}

impl UiList for UiVec {
    #[inline]
    fn len(&self) -> usize {
        self.len()
    }
    #[inline]
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
    #[inline]
    fn box_all(self) -> UiVec {
        self
    }

    impl_iter! {}
}

macro_rules! impl_arrays {
    ( $($N:tt),+ $(,)?) => {$(
        impl<W: Widget> UiList for [W; $N] {
            fn len(&self) -> usize {
                $N
            }

            fn is_empty(&self) -> bool {
                $N == 0
            }

            fn box_all(self) -> UiVec {
                arrayvec::ArrayVec::from(self).into_iter().map(|w| w.boxed_widget()).collect()
            }

            impl_iter! {}
        }
    )+};
}
impl_arrays! {
    0,
    1,
    2,
    3,
    4,
    5,
    6,
    7,
    8,
    9,
    10,
    11,
    12,
    13,
    14,
    15,
    16,
    17,
    18,
    19,
    20,
    21,
    22,
    23,
    24,
    25,
    26,
    27,
    28,
    29,
    30,
    31,
    32,
}

macro_rules! impl_tuples {
    ($($N:tt => $($WN:tt),+;)+) => {$(paste::paste! {

        impl_tuples! { $N => $($WN = [<W $WN>]),+ }

    })+};
    ($N:tt => $($WN:tt = $W:ident),+) => {
        impl<$($W: Widget),+> UiList for ($($W,)+) {
            #[inline]
            fn len(&self) -> usize {
                $N
            }

            #[inline]
            fn is_empty(&self) -> bool {
                false
            }

            #[inline]
            fn box_all(self) -> UiVec {
                ui_vec![$(self.$WN.boxed_widget()),+]
            }

            #[inline]
            fn init_all(&mut self, ctx: &mut WidgetContext) {
                $(self.$WN.init(ctx);)+
            }

            #[inline]
            fn deinit_all(&mut self, ctx: &mut WidgetContext) {
                $(self.$WN.deinit(ctx);)+
            }

            #[inline]
            fn update_all(&mut self, ctx: &mut WidgetContext) {
                $(self.$WN.update(ctx);)+
            }

            #[inline]
            fn update_hp_all(&mut self, ctx: &mut WidgetContext) {
                $(self.$WN.update_hp(ctx);)+
            }

            fn measure_all<A, D>(&mut self, mut available_size: A, mut desired_size: D, ctx: &mut LayoutContext)
            where
                A: FnMut(usize, &mut LayoutContext) -> LayoutSize,
                D: FnMut(usize, LayoutSize, &mut LayoutContext),
            {
                $(
                let av_sz = available_size($WN, ctx);
                let r = self.$WN.measure(av_sz, ctx);
                desired_size($WN, r, ctx);
                )+
            }

            fn arrange_all<F>(&mut self, mut final_size: F, ctx: &mut LayoutContext)
            where
                F: FnMut(usize, &mut LayoutContext) -> LayoutSize,
            {
                $(
                let fi_sz = final_size($WN, ctx);
                self.$WN.arrange(fi_sz, ctx);
                )+
            }

            fn render_all<O>(&self, mut origin: O, frame: &mut FrameBuilder)
            where
                O: FnMut(usize) -> LayoutPoint,
            {
                $(
                let o = origin($WN);
                frame.push_reference_frame(o, |frame| self.$WN.render(frame));
                )+
            }

            #[inline]
            fn render_update_all(&self, update: &mut FrameUpdate) {
                $(self.$WN.render_update(update);)+
            }

            fn widget_id(&self, index: usize) -> WidgetId {
                match index {
                    $($WN => self.$WN.id(),)+
                    _ => panic!("index {} out of range for length {}", index, self.len())
                }
            }

            fn widget_state(&self, index: usize) -> &LazyStateMap {
                match index {
                    $($WN => self.$WN.state(),)+
                    _ => panic!("index {} out of range for length {}", index, self.len())
                }
            }

            fn widget_state_mut(&mut self, index: usize) -> &mut LazyStateMap {
                match index {
                    $($WN => self.$WN.state_mut(),)+
                    _ => panic!("index {} out of range for length {}", index, self.len())
                }
            }

            fn widget_size(&self, index: usize) -> LayoutSize {
                match index {
                    $($WN => self.$WN.size(),)+
                    _ => panic!("index {} out of range for length {}", index, self.len())
                }
            }
        }
    };
}
impl_tuples! {
    1 => 0;
    2 => 0, 1;
    3 => 0, 1, 2;
    4 => 0, 1, 2, 3;
    5 => 0, 1, 2, 3, 4;
    6 => 0, 1, 2, 3, 4, 5;
    7 => 0, 1, 2, 3, 4, 5, 6;
    8 => 0, 1, 2, 3, 4, 5, 6, 7;

    9 => 0, 1, 2, 3, 4, 5, 6, 7, 8;
    10 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9;
    11 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10;
    12 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11;
    13 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12;
    14 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13;
    15 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14;
    16 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15;

    17 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16;
    18 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17;
    18 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18;
    20 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19;
    21 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20;
    22 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21;
    23 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22;
    24 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23;

    25 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24;
    26 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25;
    27 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26;
    28 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27;
    29 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28;
    30 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29;
    31 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30;
    32 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31;
}

impl UiList for () {
    #[inline]
    fn len(&self) -> usize {
        0
    }

    #[inline]
    fn is_empty(&self) -> bool {
        true
    }

    #[inline]
    fn box_all(self) -> UiVec {
        ui_vec![]
    }

    #[inline]
    fn init_all(&mut self, _: &mut WidgetContext) {}

    #[inline]
    fn deinit_all(&mut self, _: &mut WidgetContext) {}

    #[inline]
    fn update_all(&mut self, _: &mut WidgetContext) {}

    #[inline]
    fn update_hp_all(&mut self, _: &mut WidgetContext) {}

    fn measure_all<A, D>(&mut self, _: A, _: D, _: &mut LayoutContext)
    where
        A: FnMut(usize, &mut LayoutContext) -> LayoutSize,
        D: FnMut(usize, LayoutSize, &mut LayoutContext),
    {
    }

    fn arrange_all<F>(&mut self, _: F, _: &mut LayoutContext)
    where
        F: FnMut(usize, &mut LayoutContext) -> LayoutSize,
    {
    }

    fn render_all<O>(&self, _: O, _: &mut FrameBuilder)
    where
        O: FnMut(usize) -> LayoutPoint,
    {
    }

    #[inline]
    fn render_update_all(&self, _: &mut FrameUpdate) {}

    fn widget_id(&self, index: usize) -> WidgetId {
        panic!("index {} out of range for length 0", index)
    }

    fn widget_state(&self, index: usize) -> &LazyStateMap {
        panic!("index {} out of range for length 0", index)
    }

    fn widget_state_mut(&mut self, index: usize) -> &mut LazyStateMap {
        panic!("index {} out of range for length 0", index)
    }

    fn widget_size(&self, index: usize) -> LayoutSize {
        panic!("index {} out of range for length 0", index)
    }
}
