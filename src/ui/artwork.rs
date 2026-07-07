use super::prelude::*;

#[derive(Debug)]
pub(crate) enum ImageFetchError {
    Missing,
    HttpStatus(reqwest::StatusCode),
    Request(&'static str),
    Io(std::io::Error),
}

impl fmt::Display for ImageFetchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => formatter.write_str("image not found"),
            Self::HttpStatus(status) => write!(formatter, "HTTP status {status}"),
            Self::Request(message) => formatter.write_str(message),
            Self::Io(error) => write!(formatter, "io error: {error}"),
        }
    }
}

pub(crate) const COLLECTION_ARTWORK_INITIAL_DELAY_MS: u64 = 24;

pub(crate) const COLLECTION_ARTWORK_STAGGER_MS: u64 = 8;

pub(crate) const COLLECTION_ARTWORK_MAX_STAGGERED_ITEMS: usize = 160;

pub(crate) fn load_selected_cover_art(state: &Rc<RefCell<UiState>>) {
    let (url, cover) = {
        let ui = state.borrow();
        let url = current_display_track(&ui).and_then(|track| track.thumbnail_artwork_url.clone());
        let cover = ui.cover_art.clone();
        (url, cover)
    };

    let Some(url) = url else {
        if let Some(cover) = cover.as_ref() {
            cover.set_paintable(Option::<&gtk::gdk::Paintable>::None);
        }
        return;
    };
    let Some(cover) = cover else {
        return;
    };

    let (sender, receiver) = mpsc::channel();
    let request_url = url.clone();
    std::thread::spawn(move || {
        let result = fetch_cached_image_file(&request_url).map(|path| (request_url, path));
        let _ = sender.send(result);
    });

    let state = state.clone();
    gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
        match receiver.try_recv() {
            Ok(Ok((loaded_url, path))) => {
                let displayed_url = {
                    let ui = state.borrow();
                    current_display_track(&ui)
                        .and_then(|track| track.thumbnail_artwork_url.as_deref())
                        .map(str::to_string)
                };

                if displayed_url.as_deref() == Some(loaded_url.as_str()) {
                    let mut ui = state.borrow_mut();
                    sync_external_playback_metadata(&mut ui);
                    let file = gtk::gio::File::for_path(path);
                    match gtk::gdk::Texture::from_file(&file) {
                        Ok(texture) => cover.set_paintable(Some(&texture)),
                        Err(error) => tracing::warn!(%error, "failed to decode album artwork"),
                    }
                }
                gtk::glib::ControlFlow::Break
            }
            Ok(Err(error)) => {
                tracing::warn!(%error, "failed to fetch album artwork");
                gtk::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => gtk::glib::ControlFlow::Break,
        }
    });
}

pub(crate) fn show_full_size_artwork(state: &Rc<RefCell<UiState>>) {
    let (title, full_url, paintable) = {
        let ui = state.borrow();
        let track = current_display_track(&ui);
        let title = track
            .map(|track| track.title.clone())
            .unwrap_or_else(|| config::APP_NAME.to_string());
        let full_url = track.and_then(|track| track.artwork_url.clone());
        let paintable = ui.cover_art.as_ref().and_then(gtk::Image::paintable);
        (title, full_url, paintable)
    };

    if paintable.is_none() && full_url.is_none() {
        return;
    }

    let picture = gtk::Picture::new();
    if let Some(paintable) = paintable.as_ref() {
        picture.set_paintable(Some(paintable));
    }
    picture.set_can_shrink(true);

    let window = gtk::Window::builder()
        .title(title)
        .decorated(false)
        .default_width(720)
        .default_height(720)
        .build();
    window.set_child(Some(&picture));
    window.present();

    if let Some(full_url) = full_url {
        load_full_size_artwork(full_url, picture);
    }
}

pub(crate) fn load_full_size_artwork(url: String, picture: gtk::Picture) {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = fetch_cached_image_file(&url);
        let _ = sender.send(result);
    });

    gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
        match receiver.try_recv() {
            Ok(Ok(path)) => {
                let file = gtk::gio::File::for_path(path);
                match gtk::gdk::Texture::from_file(&file) {
                    Ok(texture) => picture.set_paintable(Some(&texture)),
                    Err(error) => tracing::warn!(%error, "failed to decode full-size artwork"),
                }
                gtk::glib::ControlFlow::Break
            }
            Ok(Err(error)) => {
                tracing::warn!(%error, "failed to fetch full-size artwork");
                gtk::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => gtk::glib::ControlFlow::Break,
        }
    });
}

