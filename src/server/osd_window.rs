use std::rc::Rc;
use std::time::{Duration, Instant};
use std::{cell::RefCell, ops::Deref};

use gtk::{
	gdk,
	glib::{self, clone, ControlFlow},
	prelude::*,
};
use pulsectl::controllers::types::DeviceInfo;

use crate::widgets::segmented_progress_widget::SegmentedProgressWidget;
use crate::{
	brightness_backend::BrightnessBackend,
	utils::{
		get_duration, get_max_volume, get_show_percentage, get_slide, get_slide_duration,
		get_slide_fps, get_slide_hide_duration, get_slide_offscreen_padding, get_top_margin,
		volume_to_f64, KeysLocks, VolumeDeviceType,
	},
};

use gtk_layer_shell::LayerShell;

const ICON_SIZE: i32 = 32;

/// A window that our application can open that contains the main project view.
#[derive(Clone, Debug)]
pub struct SwayosdWindow {
	pub window: gtk::ApplicationWindow,
	pub monitor: gdk::Monitor,
	container: gtk::Box,
	timeout_id: Rc<RefCell<Option<glib::SourceId>>>,
	animation_id: Rc<RefCell<Option<glib::SourceId>>>,
}

// TODO: Use custom widget
// - Use start, center, and end children
//   - Always center the centered widget (both left and right sides are the same width)
impl SwayosdWindow {
	/// Create a new window and assign it to the given application.
	pub fn new(app: &gtk::Application, monitor: &gdk::Monitor) -> Self {
		let window = gtk::ApplicationWindow::new(app);
		window.set_widget_name("osd");
		window.add_css_class("osd");

		window.init_layer_shell();
		window.set_monitor(Some(monitor));
		window.set_namespace(Some("swayosd"));

		window.set_exclusive_zone(-1);
		window.set_layer(gtk_layer_shell::Layer::Overlay);
		// Anchor to bottom edge for better reliability with rotated/transformed displays
		window.set_anchor(gtk_layer_shell::Edge::Bottom, true);

		// Set up the widgets
		window.set_width_request(250);

		let container = cascade! {
			gtk::Box::new(gtk::Orientation::Horizontal, 12);
			..set_widget_name("container");
		};

		window.set_child(Some(&container));

		// Disable mouse input
		window.connect_map(|window| {
			if let Some(surface) = window.surface() {
				let region = gtk::cairo::Region::create();
				surface.set_input_region(&region);
			}
		});

		let update_margins = |window: &gtk::ApplicationWindow, monitor: &gdk::Monitor| {
			window.set_margin(
				gtk_layer_shell::Edge::Bottom,
				Self::target_bottom_margin(window, monitor),
			);
		};

		// Set the window margin
		update_margins(&window, monitor);
		// Ensure window margin is updated when necessary
		window.connect_scale_factor_notify(clone!(
			#[weak]
			monitor,
			move |window| update_margins(window, &monitor)
		));
		monitor.connect_scale_factor_notify(clone!(
			#[weak]
			window,
			move |monitor| update_margins(&window, monitor)
		));
		monitor.connect_geometry_notify(clone!(
			#[weak]
			window,
			move |monitor| update_margins(&window, monitor)
		));

		Self {
			window,
			container,
			monitor: monitor.clone(),
			timeout_id: Rc::new(RefCell::new(None)),
			animation_id: Rc::new(RefCell::new(None)),
		}
	}

	pub fn close(&self) {
		self.remove_animation();
		self.window.close();
	}

