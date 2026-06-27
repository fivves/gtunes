use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::Serialize;
use std::time::Duration;
use thiserror::Error;
use url::Url;

use crate::config;

use super::models::{
    JellyfinAuthResponse, JellyfinItemSummary, JellyfinItemsResponse, JellyfinPlaylist,
    JellyfinTrack,
};

const MUSIC_TRACK_PAGE_SIZE: u32 = 500;
const PLAYLIST_PAGE_SIZE: u32 = 200;

const MUSIC_TRACK_FIELDS: &str = "MediaSources,Genres,DateCreated,DateLastSaved,ArtistItems,AlbumArtists,ParentIndexNumber,IndexNumber";
const MUSIC_TRACK_SORT: &str = "Album,ParentIndexNumber,IndexNumber,SortName";
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Error)]
pub enum JellyfinClientError {
    #[error("invalid Jellyfin server URL: {0}")]
    InvalidServerUrl(#[from] url::ParseError),
    #[error("invalid authorization header")]
    InvalidAuthHeader,
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
}

#[derive(Clone, Debug)]
pub struct JellyfinClient {
    blocking_http: reqwest::blocking::Client,
    server_url: Url,
    access_token: Option<String>,
}

impl JellyfinClient {
    pub fn new(
        server_url: &str,
        access_token: Option<String>,
    ) -> Result<Self, JellyfinClientError> {
        let server_url = Url::parse(server_url)?;
        let mut headers = HeaderMap::new();

        if let Some(token) = access_token.as_deref() {
            headers.insert(AUTHORIZATION, auth_header(token)?);
        }

        let user_agent = format!("{}/{}", config::APP_NAME, config::VERSION);
        let blocking_http = reqwest::blocking::Client::builder()
            .user_agent(user_agent)
            .default_headers(headers)
            .timeout(HTTP_TIMEOUT)
            .build()?;

        Ok(Self {
            blocking_http,
            server_url,
            access_token,
        })
    }

    pub fn authenticate(
        server_url: &str,
        username: &str,
        password: &str,
    ) -> Result<(Self, JellyfinAuthResponse), JellyfinClientError> {
        let unauthenticated = Self::new(server_url, None)?;
        let endpoint = unauthenticated
            .server_url
            .join("Users/AuthenticateByName")?;
        let auth = unauthenticated
            .blocking_http
            .post(endpoint)
            .header("X-Emby-Authorization", authorization_header_value(None))
            .json(&AuthenticateByName {
                username,
                pw: password,
            })
            .send()?
            .error_for_status()?
            .json::<JellyfinAuthResponse>()?;

        let authenticated = Self::new(server_url, Some(auth.access_token.clone()))?;
        Ok((authenticated, auth))
    }

    pub fn server_url(&self) -> &Url {
        &self.server_url
    }

    pub fn is_authenticated(&self) -> bool {
        self.access_token.is_some()
    }

    pub fn item_direct_stream_url(&self, item_id: &str) -> Result<Url, JellyfinClientError> {
        let mut url = self.server_url.join(&format!("Audio/{item_id}/stream"))?;
        url.query_pairs_mut().append_pair("static", "true");
        if let Some(token) = self.access_token.as_deref() {
            url.query_pairs_mut().append_pair("api_key", token);
        }
        Ok(url)
    }

    pub fn item_transcode_stream_url(&self, item_id: &str) -> Result<Url, JellyfinClientError> {
        let mut url = self.server_url.join(&format!(
            "Audio/{item_id}/universal?UserId=&Container=opus,mp3,aac,m4a,flac,webma,webm,wav&TranscodingContainer=mp3&AudioCodec=mp3&EnableDirectPlay=false&EnableDirectStream=false&EnableRedirection=true"
        ))?;
        if let Some(token) = self.access_token.as_deref() {
            url.query_pairs_mut().append_pair("api_key", token);
        }
        Ok(url)
    }

