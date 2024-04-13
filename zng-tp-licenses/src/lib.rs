#![doc(html_favicon_url = "https://raw.githubusercontent.com/zng-ui/zng/master/examples/res/image/zng-logo-icon.png")]
#![doc(html_logo_url = "https://raw.githubusercontent.com/zng-ui/zng/master/examples/res/image/zng-logo.png")]
#![doc = include_str!(concat!("../", std::env!("CARGO_PKG_README")))]
//! Third party license management and collection.
//!
//! Some licenses require that they must be accessible in the final binary, usually in an "about" screen. This
//! crate defines the data type for this about screen and optionally uses `cargo about` to find and bundle all
//! license files.
//!
//!

use std::fmt;

use serde::{Deserialize, Serialize};
use zng_txt::Txt;

/// Represents a license and dependencies that use it.
#[derive(Serialize, Deserialize, Clone)]
pub struct LicenseUsed {
    /// License name and text.
    pub license: License,
    /// Project or packages that use this license.
    pub used_by: Vec<User>,
}
impl fmt::Debug for LicenseUsed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("License")
            .field("license.id", &self.license.id)
            .field("used_by", &self.used_by)
            .finish_non_exhaustive()
    }
}
impl LicenseUsed {
    /// Invert data to be keyed by user.
    pub fn user_licenses(&self) -> Vec<UserLicense> {
        self.used_by
            .iter()
            .map(|u| UserLicense {
                user: u.clone(),
                license: self.license.clone(),
            })
            .collect()
    }
}

/// Invert data to be keyed by user, also sorts by user name.
pub fn user_licenses(licenses: &[LicenseUsed]) -> Vec<UserLicense> {
    let mut r: Vec<_> = licenses.iter().flat_map(|l| l.user_licenses()).collect();
    r.sort_by(|a, b| a.user.name.cmp(&b.user.name));
    r
}

/// Represents a license user with license.
#[derive(Clone)]
pub struct UserLicense {
    /// License user.
    pub user: User,
    /// License used.
    pub license: License,
}
impl fmt::Debug for UserLicense {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UserLicense")
            .field("user", &self.user)
            .field("license.id", &self.license.id)
            .finish()
    }
}

/// Represents a license id, name and text.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
pub struct License {
    /// License [SPDX] id.
    ///
    /// [SPDX]: https://spdx.org/licenses/
    pub id: Txt,
    /// License name.
    pub name: Txt,
    /// License text.
    pub text: Txt,
}

/// Represents a project or package that uses a license.
#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Clone)]
pub struct User {
    /// Project or package name.
    pub name: Txt,
    /// Package version.
    #[serde(default)]
    pub version: Txt,
    /// Project or package URL.
    #[serde(default)]
    pub url: Txt,
}

/// Merge `licenses` into `into`.
///
/// The licenses and users are not sorted, call [`sort_licenses`] after merging all licenses.
pub fn merge_licenses(into: &mut Vec<LicenseUsed>, licenses: Vec<LicenseUsed>) {
    for license in licenses {
        if let Some(l) = into.iter_mut().find(|l| l.license == license.license) {
            for user in license.used_by {
                if !l.used_by.contains(&user) {
                    l.used_by.push(user);
                }
            }
        } else {
            into.push(license);
        }
    }
}

/// Sort vec by license name, and users of each license by name.
pub fn sort_licenses(l: &mut Vec<LicenseUsed>) {
    l.sort_by(|a, b| a.license.name.cmp(&b.license.name));
    for l in l {
        l.used_by.sort_by(|a, b| a.name.cmp(&b.name));
    }
}

/// Calls [`cargo about`] for the crate.
///
/// This method must be used in build scripts (`build.rs`).
///
/// # Panics
///
/// Panics for any error, including `cargo about` errors and JSON deserialization errors.
///
/// [`cargo about`]: https://github.com/EmbarkStudios/cargo-about
#[cfg(feature = "build")]
pub fn collect_cargo_about(about_cfg_path: &str) -> Vec<LicenseUsed> {
    let mut cargo_about = std::process::Command::new("cargo");
    cargo_about
        .arg("about")
        .arg("generate")
        .arg("--format")
        .arg("json")
        .arg("--all-features");

    if !about_cfg_path.is_empty() {
        cargo_about.arg("-c").arg(about_cfg_path);
    }

    let output = cargo_about.output().expect("error calling `cargo about`");
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(
        output.status.success(),
        "error code calling `cargo about`, {:?}\nstderr:\n{error}",
        output.status
    );

    let json = String::from_utf8(output.stdout).unwrap();

    parse_cargo_about(&json).expect("error parsing `cargo about` output")
}