	pub fn changed_volume(&self, device: &DeviceInfo, device_type: &VolumeDeviceType) {
		self.clear_osd();

		let volume = volume_to_f64(&device.volume.avg());
		let icon_prefix = match device_type {
			VolumeDeviceType::Sink(_) => "sink",
			VolumeDeviceType::Source(_) => "source",
		};
		let icon_state = &match (device.mute, volume) {
			(true, _) => "muted",
			(_, 0.0) => "muted",
			(false, x) if x > 0.0 && x <= 33.0 => "low",
			(false, x) if x > 33.0 && x <= 66.0 => "medium",
			(false, x) if x > 66.0 && x <= 100.0 => "high",
			(false, x) if x > 100.0 => match device_type {
				VolumeDeviceType::Sink(_) => "high",
				VolumeDeviceType::Source(_) => "overamplified",
			},
			(_, _) => "high",
		};
		let icon_name = &format!("{}-volume-{}-symbolic", icon_prefix, icon_state);

		let max_volume: f64 = get_max_volume().into();

		let icon = self.build_icon_widget(icon_name);
		let progress = self.build_progress_widget(volume / max_volume);
		let label = self.build_text_widget(Some(&format!("{}%", volume)), Some(4));

		progress.set_sensitive(!device.mute);

		self.container.append(&icon);
		self.container.append(&progress);
		if get_show_percentage() {
			self.container.append(&label);
		}

		self.run_timeout();
	}

	pub fn changed_brightness(&self, brightness_backend: &mut dyn BrightnessBackend) {
		self.clear_osd();

		let icon_name = "display-brightness-symbolic";
		let icon = self.build_icon_widget(icon_name);

		let brightness = brightness_backend.get_current() as f64;
		let max = brightness_backend.get_max() as f64;
		let progress = self.build_progress_widget(brightness / max);
		let label = self.build_text_widget(
			Some(&format!("{}%", (brightness / max * 100.).round() as i32)),
			Some(4),
		);

		self.container.append(&icon);
		self.container.append(&progress);
		if get_show_percentage() {
			self.container.append(&label);
		}

		self.run_timeout();
	}

	pub fn changed_player(&self, icon: &str, label: Option<&str>) {
		self.clear_osd();

		let icon = self.build_icon_widget(icon);
		let label = self.build_text_widget(label, None);
		label.set_hexpand(true);

		self.container.append(&icon);
		self.container.append(&label);

		self.run_timeout();
	}

	pub fn changed_kbd_backlight(&self, value: u32, max: u32) {
		self.clear_osd();

		let value = value.min(max);

		let icon_name = match value {
			0 => "keyboard-brightness-off-symbolic",
			v if (v == max) => "keyboard-brightness-high-symbolic",
			_ => "keyboard-brightness-medium-symbolic",
		};
		let icon = self.build_icon_widget(icon_name);
		self.container.append(&icon);

		// A segmented progress bar looks cramped when there are too many segments
		if max < 5 {
			let progress = self.build_segmented_progress_widget(value, max);
			self.container.append(&progress);
		} else {
			let progress = self.build_progress_widget((value / max) as f64);
			self.container.append(&progress);
		}

		self.run_timeout();
	}

	pub fn changed_keylock(&self, key: KeysLocks, state: bool) {
		self.clear_osd();

		let label = self.build_text_widget(None, None);
		label.set_hexpand(true);

		let on_off_text = match state {
			true => "On",
			false => "Off",
		};

		let (label_text, symbol) = match key {
			KeysLocks::CapsLock => {
				let symbol = "caps-lock-symbolic";
				let text = "Caps Lock ".to_string() + on_off_text;
				(text, symbol)
			}
			KeysLocks::NumLock => {
				let symbol = "num-lock-symbolic";
				let text = "Num Lock ".to_string() + on_off_text;
				(text, symbol)
			}
			KeysLocks::ScrollLock => {
				let symbol = "scroll-lock-symbolic";
				let text = "Scroll Lock ".to_string() + on_off_text;
				(text, symbol)
			}
		};

		label.set_text(&label_text);
		let icon = self.build_icon_widget(symbol);

		icon.set_sensitive(state);

		self.container.append(&icon);
		self.container.append(&label);

		self.run_timeout();
	}

	pub fn custom_progress(&self, fraction: f64, text: Option<String>, icon_name: Option<&str>) {
		self.clear_osd();

		if let Some(icon_name) = icon_name {
			let icon = self.build_icon_widget(icon_name);
			self.container.append(&icon);
		}

		let progress = self.build_progress_widget(fraction.clamp(0.0, 1.0));
		self.container.append(&progress);

		if let Some(text) = text {
			let label = self.build_text_widget(Some(text.deref()), None);
			self.container.append(&label);
		}

		self.run_timeout();
	}