    pub fn stream_http_headers(&self) -> Vec<(String, String)> {
        stream_http_headers_for_token(self.access_token.as_deref())
    }

    pub fn item_image_url(
        &self,
        item_id: &str,
        image_kind: &str,
    ) -> Result<Url, JellyfinClientError> {
        self.item_image_url_with_size(item_id, image_kind, None)
    }

    pub fn item_image_url_with_size(
        &self,
        item_id: &str,
        image_kind: &str,
        max_size: Option<u32>,
    ) -> Result<Url, JellyfinClientError> {
        let mut url = self
            .server_url
            .join(&format!("Items/{item_id}/Images/{image_kind}"))?;
        {
            let mut query = url.query_pairs_mut();
            if let Some(max_size) = max_size {
                let max_size = max_size.to_string();
                query
                    .append_pair("maxWidth", &max_size)
                    .append_pair("maxHeight", &max_size)
                    .append_pair("quality", "80");
            }
            if let Some(token) = self.access_token.as_deref() {
                query.append_pair("api_key", token);
            }
        }
        Ok(url)
    }

    pub fn music_tracks_with_progress<F>(
        &self,
        user_id: &str,
        progress: F,
    ) -> Result<Vec<JellyfinTrack>, JellyfinClientError>
    where
        F: FnMut(usize, Option<usize>),
    {
        fetch_all_pages(
            |start, limit| self.music_tracks_page(user_id, start, limit),
            MUSIC_TRACK_PAGE_SIZE,
            progress,
        )
    }

    pub fn music_track_summaries_with_progress<F>(
        &self,
        user_id: &str,
        progress: F,
    ) -> Result<Vec<JellyfinItemSummary>, JellyfinClientError>
    where
        F: FnMut(usize, Option<usize>),
    {
        fetch_all_pages(
            |start, limit| self.music_track_summaries_page(user_id, start, limit),
            MUSIC_TRACK_PAGE_SIZE,
            progress,
        )
    }

    fn music_tracks_page(
        &self,
        user_id: &str,
        start_index: u32,
        limit: u32,
    ) -> Result<JellyfinItemsResponse<JellyfinTrack>, JellyfinClientError> {
        let mut endpoint = self.server_url.join(&format!("Users/{user_id}/Items"))?;
        endpoint
            .query_pairs_mut()
            .append_pair("Recursive", "true")
            .append_pair("IncludeItemTypes", "Audio")
            .append_pair("Fields", MUSIC_TRACK_FIELDS)
            .append_pair("SortBy", MUSIC_TRACK_SORT)
            .append_pair("SortOrder", "Ascending")
            .append_pair("StartIndex", &start_index.to_string())
            .append_pair("Limit", &limit.to_string());

        let response = self
            .blocking_http
            .get(endpoint)
            .send()?
            .error_for_status()?
            .json::<JellyfinItemsResponse<JellyfinTrack>>()?;
        Ok(response)
    }

    fn music_track_summaries_page(
        &self,
        user_id: &str,
        start_index: u32,
        limit: u32,
    ) -> Result<JellyfinItemsResponse<JellyfinItemSummary>, JellyfinClientError> {
        let mut endpoint = self.server_url.join(&format!("Users/{user_id}/Items"))?;
        endpoint
            .query_pairs_mut()
            .append_pair("Recursive", "true")
            .append_pair("IncludeItemTypes", "Audio")
            .append_pair("Fields", "DateLastSaved")
            .append_pair("SortBy", MUSIC_TRACK_SORT)
            .append_pair("SortOrder", "Ascending")
            .append_pair("StartIndex", &start_index.to_string())
            .append_pair("Limit", &limit.to_string());

        let response = self
            .blocking_http
            .get(endpoint)
            .send()?
            .error_for_status()?
            .json::<JellyfinItemsResponse<JellyfinItemSummary>>()?;
        Ok(response)
    }

