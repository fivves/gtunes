#![allow(dead_code, unused_imports)]

mod client;
mod models;

pub use client::{JellyfinClient, JellyfinClientError};
pub use models::{
    ArtistImageKind, JellyfinAlbum, JellyfinArtist, JellyfinAuthResponse, JellyfinItemId,
    JellyfinItemsResponse, JellyfinPlaylist, JellyfinTrack, JellyfinUser,
};