	pub fn custom_segmented_progress(
		&self,
		value: u32,
		n_segments: u32,
		text: Option<String>,
		icon_name: Option<&str>,
	) {
		self.clear_osd();

		if let Some(icon_name) = icon_name {
			let icon = self.build_icon_widget(icon_name);
			self.container.append(&icon);
		}

		let value = value.min(n_segments);
		let progress = self.build_segmented_progress_widget(value, n_segments);
		self.container.append(&progress);

		if let Some(text) = text {
			let label = self.build_text_widget(Some(text.deref()), None);
			self.container.append(&label);
		}

		self.run_timeout();
	}

	pub fn custom_message(&self, message: &str, icon_name: Option<&str>) {
		self.clear_osd();

		let label = self.build_text_widget(Some(message), None);
		label.set_hexpand(true);

		if let Some(icon_name) = icon_name {
			let icon = self.build_icon_widget(icon_name);
			self.container.append(&icon);
			self.container.append(&label);
			let box_spacing = self.container.spacing();
			icon.connect_realize(move |icon| {
				label.set_margin_end(
					icon.allocation().width()
						+ icon.margin_start()
						+ icon.margin_end()
						+ box_spacing,
				);
			});
		} else {
			self.container.append(&label);
		}

		self.run_timeout();
	}

	/// Clear all container children
	fn clear_osd(&self) {
		let mut next = self.container.first_child();
		while let Some(widget) = next {
			next = widget.next_sibling();
			self.container.remove(&widget);
		}
	}

	fn run_timeout(&self) {
		let was_visible = self.window.is_visible();

		// Hide window after timeout
		if let Some(timeout_id) = self.timeout_id.take() {
			timeout_id.remove()
		}
		let s = self.clone();
		self.timeout_id.replace(Some(glib::timeout_add_local_once(
			Duration::from_millis(get_duration()),
			move || {
				s.timeout_id.replace(None);
				s.slide_out();
			},
		)));

		if was_visible {
			self.show_in_place();
		} else {
			self.slide_in();
		}
	}

	fn slide_in(&self) {
		let target_margin = self.current_target_bottom_margin();
		if !get_slide() || get_slide_duration() == 0 {
			self.show_at_margin(target_margin);
			return;
		}

		let hidden_margin = self.hidden_bottom_margin();
		self.window
			.set_margin(gtk_layer_shell::Edge::Bottom, hidden_margin);
		self.window.show();
		self.animate_bottom_margin(
			hidden_margin,
			get_slide_duration(),
			false,
			self.target_margin_getter(),
		);
	}

	fn slide_out(&self) {
		let start_margin = self.current_bottom_margin();
		if !get_slide() || get_slide_hide_duration() == 0 {
			self.remove_animation();
			self.window.hide();
			return;
		}

		self.animate_bottom_margin(
			start_margin,
			get_slide_hide_duration(),
			true,
			self.hidden_margin_getter(),
		);
	}

	fn show_in_place(&self) {
		self.show_at_margin(self.current_target_bottom_margin());
	}

	fn show_at_margin(&self, margin: i32) {
		self.remove_animation();
		self.window
			.set_margin(gtk_layer_shell::Edge::Bottom, margin);
		self.window.show();
	}

