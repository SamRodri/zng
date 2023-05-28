//! Localization service [`L10N`] and helpers.
//!

use crate::{
    app::{raw_events::RAW_LOCALE_CONFIG_CHANGED_EVENT, view_process::VIEW_PROCESS_INITED_EVENT, AppExtension},
    event::EventUpdate,
    task,
    text::Txt,
    var::{types::ArcCowVar, *},
};

use std::{collections::HashMap, path::PathBuf, str::FromStr, sync::Arc};

mod types;
pub use types::*;

mod service;
use service::*;

mod sources;
pub use sources::*;

/// Localization service.
pub struct L10N;

/// Application extension that provides localization.
///
/// # Services
///
/// Services this extension provides.
///
/// * [`L10N`]
///
/// # Default
///
/// This extension is included in the [default app].
///
/// [default app]: crate::app::App::default
#[derive(Default)]
pub struct L10nManager {}
impl AppExtension for L10nManager {
    fn event_preview(&mut self, update: &mut EventUpdate) {
        if let Some(u) = RAW_LOCALE_CONFIG_CHANGED_EVENT
            .on(update)
            .map(|args| &args.config.langs)
            .or_else(|| VIEW_PROCESS_INITED_EVENT.on(update).map(|args| &args.locale_config.langs))
        {
            let lang = u
                .iter()
                .filter_map(|s| match Lang::from_str(s) {
                    Ok(l) => Some(l),
                    Err(e) => {
                        tracing::error!("received invalid lang from view-process, `{s}`, {e}");
                        None
                    }
                })
                .collect();

            L10N_SV.read().sys_lang.set_ne(Langs(lang));
        }
    }

    fn update_preview(&mut self) {
        L10N_SV.write().update();
    }
}

