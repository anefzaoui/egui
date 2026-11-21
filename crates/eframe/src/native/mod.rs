mod app_icon;
mod epi_integration;
mod event_loop_context;
pub mod run;

pub mod viewport_hooks;

#[cfg(target_os = "macos")]
pub(crate) mod macos;

/// File storage which can be used by native backends.
#[cfg(feature = "persistence")]
pub mod file_storage;

pub(crate) mod winit_integration;

#[cfg(any(feature = "glow", feature = "wgpu_no_default_features"))]
mod pending_textures;

#[cfg(feature = "glow")]
mod glow_integration;

#[cfg(feature = "wgpu_no_default_features")]
mod wgpu_integration;
