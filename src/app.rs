use adw::prelude::*;
use gtk::glib;

use crate::{config, ui};

pub fn run() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(config::APP_ID)
        .build();

    app.connect_startup(|_| {
        adw::StyleManager::default().set_color_scheme(adw::ColorScheme::Default);
        ui::styles::load();
    });

    app.connect_activate(|app| {
        let window = ui::window::build(app);
        window.present();
    });

    app.run()
}