pub(crate) fn image_http_client() -> Result<&'static reqwest::blocking::Client, ImageFetchError> {
    // Artwork loads happen in bursts (one per visible tile); share one client
    // so connections are pooled instead of paying TLS setup per image.
    static CLIENT: std::sync::OnceLock<Option<reqwest::blocking::Client>> =
        std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .ok()
        })
        .as_ref()
        .ok_or(ImageFetchError::Request("request client failed"))
}

pub(crate) fn fetch_image_file(url: &str) -> Result<PathBuf, ImageFetchError> {
    let response = image_http_client()?
        .get(url)
        .send()
        .map_err(|_| ImageFetchError::Request("request failed"))?;
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(ImageFetchError::Missing);
    }
    if !status.is_success() {
        return Err(ImageFetchError::HttpStatus(status));
    }

    let bytes = response
        .bytes()
        .map_err(|_| ImageFetchError::Request("failed to read response body"))?;

    let path = artwork_cache_path(url);
    std::fs::write(&path, bytes).map_err(ImageFetchError::Io)?;
    Ok(path)
}

pub(crate) fn fetch_cached_image_file(url: &str) -> Result<PathBuf, ImageFetchError> {
    let path = artwork_cache_path(url);
    if path.exists() {
        Ok(path)
    } else {
        fetch_image_file(url)
    }
}

pub(crate) fn load_queue_art(
    url: Option<String>,
    image: gtk::Image,
    current_url: Rc<RefCell<Option<String>>>,
) {
    let Some(url) = url else {
        return;
    };

    let (sender, receiver) = mpsc::channel();
    let request_url = url.clone();
    std::thread::spawn(move || {
        let result = fetch_cached_image_file(&request_url);
        let _ = sender.send(result);
    });

    gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
        match receiver.try_recv() {
            Ok(Ok(path)) => {
                if current_url.borrow().as_deref() != Some(url.as_str()) {
                    return gtk::glib::ControlFlow::Break;
                }
                let file = gtk::gio::File::for_path(path);
                match gtk::gdk::Texture::from_file(&file) {
                    Ok(texture) => {
                        image.set_paintable(Some(&texture));
                        image.remove_css_class("artwork-loading");
                    }
                    Err(error) => {
                        tracing::warn!(%error, "failed to decode queue artwork");
                        image.remove_css_class("artwork-loading");
                    }
                }
                gtk::glib::ControlFlow::Break
            }
            Ok(Err(error)) => {
                tracing::warn!(%error, "failed to fetch queue artwork");
                image.remove_css_class("artwork-loading");
                gtk::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => gtk::glib::ControlFlow::Break,
        }
    });
}

pub(crate) fn load_collection_queue_art(
    url: Option<String>,
    image: gtk::Image,
    current_url: Rc<RefCell<Option<String>>>,
    tile_index: usize,
) {
    gtk::glib::timeout_add_local_once(collection_artwork_delay(tile_index), move || {
        load_queue_art(url, image, current_url);
    });
}

pub(crate) fn load_collection_picture_art(url: String, image: gtk::Image, tile_index: usize) {
    gtk::glib::timeout_add_local_once(collection_artwork_delay(tile_index), move || {
        load_picture_art(url, image);
    });
}

pub(crate) fn collection_artwork_delay(tile_index: usize) -> Duration {
    let stagger_index = tile_index.min(COLLECTION_ARTWORK_MAX_STAGGERED_ITEMS) as u64;
    Duration::from_millis(
        COLLECTION_ARTWORK_INITIAL_DELAY_MS + (stagger_index * COLLECTION_ARTWORK_STAGGER_MS),
    )
}

pub(crate) fn load_picture_art(url: String, image: gtk::Image) {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = fetch_cached_image_file(&url);
        let _ = sender.send(result);
    });

    gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
        match receiver.try_recv() {
            Ok(Ok(path)) => {
                let file = gtk::gio::File::for_path(path);
                match gtk::gdk::Texture::from_file(&file) {
                    Ok(texture) => {
                        image.remove_css_class("artist-placeholder");
                        image.remove_css_class("artwork-loading");
                        image.set_pixel_size(ARTIST_ART_SIZE);
                        image.set_paintable(Some(&texture));
                    }
                    Err(error) => {
                        tracing::warn!(%error, "failed to decode artist artwork");
                        image.remove_css_class("artwork-loading");
                    }
                }
                gtk::glib::ControlFlow::Break
            }
            Ok(Err(ImageFetchError::Missing)) => {
                tracing::debug!("artist artwork is unavailable");
                image.remove_css_class("artwork-loading");
                gtk::glib::ControlFlow::Break
            }
            Ok(Err(error)) => {
                tracing::warn!(%error, "failed to fetch artist artwork");
                image.remove_css_class("artwork-loading");
                gtk::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => gtk::glib::ControlFlow::Break,
        }
    });
}