    pub fn music_tracks_by_ids(
        &self,
        user_id: &str,
        ids: &[String],
    ) -> Result<Vec<JellyfinTrack>, JellyfinClientError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut endpoint = self.server_url.join(&format!("Users/{user_id}/Items"))?;
        endpoint
            .query_pairs_mut()
            .append_pair("Recursive", "true")
            .append_pair("IncludeItemTypes", "Audio")
            .append_pair("Ids", &ids.join(","))
            .append_pair("Fields", MUSIC_TRACK_FIELDS)
            .append_pair("SortBy", MUSIC_TRACK_SORT)
            .append_pair("SortOrder", "Ascending")
            .append_pair("Limit", &ids.len().to_string());

        let response = self
            .blocking_http
            .get(endpoint)
            .send()?
            .error_for_status()?
            .json::<JellyfinItemsResponse<JellyfinTrack>>()?;
        Ok(response.items)
    }

    pub fn music_playlists(
        &self,
        user_id: &str,
    ) -> Result<Vec<JellyfinPlaylist>, JellyfinClientError> {
        fetch_all_pages(
            |start, limit| self.music_playlists_page(user_id, start, limit),
            PLAYLIST_PAGE_SIZE,
            |_, _| {},
        )
    }

    pub fn music_playlist_summaries(
        &self,
        user_id: &str,
    ) -> Result<Vec<JellyfinItemSummary>, JellyfinClientError> {
        fetch_all_pages(
            |start, limit| self.music_playlist_summaries_page(user_id, start, limit),
            PLAYLIST_PAGE_SIZE,
            |_, _| {},
        )
    }

    fn music_playlists_page(
        &self,
        user_id: &str,
        start_index: u32,
        limit: u32,
    ) -> Result<JellyfinItemsResponse<JellyfinPlaylist>, JellyfinClientError> {
        let mut endpoint = self.server_url.join(&format!("Users/{user_id}/Items"))?;
        endpoint
            .query_pairs_mut()
            .append_pair("Recursive", "true")
            .append_pair("IncludeItemTypes", "Playlist")
            .append_pair("Fields", "DateLastSaved")
            .append_pair("SortBy", "SortName")
            .append_pair("SortOrder", "Ascending")
            .append_pair("StartIndex", &start_index.to_string())
            .append_pair("Limit", &limit.to_string());

        let response = self
            .blocking_http
            .get(endpoint)
            .send()?
            .error_for_status()?
            .json::<JellyfinItemsResponse<JellyfinPlaylist>>()?;
        Ok(response)
    }

    fn music_playlist_summaries_page(
        &self,
        user_id: &str,
        start_index: u32,
        limit: u32,
    ) -> Result<JellyfinItemsResponse<JellyfinItemSummary>, JellyfinClientError> {
        let mut endpoint = self.server_url.join(&format!("Users/{user_id}/Items"))?;
        endpoint
            .query_pairs_mut()
            .append_pair("Recursive", "true")
            .append_pair("IncludeItemTypes", "Playlist")
            .append_pair("Fields", "DateLastSaved")
            .append_pair("SortBy", "SortName")
            .append_pair("SortOrder", "Ascending")
            .append_pair("StartIndex", &start_index.to_string())
            .append_pair("Limit", &limit.to_string());

        let response = self
            .blocking_http
            .get(endpoint)
            .send()?
            .error_for_status()?
            .json::<JellyfinItemsResponse<JellyfinItemSummary>>()?;
        Ok(response)
    }

    pub fn playlist_tracks(
        &self,
        user_id: &str,
        playlist_id: &str,
    ) -> Result<Vec<JellyfinTrack>, JellyfinClientError> {
        fetch_all_pages(
            |start, limit| self.playlist_tracks_page(user_id, playlist_id, start, limit),
            MUSIC_TRACK_PAGE_SIZE,
            |_, _| {},
        )
    }

