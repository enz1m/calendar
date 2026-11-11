//! RFC 5545 URI, mailbox and opaque identity admission.

use std::fmt::Write as _;

use enzui_core::random;

// RFC 5545 does not cap URI length; the application bounds retained and exported values.
pub(crate) const MAX_URI_LEN: usize = 2048;

// Imported UIDs are bounded because they become retained deduplication keys.
pub(crate) const MAX_UID_LEN: usize = 255;

// URI values preserve any valid scheme but reject controls, whitespace and oversized content.
pub(crate) fn safe_uri(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_URI_LEN {
        return false;
    }
    if value
        .chars()
        .any(|c| c.is_control() || c == '\0' || c.is_whitespace())
    {
        return false;
    }
    has_valid_scheme(value)
}

fn has_valid_scheme(value: &str) -> bool {
    let Some((scheme, body)) = value.split_once(':') else {
        return false;
    };
    if body.is_empty() {
        return false;
    }
    let mut scheme = scheme.chars();
    scheme.next().is_some_and(|c| c.is_ascii_alphabetic())
        && scheme.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

// Invitees round-trip as bare mailboxes in mailto URIs; other cal-address forms are not
// representable.
pub(crate) fn safe_cal_address(who: &str) -> bool {
    if who.is_empty() || who.len() > MAX_URI_LEN {
        return false;
    }
    if who.chars().any(|c| {
        c.is_control()
            || c == '\0'
            || c.is_whitespace()
            || matches!(c, '<' | '>' | ',' | ';' | ':' | '"' | '\\')
    }) {
        return false;
    }
    let mut parts = who.split('@');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(local), Some(domain), None) => {
            !local.is_empty()
                && !domain.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        }
        _ => false,
    }
}

// UIDs are opaque and preserve their bytes except unsafe control content.
pub(crate) fn safe_uid(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_UID_LEN
        && !value.chars().any(|c| c.is_control() || c == '\0')
}

pub(crate) fn import_omission_warnings(
    safe_title: &str,
    url: bool,
    attendee: bool,
    uid: bool,
) -> Vec<String> {
    let mut warnings = Vec::new();
    for (rejected, field) in [(url, "web link"), (attendee, "attendee"), (uid, "identity")] {
        if rejected {
            warnings.push(crate::l10n::format(
                "Imported \"{safe_title}\" but left out an unsafe {field}.",
                &[
                    ("safe_title", (safe_title).to_owned().into()),
                    ("field", crate::l10n::tr(field).into()),
                ],
            ));
        }
    }
    warnings
}

// Entropy failure leaves UID creation unavailable; predictable fallback identities are forbidden.
pub(crate) fn mint_uid() -> Option<String> {
    let bytes = random::random_array::<16>().ok()?;
    Some(format_uid(bytes))
}

fn format_uid(mut bytes: [u8; 16]) -> String {
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let mut uid = String::with_capacity(55);
    for (i, byte) in bytes.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            uid.push('-');
        }
        let _ = write!(uid, "{byte:02x}");
    }
    uid.push_str("@calendar-app.enzui");
    uid
}