/// Parse the output of [`cargo about`].
///
/// Example command:
///
/// ```console
/// cargo about generate -c .cargo/about.toml --format json --workspace --all-features
/// ```
///
/// See also [`collect_cargo_about`] that calls the command.
///
/// [`cargo about`]: https://github.com/EmbarkStudios/cargo-about
#[cfg(feature = "build")]
pub fn parse_cargo_about(json: &str) -> Result<Vec<LicenseUsed>, serde_json::Error> {
    #[derive(Deserialize)]
    struct Output {
        licenses: Vec<LicenseJson>,
    }
    #[derive(Deserialize)]
    struct LicenseJson {
        id: Txt,
        name: Txt,
        text: Txt,
        used_by: Vec<UsedBy>,
    }
    impl From<LicenseJson> for LicenseUsed {
        fn from(value: LicenseJson) -> Self {
            Self {
                license: License {
                    id: value.id,
                    name: value.name,
                    text: value.text,
                },
                used_by: value.used_by.into_iter().map(Into::into).collect(),
            }
        }
    }
    #[derive(Deserialize)]
    struct UsedBy {
        #[serde(rename = "crate")]
        crate_: Crate,
    }
    #[derive(Deserialize)]
    struct Crate {
        name: Txt,
        version: Txt,
        #[serde(default)]
        repository: Option<Txt>,
    }
    impl From<UsedBy> for User {
        fn from(value: UsedBy) -> Self {
            let repo = value.crate_.repository.unwrap_or_default();
            Self {
                version: value.crate_.version,
                url: if repo.is_empty() {
                    zng_txt::formatx!("https://crates.io/crates/{}", value.crate_.name)
                } else {
                    repo
                },
                name: value.crate_.name,
            }
        }
    }

    serde_json::from_str::<Output>(json).map(|o| o.licenses.into_iter().map(Into::into).collect())
}

/// Bincode serialize and deflate the licenses.
///
/// # Panics
///
/// Panics in case of any error.
#[cfg(feature = "build")]
pub fn encode_licenses(licenses: &[LicenseUsed]) -> Vec<u8> {
    deflate::deflate_bytes(&bincode::serialize(licenses).expect("bincode error"))
}

/// Encode licenses and write to the output file that is included by [`include_bundle!`].
///
/// # Panics
///
/// Panics in case of any error.
#[cfg(feature = "build")]
pub fn write_bundle(licenses: &[LicenseUsed]) {
    let bin = encode_licenses(licenses);
    std::fs::write(format!("{}/zng-tp-licenses.bin", std::env::var("OUT_DIR").unwrap()), bin).expect("error writing file");
}

/// Includes the bundle file generated using [`write_bundle`].
///
/// This macro output is a `Vec<License>`. Note that if not built with `feature = "bundle"` this
/// macro always returns an empty vec.
#[macro_export]
#[cfg(feature = "bundle")]
macro_rules! include_bundle {
    () => {
        $crate::include_bundle!(concat!(env!("OUT_DIR"), "/zng-tp-licenses.bin"))
    };
    ($custom_name:expr) => {{
        $crate::decode_licenses(include_bytes!($custom_name))
    }};
}

/// Includes the bundle file generated using [`write_bundle`].
///
/// This macro output is a `Vec<License>`. Note that if not built with `feature = "bundle"` this
/// macro always returns an empty vec.
#[macro_export]
#[cfg(not(feature = "bundle"))]
macro_rules! include_bundle {
    () => {
        $crate::include_bundle!(concat!(env!("OUT_DIR"), "/zng-tp-licenses.bin"))
    };
    ($custom_name:expr) => {{
        Vec::<$crate::License>::new()
    }};
}

/// Decode licenses encoded with [`encode_licenses`]. Note that the encoded format is only guaranteed to work
/// if both encoding and decoding is made with the same `Cargo.lock` dependencies.
#[cfg(feature = "bundle")]
pub fn decode_licenses(bin: &[u8]) -> Vec<LicenseUsed> {
    let bin = inflate::inflate_bytes(bin).expect("invalid bundle deflate binary");
    bincode::deserialize(&bin).expect("invalid bundle bincode binary")
}
