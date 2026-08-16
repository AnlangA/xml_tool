//! Runtime localization with Fluent.
//!
//! Both locales are compiled into the binary; the active one is a pure
//! preference switch (no restart, no reload). English is the fallback for
//! missing keys — enforced structurally by the key-parity test in
//! `tests/ui_shell_tests.rs`, which fails the build when the two `.ftl`
//! files drift apart.

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource};
use unic_langid::LanguageIdentifier;

const EN_US: &str = include_str!("../../assets/i18n/en-US.ftl");
const ZH_CN: &str = include_str!("../../assets/i18n/zh-CN.ftl");

/// Available UI languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    English,
    Chinese,
}

impl Language {
    /// The BCP-47 tag of this language.
    pub fn tag(self) -> &'static str {
        match self {
            Language::English => "en-US",
            Language::Chinese => "zh-CN",
        }
    }
}

/// Compiled message bundles for both locales plus the active selection.
pub struct Localization {
    english: FluentBundle<FluentResource>,
    chinese: FluentBundle<FluentResource>,
    active: Language,
}

impl Default for Localization {
    fn default() -> Self {
        Localization::new()
    }
}

impl Localization {
    /// Loads both locales and picks the initial language from the system.
    pub fn new() -> Localization {
        Localization::with_language(system_language())
    }

    /// Loads both locales with an explicit active language.
    pub fn with_language(active: Language) -> Localization {
        Localization {
            english: build_bundle(EN_US),
            chinese: build_bundle(ZH_CN),
            active,
        }
    }

    /// The active language.
    pub fn language(&self) -> Language {
        self.active
    }

    /// Switches the active language; takes effect on the next frame.
    pub fn set_language(&mut self, language: Language) {
        self.active = language;
    }

    /// Translates `key` with no arguments, falling back to English and
    /// finally to the key itself (visible in UI, caught by tests).
    pub fn msg(&self, key: &str) -> String {
        self.msg_with(key, None)
    }

    /// Translates `key` with Fluent arguments.
    pub fn msg_with(&self, key: &str, args: Option<&FluentArgs>) -> String {
        for bundle in self.bundles_in_preference_order() {
            if let Some(message) = bundle.get_message(key)
                && let Some(value) = message.value()
            {
                let mut errors = Vec::new();
                let text = bundle.format_pattern(value, args, &mut errors);
                if errors.is_empty() || !text.is_empty() {
                    return text.into_owned();
                }
            }
        }
        format!("⚠ {key}")
    }

    fn bundles_in_preference_order(&self) -> [&FluentBundle<FluentResource>; 2] {
        match self.active {
            Language::English => [&self.english, &self.chinese],
            Language::Chinese => [&self.chinese, &self.english],
        }
    }

    /// All message ids of the given locale (for parity tests). Keys are
    /// `identifier =` at line starts per Fluent syntax; comments skipped.
    pub fn keys_of(language: Language) -> Vec<String> {
        let text = match language {
            Language::English => EN_US,
            Language::Chinese => ZH_CN,
        };
        let mut keys = Vec::new();
        for line in text.lines() {
            if line.starts_with('#') || line.starts_with(" ") || line.starts_with("\t") {
                continue;
            }
            let Some((id, rest)) = line.split_once(' ') else {
                continue;
            };
            if rest.trim_start().starts_with('=')
                && !id.is_empty()
                && id
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
            {
                keys.push(id.to_string());
            }
        }
        keys
    }
}

fn build_bundle(source: &str) -> FluentBundle<FluentResource> {
    let resource =
        FluentResource::try_new(source.to_string()).expect("bundled .ftl files must parse");
    let langid: LanguageIdentifier = "en-US".parse().expect("valid langid");
    let mut bundle = FluentBundle::new(vec![langid]);
    bundle
        .add_resource(resource)
        .expect("bundled .ftl files must load without conflicts");
    bundle
}

/// Detects the system language once at startup; anything not starting with
/// `zh` maps to English.
pub fn system_language() -> Language {
    let tags = [
        sys_locale::get_locale(),
        std::env::var("LANG").ok(),
        std::env::var("LC_ALL").ok(),
    ];
    for tag in tags.into_iter().flatten() {
        if tag.to_lowercase().starts_with("zh") {
            return Language::Chinese;
        }
    }
    Language::English
}

/// Convenience: builds FluentArgs from pairs.
#[macro_export]
macro_rules! fluent_args {
    ($($key:literal => $value:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut args = fluent_bundle::FluentArgs::new();
        $( args.set($key, $value); )*
        args
    }};
}

/// The canonical en-US langid (re-exported for tests).
pub fn en_us_langid() -> LanguageIdentifier {
    "en-US".parse().expect("valid langid")
}
