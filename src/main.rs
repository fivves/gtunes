mod app;
mod cache;
mod config;
mod discord;
mod jellyfin;
mod playback;
mod ui;
mod waveform;

fn main() -> gtk::glib::ExitCode {
    tracing_subscriber::fmt()
        .compact()
        .with_target(false)
        .init();

    if let Err(error) = gst::init() {
        tracing::error!(%error, "failed to initialize GStreamer");
        return gtk::glib::ExitCode::FAILURE;
    }

    app::run()
}
