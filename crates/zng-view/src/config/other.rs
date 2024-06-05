#![allow(unused)]

use zng_view_api::config::{AnimationsConfig, ColorScheme, FontAntiAliasing, KeyRepeatConfig, MultiClickConfig, TouchConfig};

pub fn font_aa() -> FontAntiAliasing {
    warn("font_aa");
    FontAntiAliasing::Subpixel
}

pub fn multi_click_config() -> MultiClickConfig {
    warn("multi_click_config");
    MultiClickConfig::default()
}

pub fn animations_config() -> AnimationsConfig {
    warn("animations_config");
    AnimationsConfig::default()
}

pub fn key_repeat_config() -> KeyRepeatConfig {
    warn("key_repeat_config");
    KeyRepeatConfig::default()
}

pub fn touch_config() -> TouchConfig {
    warn("touch_config");
    TouchConfig::default()
}

pub fn color_scheme_config() -> ColorScheme {
    warn("color_scheme_config");
    ColorScheme::default()
}

#[cfg(not(windows))]
pub fn locale_config() -> LocaleConfig {
    LocaleConfig {
        langs: sys_locale::get_locale().into_iter().map(zng_txt::Txt::from).collect(),
    }
}

pub fn spawn_listener(_: crate::AppEventSender) {
    tracing::warn!("config events not implemented for {}", std::env::consts::OS);
}

fn warn(name: &str) {
    tracing::warn!("system '{name}' not implemented for {}", std::env::consts::OS);
}