///<span data-del-macro-root></span> Gets a variable that localizes and formats the text in a widget context.
///
/// # Syntax
///
/// Macro expects a message key string literal a *message template* string literal that is also used
/// as fallback, followed by optional named format arguments `arg = <arg>,..`.
///
/// The message string syntax is the [Fluent Project] syntax, interpolations in the form of `"{$var}"` are resolved to a local `$var`.
///
/// ```
/// # use zero_ui_core::{l10n::*, var::*};
/// # let _scope = zero_ui_core::app::App::minimal();
/// let name = var("World");
/// let msg = l10n!("file/id.attribute", "Hello {$name}!");
/// ```
///
/// ## Key
///
/// This message key can be just a Fluent identifier, `"id"`, a Fluent attribute identifier can be added `"id.attr"`, and finally
/// a file name can be added `"file/id"`. The key syntax is validated at compile time.
///
/// ### Id
///
/// The only required part of a key is the ID, it must contain at least one character, it must start with an ASCII letter
/// and can be followed by any ASCII alphanumeric, _ and -, `[a-zA-Z][a-zA-Z0-9_-]*`.
///
/// ### Attribute
///
/// An attribute identifier can be suffixed on the id, separated by a `.` followed by an identifier of the same pattern as the
/// id, `.[a-zA-Z][a-zA-Z0-9_-]*`.
///
/// ### File
///
/// An optional file name can be prefixed on the id, separated by a `/`, it can be a single file name, no extension.
///
/// Using the default directory resolver the key `"file/id.attr"` will search the id and attribute in the file `{dir}/{lang}/file.ftl`:
///
/// ```ftl
/// id =
///     .attr = message
/// ```
///
/// And a key `"id.attr"` will be search the file `{dir}/{lang}.ftl`.
///
///
/// # Scrap Template
///
/// The `zero-ui-l10n-scraper` tool can be used to collect all localizable text of Rust code files, it is a text based search that
/// matches this macro name and the two first input literals, avoid renaming this macro to support scrapping, otherwise you will
/// have to declare the template file manually.
///
/// The scrapper also has some support for comments, if the previous code line from a [`l10n!`] call is a comment starting with
/// prefix `l10n-# ` the text the follows is collected, same for a comment in the same line of the [`l10n!`] call. Sections
/// can be declared using `l10n-##` and standalone notes can be added to the top of the template file from anywhere using `l10n-###`.
///
/// ```
/// use zero_ui_core::{l10n::*, var::*};
/// # let _scope = zero_ui_core::app::App::minimal();
///
/// // l10n-### Standalone Note
///
/// // l10n-# Comment for `id`.
/// let msg = l10n!("id", "id message");
///
/// // l10n-# Comment for `id.attr`.
/// let msg = l10n!("id.attr", "attr message");
///
/// // l10n-## Section
///
/// let msg = l10n!("other", "other message"); // l10n-# Comment for `other`.
/// ```
///
/// The example above is scrapped to a `template.ftl` file:
///
/// ```ftl
/// ### Standalone Note
///
/// # Comment for `id`.
/// #
/// # attr:
/// #     Comment for `id.attr`.
/// id = id message
///     .attr = attr message
///
/// ## Section
///
/// # Commend for `other`.
/// other = other message
/// ```
///
/// [Fluent Project]: https://projectfluent.org/fluent/guide/
#[macro_export]
macro_rules! l10n {
    ($message_id:tt, $message:tt $(,)?) => {
        $crate::l10n::__l10n! {
            l10n_path { $crate::l10n }
            message_id { $message_id }
            message { $message }
        }
    };
    ($message_id:tt, $message:tt, $($arg:ident = $arg_expr:expr),* $(,)?) => {
        {
            $(
                let $arg = $arg_expr;
            )*
            $crate::l10n::__l10n! {
                l10n_path { $crate::l10n }
                message_id { $message_id }
                message { $message }
            }
        }
    };
    ($($error:tt)*) => {
        std::compile_error!(r#"expected ("id", "message") or ("id", "msg {$arg}", arg=expr)"#)
    }
}
#[doc(inline)]
pub use l10n;

#[doc(hidden)]
pub use zero_ui_proc_macros::l10n as __l10n;

impl L10N {
    /// Change the localization resources to `source`.
    ///
    /// All active variables and handles will be updated to use the new source.
    pub fn load(source: impl L10nSource) {
        todo!("!!:") // service2, remove load_dir?
    }

    /// Start watching the `dir` for `"dir/{locale}.ftl"` files.
    ///
    /// The [`available_langs`] variable maintains an up-to-date list of locale files found, the files
    /// are only loaded when needed, and also are watched to update automatically.
    ///
    /// [`available_langs`]: Self::available_langs
    pub fn load_dir(&self, dir: impl Into<PathBuf>) {
        L10N_SV.write().load_dir(dir.into());
    }

    /// Available localization files.
    ///
    /// The value maps lang to one or more files, the files can be `{dir}/{lang}.flt` or `{dir}/{lang}/file.flt`.
    ///
    /// Note that this map will include any file in the source dir that has a name that is a valid [`lang!`],
    /// that includes the `template.flt` file and test pseudo-locales such as `qps-ploc.flt`.
    pub fn available_langs(&self) -> ReadOnlyArcVar<Arc<LangMap<HashMap<Txt, PathBuf>>>> {
        L10N_SV.read().available_langs.read_only()
    }

    /// Status of the [`available_langs`] list.
    ///
    /// This will be `NotAvailable` before the first call to [`load_dir`], then it changes to `Loading`, then
    /// `Loaded` or `Error`.
    ///
    /// Note that this is the status of the resource list, not of each individual resource, you
    /// can use [`LangResourceHandle::status`] for that.
    ///
    /// [`available_langs`]: Self::available_langs
    /// [`load_dir`]: Self::load_dir
    pub fn available_langs_status(&self) -> ReadOnlyArcVar<LangResourceStatus> {
        L10N_SV.read().available_langs_status.read_only()
    }

    /// Waits until [`available_langs_status`] is not `Loading`.
    ///
    /// [`available_langs_status`]: Self::available_langs_status
    pub async fn wait_available_langs(&self) {
        // wait potential `load_dir` start.
        task::yield_now().await;

        let status = self.available_langs_status();
        while matches!(status.get(), LangResourceStatus::Loading) {
            status.wait_is_new().await;
        }
    }

    /// Gets a read-write variable that sets the preferred languages for the app scope.
    /// Lang not available are ignored until they become available, the first language in the
    /// vec is the most preferred.
    ///
    /// The value is the same as [`sys_lang`], the variable disconnects from system lang if it is assigned directly.
    ///
    /// Note that the [`LANG_VAR`] is used in message requests, the default value of that
    /// context variable is this one.
    ///
    /// [`sys_lang`]: Self::sys_lang
    pub fn app_lang(&self) -> ArcCowVar<Langs, ArcVar<Langs>> {
        L10N_SV.read().app_lang.clone()
    }

    /// Gets a read-only variable that is the current system language.
    ///
    /// The variable will update when the view-process notifies that the config has changed. Is
    /// empty if the system locale cannot be retrieved.
    pub fn sys_lang(&self) -> ReadOnlyArcVar<Langs> {
        L10N_SV.read().sys_lang.read_only()
    }

    /// Gets a variable that is a localized message in the localization context
    /// where the variable is first used. The variable will update when the contextual language changes.
    ///
    /// If the message has variable arguments they must be provided using [`L10nMessageBuilder::arg`], the
    /// returned variable will also update when the arg variables update.
    ///
    /// Prefer using the [`l10n!`] macro instead of this method, the macro does compile time validation.
    ///
    /// # Params
    ///
    /// * `file`: Name of the resource file, in the default directory layout the file is searched at `{dir}/{lang}/{file}.flt`, if
    ///           empty the file is searched at `{dir}/{lang}.flt`. Only a single file name is valid, no other path components allowed.
    /// * `id`: Message identifier inside the resource file.
    /// * `attribute`: Attribute of the identifier, leave empty to not use an attribute.
    ///
    /// The `id` and `attribute` is only valid if it starts with letter `[a-zA-Z]`, followed by any letters, digits, _ or - `[a-zA-Z0-9_-]*`.
    ///
    /// Panics if any parameter is invalid.
    pub fn message(
        &self,
        file: impl Into<Txt>,
        id: impl Into<Txt>,
        attribute: impl Into<Txt>,
        fallback: impl Into<Txt>,
    ) -> L10nMessageBuilder {
        L10nService::message(file.into(), id.into(), attribute.into(), true, fallback.into())
    }

    /// Function called by `l10n!`.
    #[doc(hidden)]
    pub fn l10n_message(
        &self,
        file: &'static str,
        id: &'static str,
        attribute: &'static str,
        fallback: &'static str,
    ) -> L10nMessageBuilder {
        L10nService::message(
            Txt::from_static(file),
            Txt::from_static(id),
            Txt::from_static(attribute),
            false,
            Txt::from_static(fallback),
        )
    }

    /// Gets a formatted message var localized to a given `lang`.
    ///
    /// The returned variable is read-only and will update when the backing resource changes and when the `args` variables change.
    ///
    /// The lang file resource is lazy loaded and stays in memory only when there are variables alive linked to it, each lang
    /// in the list is matched to available resources if no match is available the `fallback` message is used. The variable
    /// may temporary contain the `fallback` as lang resources are loaded asynchrony.
    pub fn localized_messsage(
        &self,
        lang: impl Into<Langs>,
        file: impl Into<Txt>,
        id: impl Into<Txt>,
        attribute: impl Into<Txt>,
        fallback: impl Into<Txt>,
        args: impl Into<Vec<(Txt, BoxedVar<L10nArgument>)>>,
    ) -> ReadOnlyArcVar<Txt> {
        L10N_SV.write().message_text(
            lang.into(),
            file.into(),
            id.into(),
            attribute.into(),
            true,
            fallback.into(),
            args.into(),
        )
    }

    /// Gets a handle to the lang file resource.
    ///
    /// The resource will be loaded and stay in memory until all clones of the handle are dropped, this
    /// can be used to pre-load resources so that localized messages find it immediately avoiding flashing
    /// the fallback text in the UI.
    ///
    /// If the resource directory or file changes it is auto-reloaded, just like when a message variable
    /// held on the resource does.
    ///
    /// # Params
    ///
    /// * `lang`: Language identifier.
    /// * `file`: Name of the resource file, in the default directory layout the file is searched at `{dir}/{lang}/{file}.flt`, if
    ///           empty the file is searched at `{dir}/{lang}.flt`. Only a single file name is valid, no other path components allowed.
    ///
    /// Panics if the file is invalid.
    pub fn lang_resource(&self, lang: impl Into<Lang>, file: impl Into<Txt>) -> LangResourceHandle {
        L10N_SV.write().lang_resource(lang.into(), file.into(), true)
    }

    /// Gets a handle to all resource files for the `lang` after they load.
    ///
    /// This awaits for the available langs to load, then collect an awaits for all lang files.
    pub async fn wait_lang(&self, lang: impl Into<Lang>) -> LangResourceHandles {
        let lang = lang.into();
        let base = self.lang_resource(lang.clone(), "");
        base.wait().await;

        let mut r = vec![base];
        for (name, _) in self.available_langs().get().get(&lang).into_iter().flatten() {
            r.push(self.lang_resource(lang.clone(), name.clone()));
        }
        for h in &r[1..] {
            h.wait().await;
        }
        LangResourceHandles(r)
    }

    /// Gets a handle to all resource files of the first lang in `langs` that is available and loaded.
    ///
    /// This awaits for the available langs to load, then collect an awaits for all lang files.
    pub async fn wait_first(&self, langs: impl Into<Langs>) -> (Option<Lang>, LangResourceHandles) {
        let langs = langs.into();

        L10N.wait_available_langs().await;

        let available = L10N.available_langs().get();
        for lang in langs.0 {
            if let Some(files) = available.get_exact(&lang) {
                let mut r = Vec::with_capacity(files.len());
                for name in files.keys() {
                    r.push(self.lang_resource(lang.clone(), name.clone()));
                }
                let handle = LangResourceHandles(r);
                handle.wait().await;

                return (Some(lang), handle);
            }
        }

        (None, LangResourceHandles(vec![]))
    }
}

/// <span data-del-macro-root></span> Compile-time validated [`Lang`] value.
///
/// The language is parsed during compile and any errors are emitted as compile time errors.
///
/// # Syntax
///
/// The input can be a single a single string literal with `-` separators, or a single ident with `_` as the separators.
///
/// # Examples
///
/// ```
/// # use zero_ui_core::l10n::lang;
/// let en_us = lang!(en_US);
/// let en = lang!(en);
///
/// assert!(en.matches(&en_us, true, false));
/// assert_eq!(en_us, lang!("en-US"));
/// ```
#[macro_export]
macro_rules! lang {
    ($($tt:tt)+) => {
        {
            let lang: $crate::l10n::unic_langid::LanguageIdentifier = $crate::l10n::__lang!($($tt)+);
            lang
        }
    }
}
#[doc(inline)]
pub use crate::lang;

#[doc(hidden)]
pub use zero_ui_proc_macros::lang as __lang;

#[doc(hidden)]
pub use unic_langid;

/// Represents a localization data source.
///
/// See [`L10N.load`] for more details.
///
/// [`L10N.load`]: L10N::load
pub trait L10nSource: Send + 'static {
    /// Gets a read-only variable with all lang files that the source can provide.
    fn available_langs(&mut self) -> BoxedVar<Arc<LangMap<HashMap<Txt, PathBuf>>>>;
    /// Gets a read-only variable that is the status of the [`available_langs`] value.
    ///
    /// [`available_langs`]: Self::available_langs
    fn available_langs_status(&mut self) -> BoxedVar<LangResourceStatus>;

    /// Gets a read-only variable that provides the fluent resource for the `lang` and `file` if available.
    fn lang_resource(&mut self, lang: Lang, file: Txt) -> BoxedVar<Option<Arc<fluent::FluentResource>>>;
    /// Gets a read-only variable that is the status of the [`lang_resource`] value.
    ///
    /// [`lang_resource`]: Self::lang_resource
    fn lang_resource_status(&mut self, lang: Lang, file: Txt) -> BoxedVar<LangResourceStatus>;
}
