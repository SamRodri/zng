use super::*;

use std::{future::*, marker::PhantomData, pin::Pin, task::Poll};

/// See [`Var::wait_new`].
pub struct WaitNewFut<'a, T: VarValue, V: Var<T>> {
    is_new: WaitIsNewFut<'a, V>,
    _value: PhantomData<&'a T>,
}
impl<'a, T: VarValue, V: Var<T>> WaitNewFut<'a, T, V> {
    pub(super) fn new(var: &'a V) -> Self {
        Self {
            is_new: WaitIsNewFut::new(var),
            _value: PhantomData,
        }
    }
}
impl<'a, T: VarValue, V: Var<T>> Future for WaitNewFut<'a, T, V> {
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<T> {
        match self.is_new.poll_impl(cx) {
            Poll::Ready(()) => Poll::Ready(self.is_new.var.get()),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// See [`Var::wait_is_new`].
pub struct WaitIsNewFut<'a, V: AnyVar> {
    var: &'a V,
    update_id: VarUpdateId,
}
impl<'a, V: AnyVar> WaitIsNewFut<'a, V> {
    pub(super) fn new(var: &'a V) -> Self {
        Self {
            update_id: var.last_update(),
            var,
        }
    }

    fn poll_impl(&mut self, cx: &mut std::task::Context<'_>) -> Poll<()> {
        let update_id = self.var.last_update();
        if update_id != self.update_id {
            // has changed since init or last poll
            self.update_id = update_id;
            Poll::Ready(())
        } else {
            // has not changed since init or last poll, register hook
            let waker = cx.waker().clone();
            let handle = self.var.hook(Box::new(move |_| {
                waker.wake_by_ref();
                false
            }));

            // check if changed in parallel while was registering hook
            let update_id = self.var.last_update();
            if update_id != self.update_id {
                // changed in parallel
                // the hook will be dropped (handle not perm), it may wake in parallel too, but poll checks again.
                self.update_id = update_id;
                Poll::Ready(())
            } else {
                // really not ready yet
                handle.perm();
                Poll::Pending
            }
        }
    }
}
impl<'a, V: AnyVar> Future for WaitIsNewFut<'a, V> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        self.poll_impl(cx)
    }
}

/// See [`Var::wait_animation`].
pub struct WaitIsNotAnimatingFut<'a, V: AnyVar> {
    var: &'a V,
    observed_animation_start: bool,
}
impl<'a, V: AnyVar> WaitIsNotAnimatingFut<'a, V> {
    pub(super) fn new(var: &'a V) -> Self {
        Self {
            observed_animation_start: var.is_animating(),
            var,
        }
    }
}
impl<'a, V: AnyVar> Future for WaitIsNotAnimatingFut<'a, V> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<()> {
        if !self.var.capabilities().contains(VarCapabilities::NEW) {
            // var cannot have new value, ready to avoid deadlock.
            self.observed_animation_start = false;
            return Poll::Ready(());
        }
        if self.observed_animation_start {
            // already observed `is_animating` in a previous poll.

            if self.var.is_animating() {
                // still animating, but received poll so an animation was overridden and stopped.
                // try hook with new animation.

                while self.var.capabilities().contains(VarCapabilities::NEW) {
                    let waker = cx.waker().clone();
                    let r = self.var.hook_animation_stop(Box::new(move || {
                        waker.wake_by_ref();
                    }));
                    if r.is_err() {
                        // failed to hook with new animation too.
                        if self.var.is_animating() {
                            // but has yet another animation, try again.
                            continue;
                        } else {
                            // observed `is_animating` changing to `false`.
                            self.observed_animation_start = false;
                            return Poll::Ready(());
                        }
                    } else {
                        // new animation hook setup ok, break loop.
                        return Poll::Pending;
                    }
                }

                // var no longer has the `NEW` capability.
                self.observed_animation_start = false;
                Poll::Ready(())
            } else {
                // now observed change to `false`.
                self.observed_animation_start = false;
                Poll::Ready(())
            }
        } else {
            // have not observed `is_animating` yet.

            // hook with normal var updates, `is_animating && is_new` is always `true`.
            let waker = cx.waker().clone();
            let start_hook = self.var.hook(Box::new(move |_| {
                waker.wake_by_ref();
                false
            }));

            if self.var.is_animating() {
                // observed `is_animating` already, changed in other thread during the `hook` setup.
                self.observed_animation_start = true;

                while self.var.capabilities().contains(VarCapabilities::NEW) {
                    // hook with animation stop.
                    let waker = cx.waker().clone();
                    let r = self.var.hook_animation_stop(Box::new(move || {
                        waker.wake_by_ref();
                    }));
                    if r.is_err() {
                        // failed to hook, animation already stopped during hook setup.
                        if self.var.is_animating() {
                            // but var is still animating, reason a new animation replaced the previous one (that stopped).
                            // continue to hook with new animation.
                            continue;
                        } else {
                            // we have observed `is_animating` changing to `false` in one poll call.
                            self.observed_animation_start = false;
                            return Poll::Ready(());
                        }
                    } else {
                        // animation hook setup ok, break loop.
                        return Poll::Pending;
                    }
                }

                // var no longer has the `NEW` capability.
                self.observed_animation_start = false;
                Poll::Ready(())
            } else {
                // updates hook ok.
                start_hook.perm();
                Poll::Pending
            }
        }
    }
}