    fn playlist_tracks_page(
        &self,
        user_id: &str,
        playlist_id: &str,
        start_index: u32,
        limit: u32,
    ) -> Result<JellyfinItemsResponse<JellyfinTrack>, JellyfinClientError> {
        let mut endpoint = self
            .server_url
            .join(&format!("Playlists/{playlist_id}/Items"))?;
        endpoint
            .query_pairs_mut()
            .append_pair("UserId", user_id)
            .append_pair("IncludeItemTypes", "Audio")
            .append_pair(
                "Fields",
                "MediaSources,Genres,DateCreated,ArtistItems,AlbumArtists,ParentIndexNumber,IndexNumber",
            )
            .append_pair("StartIndex", &start_index.to_string())
            .append_pair("Limit", &limit.to_string());

        let response = self
            .blocking_http
            .get(endpoint)
            .send()?
            .error_for_status()?
            .json::<JellyfinItemsResponse<JellyfinTrack>>()?;
        Ok(response)
    }
}

fn fetch_all_pages<T, F, P>(
    mut fetch_page: F,
    page_size: u32,
    mut on_progress: P,
) -> Result<Vec<T>, JellyfinClientError>
where
    F: FnMut(u32, u32) -> Result<JellyfinItemsResponse<T>, JellyfinClientError>,
    P: FnMut(usize, Option<usize>),
{
    let mut items = Vec::new();
    let mut start_index = 0u32;
    let mut total = None;

    loop {
        let response = fetch_page(start_index, page_size)?;
        let page_len = response.items.len();

        if total.is_none() {
            total = response.total_record_count.map(|n| n.max(0) as usize);
        }

        items.extend(response.items);
        on_progress(items.len(), total);

        let reached_total = total.map(|t| items.len() >= t).unwrap_or(false);
        if page_len == 0 || reached_total || page_len < page_size as usize {
            break;
        }

        start_index += page_len as u32;
    }

    Ok(items)
}

pub fn stream_http_headers_for_token(token: Option<&str>) -> Vec<(String, String)> {
    token
        .map(|token| vec![("X-Emby-Token".to_string(), token.to_string())])
        .unwrap_or_default()
}

fn auth_header(token: &str) -> Result<HeaderValue, JellyfinClientError> {
    HeaderValue::from_str(&authorization_header_value(Some(token)))
        .map_err(|_| JellyfinClientError::InvalidAuthHeader)
}

fn authorization_header_value(token: Option<&str>) -> String {
    match token {
        Some(token) => format!(
            "MediaBrowser Client=\"{}\", Device=\"Linux Desktop\", DeviceId=\"gtunes-dev\", Version=\"{}\", Token=\"{}\"",
            config::APP_NAME,
            config::VERSION,
            token
        ),
        None => format!(
            "MediaBrowser Client=\"{}\", Device=\"Linux Desktop\", DeviceId=\"gtunes-dev\", Version=\"{}\"",
            config::APP_NAME,
            config::VERSION,
        ),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct AuthenticateByName<'a> {
    username: &'a str,
    pw: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_headers_include_jellyfin_token_when_available() {
        assert_eq!(
            stream_http_headers_for_token(Some("secret")),
            vec![("X-Emby-Token".to_string(), "secret".to_string())]
        );
        assert!(stream_http_headers_for_token(None).is_empty());
    }

    #[test]
    fn stream_urls_prefer_direct_play_and_expose_transcode_fallback() {
        let client = JellyfinClient::new("https://jellyfin.example/base/", Some("token".into()))
            .expect("valid client");

        let direct = client
            .item_direct_stream_url("track-id")
            .expect("direct stream url");
        assert_eq!(
            direct.as_str(),
            "https://jellyfin.example/base/Audio/track-id/stream?static=true&api_key=token"
        );

        let fallback = client
            .item_transcode_stream_url("track-id")
            .expect("transcode stream url");
        let query = fallback.query().expect("query string");
        assert_eq!(fallback.path(), "/base/Audio/track-id/universal");
        assert!(query.contains("EnableDirectPlay=false"));
        assert!(query.contains("AudioCodec=mp3"));
        assert!(query.contains("api_key=token"));
    }
}
