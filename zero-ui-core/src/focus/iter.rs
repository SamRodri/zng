//! Focusable info tree iterators.
//!

use super::*;

use crate::widget_info::{iter as w_iter, WidgetInfo};

/// Filter-maps an iterator of [`WidgetInfo`] to [`WidgetFocusInfo`].
pub trait IterFocusableExt<'a, I: Iterator<Item = WidgetInfo<'a>>> {
    /// Returns an iterator of only the focusable widgets.
    ///
    /// See the [`Focus::focus_disabled_widgets`] config for more on the parameter.
    ///
    /// [`Focus::focus_disabled_widgets`]: crate::focus::Focus::focus_disabled_widgets
    fn focusable(self, focus_disabled_widgets: bool) -> IterFocusuable<'a, I>;
}
impl<'a, I> IterFocusableExt<'a, I> for I
where
    I: Iterator<Item = WidgetInfo<'a>>,
{
    fn focusable(self, focus_disabled_widgets: bool) -> IterFocusuable<'a, I> {
        IterFocusuable {
            iter: self,
            focus_disabled_widgets,
        }
    }
}

/// Filter a widget info iterator to only focusable items.
///
/// Use [`IterFocusableExt::focusable`] to create.
pub struct IterFocusuable<'a, I: Iterator<Item = WidgetInfo<'a>>> {
    iter: I,
    focus_disabled_widgets: bool,
}
impl<'a, I> Iterator for IterFocusuable<'a, I>
where
    I: Iterator<Item = WidgetInfo<'a>>,
{
    type Item = WidgetFocusInfo<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        for next in self.iter.by_ref() {
            if let Some(next) = next.as_focusable(self.focus_disabled_widgets) {
                return Some(next);
            }
        }
        None
    }
}
impl<'a, I> DoubleEndedIterator for IterFocusuable<'a, I>
where
    I: Iterator<Item = WidgetInfo<'a>> + DoubleEndedIterator,
{
    fn next_back(&mut self) -> Option<Self::Item> {
        while let Some(next) = self.iter.next_back() {
            if let Some(next) = next.as_focusable(self.focus_disabled_widgets) {
                return Some(next);
            }
        }
        None
    }
}

/// Iterator over all focusable items in a branch of the widget tree.
///
/// This `struct` is created by the [`descendants`] and [`self_and_descendants`] methods on [`WidgetFocusInfo`].
/// See its documentation for more.
///
/// [`descendants`]: WidgetFocusInfo::descendants
/// [`self_and_descendants`]: WidgetFocusInfo::self_and_descendants
pub struct FocusableDescendants<'a> {
    iter: w_iter::Descendants<'a>,
}
impl<'a> FocusableDescendants<'a> {
    pub(super) fn new(iter: w_iter::Descendants<'a>) -> Self {
        Self { iter }
    }

    /// Filter out entire branches of descendants at a time.
    pub fn filter<F>(self, mut filter: F) -> w_iter::FilterDescendants<'a, impl FnMut(WidgetInfo<'a>) -> w_iter::DescendantFilter>
    where
        F: FnMut(WidgetFocusInfo<'a>) -> w_iter::DescendantFilter,
    {
        self.iter.filter(move |w| {
            if let Some(f) = w.focusable() {
                filter(f)
            } else {
                w_iter::DescendantFilter::Skip
            }
        })
    }
}
impl<'a> Iterator for FocusableDescendants<'a> {
    type Item = WidgetFocusInfo<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        for next in self.iter.by_ref() {
            if let Some(next) = next.as_focusable(self.focus_disabled_widgets) {
                return Some(next);
            }
        }
        None
    }
}
