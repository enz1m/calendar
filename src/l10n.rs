//! Calendar message keys and locale-aware domain formatting.
use enzui_core::localization::{Arguments, Catalog, CatalogBuilder, Locale, Localizer, Value};
use std::collections::HashMap;
use std::sync::LazyLock;

static KEYS: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../assets/locales/keys.json"))
        .expect("embedded message index")
});
static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    let mut builder = CatalogBuilder::new();
    builder
        .add_resource("en-US", include_str!("../assets/locales/en-US.ftl"))
        .expect("embedded English catalog");
    builder
        .add_resource("de", include_str!("../assets/locales/de.ftl"))
        .expect("embedded German catalog");
    builder.build()
});
pub(crate) fn app_localizer() -> Localizer {
    let localizer = for_locales(enzui_core::localization::preferred_locales());
    for category in enzui_core::localization::LocaleCategory::ALL {
        let locale = enzui_core::localization::category_locale_from_env(category, |key| {
            std::env::var(key).ok()
        });
        let _ = localizer.set_category_locale(category, locale);
    }
    localizer
}

pub(crate) fn for_locales(locales: Vec<Locale>) -> Localizer {
    Localizer::new(
        CATALOG.clone(),
        locales,
        "en-US".parse().expect("English locale"),
    )
}
pub(crate) fn localizer() -> Localizer {
    enzui_core::localization::scoped().unwrap_or_else(app_localizer)
}
pub(crate) fn tr(source: impl AsRef<str>) -> String {
    let source = source.as_ref();
    KEYS.get(source)
        .map_or_else(|| source.to_owned(), |key| localizer().text(key))
}
pub(crate) fn format(source: &str, values: &[(&str, Value<'_>)]) -> String {
    let mut arguments = Arguments::new();
    for (name, value) in values {
        arguments.set(*name, value.clone());
    }
    let key = KEYS.get(source).map_or(source, String::as_str);
    localizer().text_with(key, &arguments)
}
pub(crate) fn date(date: enzui_core::civil_date::Date, full: bool) -> String {
    use enzui_core::localization::DateStyle;
    localizer()
        .locale_for(enzui_core::localization::LocaleCategory::Time)
        .format_date(
            date,
            if full {
                DateStyle::Full
            } else {
                DateStyle::Medium
            },
        )
        .unwrap_or_else(|_| date.to_string())
}

pub(crate) fn count(key: &str, count: usize) -> String {
    format(key, &[("count", (count as i64).into())])
}
pub(crate) fn rtl() -> bool {
    localizer().locale().direction() == enzui_core::placement::Direction::RightToLeft
}

pub(crate) fn number(value: u32) -> String {
    use enzui_core::calendar_semantics::CalendarLabels;
    localizer().scope(|| enzui_core::localization::LocalizedCalendarLabels.day_number(value))
}
