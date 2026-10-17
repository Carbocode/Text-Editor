use adw::prelude::*;
use gtk::{gdk, glib};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[derive(Clone, Copy)]
enum Axis {
    Horizontal,
    Vertical,
}

/// Keep smooth scrolling in the native tab strip, without GtkScrolledWindow's
/// overshoot. Capture before AdwTabBox's wheel handler, which can otherwise
/// interpret a small vertical component of a touchpad gesture as a wheel step.
pub(super) struct TouchpadScroll {
    bar: glib::WeakRef<adw::TabBar>,
    axis: Cell<Option<Axis>>,
    pending: Cell<(f64, f64)>,
    animation: RefCell<Option<adw::TimedAnimation>>,
}

impl TouchpadScroll {
    pub(super) fn install(bar: &adw::TabBar, view: &adw::TabView) -> Rc<Self> {
        let state = Rc::new(Self {
            bar: bar.downgrade(),
            axis: Cell::new(None),
            pending: Cell::new((0.0, 0.0)),
            animation: RefCell::new(None),
        });
        let controller = gtk::EventControllerScroll::new(
            gtk::EventControllerScrollFlags::BOTH_AXES | gtk::EventControllerScrollFlags::KINETIC,
        );
        controller.set_propagation_phase(gtk::PropagationPhase::Capture);
        let scroll = state.clone();
        controller.connect_scroll_begin(move |_| scroll.begin());
        let scroll = state.clone();
        controller
            .connect_scroll(move |controller, dx, dy| scroll.scroll(controller.unit(), dx, dy));
        let scroll = state.clone();
        controller.connect_decelerate(move |controller, vx, vy| {
            if controller.unit() == gdk::ScrollUnit::Surface {
                scroll.decelerate(vx, vy);
            }
        });
        bar.add_controller(controller);
        let scroll = state.clone();
        view.connect_selected_page_notify(move |_| scroll.stop());
        let scroll = state.clone();
        view.connect_n_pages_notify(move |_| scroll.stop());
        state
    }

    pub(super) fn begin(&self) {
        self.stop();
        self.axis.set(None);
        self.pending.set((0.0, 0.0));
    }

    fn stop(&self) {
        if let Some(animation) = self.animation.borrow_mut().take() {
            animation.pause();
        }
    }

    pub(super) fn adjustment(&self) -> Option<gtk::Adjustment> {
        fn find(widget: &gtk::Widget) -> Option<gtk::Adjustment> {
            if let Some(scroll) = widget.downcast_ref::<gtk::ScrolledWindow>() {
                let adjustment = scroll.hadjustment();
                if adjustment.upper() - adjustment.page_size() > adjustment.lower() {
                    return Some(adjustment);
                }
            }
            let mut child = widget.first_child();
            while let Some(widget) = child {
                if let Some(adjustment) = find(&widget) {
                    return Some(adjustment);
                }
                child = widget.next_sibling();
            }
            None
        }
        find(self.bar.upgrade()?.upcast_ref())
    }

    fn delta(&self, dx: f64, dy: f64) -> f64 {
        let axis = self.axis.get().expect("gesture axis selected");
        match axis {
            Axis::Horizontal => dx,
            Axis::Vertical => {
                if self
                    .bar
                    .upgrade()
                    .is_some_and(|bar| bar.direction() == gtk::TextDirection::Rtl)
                {
                    -dy
                } else {
                    dy
                }
            }
        }
    }

    pub(super) fn scroll(&self, unit: gdk::ScrollUnit, dx: f64, dy: f64) -> glib::Propagation {
        // Horizontal wheel events must not reach GtkScrolledWindow either:
        // its wheel path also accumulates overshoot and springs back at edges.
        // Leave vertical wheel navigation to AdwTabBox.
        if unit != gdk::ScrollUnit::Surface && (unit != gdk::ScrollUnit::Wheel || dx == 0.0) {
            return glib::Propagation::Proceed;
        }
        let Some(adjustment) = self.adjustment() else {
            return glib::Propagation::Proceed;
        };
        self.stop();
        if unit == gdk::ScrollUnit::Wheel {
            let step = adjustment.page_size().powf(2.0 / 3.0);
            Self::move_by(&adjustment, dx * step);
            return glib::Propagation::Stop;
        }
        if dx == 0.0 && dy == 0.0 {
            return glib::Propagation::Stop;
        }
        let (dx, dy) = if self.axis.get().is_none() {
            let (x, y) = self.pending.get();
            let accumulated = (x + dx, y + dy);
            self.pending.set(accumulated);
            // Ignore initial finger jitter when deciding the gesture axis.
            if accumulated.0.abs().max(accumulated.1.abs()) < 3.0 {
                return glib::Propagation::Stop;
            }
            self.axis
                .set(Some(if accumulated.0.abs() >= accumulated.1.abs() {
                    Axis::Horizontal
                } else {
                    Axis::Vertical
                }));
            accumulated
        } else {
            (dx, dy)
        };
        // Surface deltas are logical pixels, not wheel detents. Keep them 1:1
        // and bound every update, including the event that crosses an edge.
        Self::move_by(&adjustment, self.delta(dx, dy));
        glib::Propagation::Stop
    }

    fn move_by(adjustment: &gtk::Adjustment, delta: f64) {
        let value = adjustment.value() + delta;
        let bounded = value.clamp(
            adjustment.lower(),
            adjustment.upper() - adjustment.page_size(),
        );
        if bounded == adjustment.value() {
            // Even a gesture blocked at the edge must interrupt any pending
            // native reveal-tab animation. set_value() elides this signal when
            // the value is unchanged; AdwTabBox listens to it to pause scrolling.
            adjustment.emit_by_name::<()>("value-changed", &[]);
        } else {
            adjustment.set_value(bounded);
        }
    }

    pub(super) fn decelerate(&self, vx: f64, vy: f64) {
        if self.axis.get().is_none() {
            return;
        }
        let Some(adjustment) = self.adjustment() else {
            return;
        };
        let Some(bar) = self.bar.upgrade() else {
            return;
        };
        let velocity = self.delta(vx, vy);
        if velocity.abs() < 0.01 {
            return;
        }
        self.stop();
        let from = adjustment.value();
        // EaseOutCubic starts at 3 * distance / duration. Shorten the animation
        // at an edge so it reaches the boundary without overshoot or rebound.
        let to = (from + velocity * 250.0 / 3.0).clamp(
            adjustment.lower(),
            adjustment.upper() - adjustment.page_size(),
        );
        if (to - from).abs() < 0.01 {
            return;
        }
        let duration = (3.0 * (to - from) / velocity).clamp(1.0, 250.0) as u32;
        let target = adw::CallbackAnimationTarget::new(move |value| {
            adjustment.set_value(value.clamp(
                adjustment.lower(),
                (adjustment.upper() - adjustment.page_size()).max(adjustment.lower()),
            ));
        });
        let animation = adw::TimedAnimation::new(&bar, from, to, duration, target);
        animation.set_easing(adw::Easing::EaseOutCubic);
        animation.play();
        self.animation.replace(Some(animation));
    }
}
