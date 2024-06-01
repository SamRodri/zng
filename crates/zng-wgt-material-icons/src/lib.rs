#![doc(html_favicon_url = "https://raw.githubusercontent.com/zng-ui/zng/main/examples/image/res/zng-logo-icon.png")]
#![doc(html_logo_url = "https://raw.githubusercontent.com/zng-ui/zng/main/examples/image/res/zng-logo.png")]
//!
//! Material icons for the [`Icon!`] widget.
//!
//! A constant for each icon is defined in a module for each font. The font files are embedded
//! and can be registered using the [`MaterialFonts`] app extension.
//!
//! The icons are from the [Material Design Icons] project.
//!
//! [`Icon!`]: struct@zng_wgt_text::icon::Icon
//! [Material Design Icons]: https://github.com/google/material-design-icons
//!
//! # Crate
//!
#![doc = include_str!(concat!("../", std::env!("CARGO_PKG_README")))]
#![warn(unused_extern_crates)]
#![warn(missing_docs)]

use std::{fmt, mem};
use zng_ext_font::FontName;
use zng_wgt_text::icon::GlyphIcon;

/// Material fonts.
///
/// You can call the [`MaterialFonts::register`] method yourself before creating any windows or you can
/// use this struct as an app extension that does the same thing on app init.
#[cfg(feature = "embedded")]
pub struct MaterialFonts;
#[cfg(feature = "embedded")]
impl MaterialFonts {
    /// Register the material fonts in an app.
    ///
    /// The fonts will be available after the current update.
    pub fn register() {
        let sets = [
            #[cfg(feature = "outlined")]
            (outlined::meta::FONT_NAME, outlined::meta::FONT_BYTES),
            #[cfg(feature = "filled")]
            (filled::meta::FONT_NAME, filled::meta::FONT_BYTES),
            #[cfg(feature = "rounded")]
            (rounded::meta::FONT_NAME, rounded::meta::FONT_BYTES),
            #[cfg(feature = "sharp")]
            (sharp::meta::FONT_NAME, sharp::meta::FONT_BYTES),
        ];

        for (name, bytes) in sets {
            let font = zng_ext_font::CustomFont::from_bytes(name, zng_ext_font::FontDataRef::from_static(bytes), 0);
            zng_ext_font::FONTS.register(font);
        }
    }
}
#[cfg(feature = "embedded")]
impl zng_app::AppExtension for MaterialFonts {
    fn init(&mut self) {
        MaterialFonts::register();
    }
}

/// Represents a material font icon.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MaterialIcon {
    /// Font name.
    pub font: FontName,
    /// Constant name of the icon.
    pub name: &'static str,
    /// Codepoint.
    pub code: char,
}
impl MaterialIcon {
    /// Format the name for display.
    pub fn display_name(&self) -> String {
        format!("{self}")
    }
}
zng_var::impl_from_and_into_var! {
    fn from(icon: MaterialIcon) -> GlyphIcon {
        GlyphIcon::new(icon.font, icon.code)
    }
}
impl fmt::Display for MaterialIcon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut chars = self.name.chars().peekable();
        if let Some(n) = chars.next() {
            // skip N if followed by number.
            if n == 'N' {
                if let Some(q) = chars.peek() {
                    if !q.is_ascii_digit() {
                        write!(f, "{n}")?;
                    }
                }
            } else {
                write!(f, "{n}")?;
            }
        }
        let mut is_cap = false;
        for c in chars {
            if c == '_' {
                write!(f, " ")?;
                is_cap = true;
            } else if mem::take(&mut is_cap) {
                write!(f, "{c}")?;
            } else {
                write!(f, "{}", c.to_lowercase())?;
            }
        }

        Ok(())
    }
}

/// Outline icons.
///  
/// This is the "Material Icons Outlined" font.
#[cfg(feature = "outlined")]
pub mod outlined {
    use super::*;

    /// Font metadata.
    pub mod meta {
        use super::*;

        /// "Material Icons Outlined".
        pub const FONT_NAME: FontName = FontName::from_static("Material Icons Outlined");

        /// Embedded font bytes.
        #[cfg(feature = "embedded")]
        pub const FONT_BYTES: &[u8] = include_bytes!("../fonts/MaterialIconsOutlined-Regular.otf");
    }

    include!(concat!(env!("OUT_DIR"), "/generated.outlined.rs"));
}

/// Filled icons.
///
/// This is the "Material Icons" font.
#[cfg(feature = "filled")]
pub mod filled {
    use super::*;

    /// Font metadata.
    pub mod meta {
        use super::*;

        /// "Material Icons".
        pub const FONT_NAME: FontName = FontName::from_static("Material Icons");

        /// Embedded font bytes.
        #[cfg(feature = "embedded")]
        pub const FONT_BYTES: &[u8] = include_bytes!("../fonts/MaterialIcons-Regular.ttf");
    }

    include!(concat!(env!("OUT_DIR"), "/generated.filled.rs"));
}

/// Rounded icons.
///  
/// This is the "Material Icons Rounded" font.
#[cfg(feature = "rounded")]
pub mod rounded {
    use super::*;

    /// Font metadata.
    pub mod meta {
        use super::*;

        /// "Material Icons Rounded".
        pub const FONT_NAME: FontName = FontName::from_static("Material Icons Rounded");

        /// Embedded font bytes.
        #[cfg(feature = "embedded")]
        pub const FONT_BYTES: &[u8] = include_bytes!("../fonts/MaterialIconsRound-Regular.otf");
    }

    include!(concat!(env!("OUT_DIR"), "/generated.rounded.rs"));
}

/// Sharp icons.
///  
/// This is the "Material Icons Sharp" font.
#[cfg(feature = "sharp")]
pub mod sharp {
    use super::*;

    /// Font metadata.
    pub mod meta {
        use super::*;

        /// "Material Icons Sharp".
        pub const FONT_NAME: FontName = FontName::from_static("Material Icons Sharp");

        /// Embedded font bytes.
        #[cfg(feature = "embedded")]
        pub const FONT_BYTES: &[u8] = include_bytes!("../fonts/MaterialIconsSharp-Regular.otf");
    }

    include!(concat!(env!("OUT_DIR"), "/generated.sharp.rs"));
}