	fn animate_bottom_margin<F>(&self, from: i32, duration_ms: u64, hide_after: bool, to_margin: F)
	where
		F: Fn() -> i32 + 'static,
	{
		self.remove_animation();

		let to = to_margin();
		if from == to {
			self.window.set_margin(gtk_layer_shell::Edge::Bottom, to);
			if hide_after {
				self.window.hide();
			}
			return;
		}

		let window = self.window.clone();
		let animation_id = self.animation_id.clone();
		let started_at = Instant::now();
		let interval = Duration::from_millis((1000 / get_slide_fps()).max(1));
		let duration = Duration::from_millis(duration_ms);

		let source_id = glib::timeout_add_local(interval, move || {
			let progress = (started_at.elapsed().as_secs_f64() / duration.as_secs_f64()).min(1.0);
			let eased_progress = 1.0 - (1.0 - progress).powi(3);
			let to = to_margin();
			let margin = from as f64 + (to - from) as f64 * eased_progress;
			window.set_margin(gtk_layer_shell::Edge::Bottom, margin.round() as i32);

			if progress >= 1.0 {
				window.set_margin(gtk_layer_shell::Edge::Bottom, to_margin());
				if hide_after {
					window.hide();
				}
				animation_id.replace(None);
				ControlFlow::Break
			} else {
				ControlFlow::Continue
			}
		});

		self.animation_id.replace(Some(source_id));
	}

	fn remove_animation(&self) {
		if let Some(animation_id) = self.animation_id.take() {
			animation_id.remove();
		}
	}

	fn current_bottom_margin(&self) -> i32 {
		self.window.margin(gtk_layer_shell::Edge::Bottom)
	}

	fn current_target_bottom_margin(&self) -> i32 {
		Self::target_bottom_margin(&self.window, &self.monitor)
	}

	fn hidden_bottom_margin(&self) -> i32 {
		-(self.window_height().max(1) + get_slide_offscreen_padding())
	}

	fn window_height(&self) -> i32 {
		Self::measured_window_height(&self.window, &self.container)
	}

	fn target_margin_getter(&self) -> impl Fn() -> i32 + 'static {
		let window = self.window.clone();
		let monitor = self.monitor.clone();
		move || Self::target_bottom_margin(&window, &monitor)
	}

	fn hidden_margin_getter(&self) -> impl Fn() -> i32 + 'static {
		let window = self.window.clone();
		let container = self.container.clone();
		move || {
			-(Self::measured_window_height(&window, &container).max(1)
				+ get_slide_offscreen_padding())
		}
	}

	fn measured_window_height(window: &gtk::ApplicationWindow, container: &gtk::Box) -> i32 {
		let allocated_height = window.allocated_height().max(container.allocated_height());
		if allocated_height > 0 {
			return allocated_height;
		}

		let (_minimum, natural, _minimum_baseline, _natural_baseline) =
			container.measure(gtk::Orientation::Vertical, -1);
		natural
	}

	fn target_bottom_margin(window: &gtk::ApplicationWindow, monitor: &gdk::Monitor) -> i32 {
		// Monitor scale factor is not always correct, so transform monitor height
		// into the coordinate system of the window before calculating the offset.
		let mon_height = monitor.geometry().height() / window.scale_factor();
		(mon_height as f32 * (1.0 - get_top_margin())).round() as i32
	}

	fn build_icon_widget(&self, icon_name: &str) -> gtk::Image {
		let icon = gtk::gio::ThemedIcon::from_names(&[icon_name, "missing-symbolic"]);

		cascade! {
			gtk::Image::from_gicon(&icon.upcast::<gtk::gio::Icon>());
			..set_pixel_size(ICON_SIZE);
		}
	}

	fn build_text_widget(&self, text: Option<&str>, min_chars: Option<u32>) -> gtk::Label {
		cascade! {
			gtk::Label::new(text);
			// width-chars is based off of the average font width, so we add 1
			// to make sure that it's wide enough.
			..set_width_chars(min_chars.map_or(-1, |v| (v + 1) as i32));
			..set_halign(gtk::Align::Center);
			..add_css_class("title-4");
		}
	}

	fn build_progress_widget(&self, fraction: f64) -> gtk::ProgressBar {
		cascade! {
			gtk::ProgressBar::new();
			..set_fraction(fraction);
			..set_valign(gtk::Align::Center);
			..set_hexpand(true);
		}
	}

	fn build_segmented_progress_widget(
		&self,
		value: u32,
		n_segments: u32,
	) -> SegmentedProgressWidget {
		cascade! {
			SegmentedProgressWidget::new(n_segments);
			..set_value(value);
			..set_valign(gtk::Align::Center);
			..set_hexpand(true);
		}
	}
}
