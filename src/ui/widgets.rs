use super::prelude::*;

pub(crate) fn menu_item_button(icon_name: &str, title: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("flat");
    button.add_css_class("settings-menu-item");
    button.set_halign(Align::Fill);

    let row = gtk::Box::new(Orientation::Horizontal, 12);
    row.set_margin_top(7);
    row.set_margin_bottom(7);
    row.set_margin_start(8);
    row.set_margin_end(8);
    row.set_halign(Align::Fill);
    row.append(&gtk::Image::from_icon_name(icon_name));
    let title = label(title, "settings-menu-label");
    title.set_hexpand(true);
    title.set_halign(Align::Start);
    row.append(&title);
    button.set_child(Some(&row));
    button
}

pub(crate) fn label(text: &str, class_name: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_valign(Align::Center);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    if !class_name.is_empty() {
        label.add_css_class(class_name);
    }
    label
}

pub(crate) fn icon_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon_name)
        .tooltip_text(tooltip)
        .build();
    button.add_css_class("icon-button");
    button
}

pub(crate) fn cover_art(size: i32) -> gtk::Image {
    let art = gtk::Image::new();
    art.add_css_class("cover");
    art.set_size_request(size, size);
    art.set_pixel_size(size);
    art
}

pub(crate) fn format_duration(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes}:{seconds:02}")
}

pub(crate) fn rounded_rect(
    cr: &gtk::cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
) {
    let radius = radius.min(width / 2.0).min(height / 2.0);
    cr.new_sub_path();
    cr.arc(
        x + width - radius,
        y + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    cr.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    cr.arc(
        x + radius,
        y + height - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + radius,
        y + radius,
        radius,
        std::f64::consts::PI,
        std::f64::consts::PI * 1.5,
    );
    cr.close_path();
}

#[allow(dead_code)]
pub(crate) fn set_margin_all(widget: &impl IsA<gtk::Widget>, margin: i32) {
    widget.set_margin_top(margin);
    widget.set_margin_bottom(margin);
    widget.set_margin_start(margin);
    widget.set_margin_end(margin);
}
