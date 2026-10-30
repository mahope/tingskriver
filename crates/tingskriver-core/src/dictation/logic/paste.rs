//! Decisions for inserting dictated text via the clipboard.
//!
//! The Windows side (`win::insert_text`) offers the text with delayed
//! rendering: Windows asks Tingskriver for the text (`WM_RENDERFORMAT`) the
//! moment a program reads it. That request is the read notification, so the
//! original clipboard is restored only after the target has the text, and a
//! field that ignores Ctrl+V is detected and gets the text typed instead.

/// Clipboard formats (standard ids) that hold GDI handles, not global memory.
/// Windows synthesizes most of them again from the formats that are saved
/// (CF_BITMAP from CF_DIB, CF_METAFILEPICT from CF_ENHMETAFILE).
const GDI_FORMATS: &[u32] = &[2, 3, 9, 0x80, 0x82, 0x83, 0x8E];
/// CF_ENHMETAFILE: a GDI handle, saved through its bytes.
pub const CF_ENHMETAFILE: u32 = 14;
/// OLE bookkeeping formats hold pointers into the copying process; copying
/// their bytes back would hand other programs dangling data.
const SKIPPED_FORMAT_NAMES: &[&str] = &[
    "DataObject",
    "Ole Private Data",
    "OwnerLink",
    "ObjectLink",
    "Link",
];
/// More than this is not kept in memory; the text is typed instead, so the
/// original clipboard is never lost.
pub const MAX_SAVED_BYTES: usize = 256 * 1024 * 1024;

/// Registered formats that keep an entry out of clipboard history (Win+V),
/// cloud clipboard and clipboard monitors. The value is a DWORD 0.
pub const PRIVACY_FORMATS: &[&str] = &[
    "ExcludeClipboardContentFromMonitorProcessing",
    "CanIncludeInClipboardHistory",
    "CanUploadToCloudClipboard",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatKind {
    /// Global memory: copy the bytes.
    Memory,
    /// Enhanced metafile: copy via its bits.
    EnhMetafile,
    /// Not saved (synthesized again by Windows, or unsafe to copy).
    Skip,
}

/// How to save a clipboard format. `name` is the registered name for ids
/// from 0xC000 up.
pub fn format_kind(format: u32, name: Option<&str>) -> FormatKind {
    if format == CF_ENHMETAFILE {
        return FormatKind::EnhMetafile;
    }
    if GDI_FORMATS.contains(&format) {
        return FormatKind::Skip;
    }
    match name {
        Some(name) if format >= 0xC000 && SKIPPED_FORMAT_NAMES.contains(&name) => FormatKind::Skip,
        _ => FormatKind::Memory,
    }
}

/// Whether the saved clipboard can be restored in full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveCheck {
    Complete,
    /// Use typing instead: the clipboard would not come back unchanged.
    TypeInstead(String),
}

/// `total_bytes` of all saved formats, `failed` formats that should have been
/// saved but could not be read.
pub fn check_saved(total_bytes: usize, failed: &[u32]) -> SaveCheck {
    if total_bytes > MAX_SAVED_BYTES {
        return SaveCheck::TypeInstead(format!(
            "clipboard holds {} MB, more than {} MB",
            total_bytes / (1024 * 1024),
            MAX_SAVED_BYTES / (1024 * 1024)
        ));
    }
    if !failed.is_empty() {
        return SaveCheck::TypeInstead(format!("clipboard formats {failed:?} could not be saved"));
    }
    SaveCheck::Complete
}

/// What was observed while the text was on the clipboard.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Observed {
    /// Something read the text before Ctrl+V was sent (a clipboard monitor
    /// that ignores the exclusion format). Later reads give no notification.
    pub read_before_paste: bool,
    pub ctrl_v_sent: bool,
    /// The target read the text after Ctrl+V.
    pub read_after_paste: bool,
    /// Another program put something on the clipboard in the meantime.
    pub replaced: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The target read the text: restore after the short delay.
    Pasted,
    /// Read before Ctrl+V, so the moment the target reads it is unknown:
    /// restore after the long wait, never type (that would insert twice).
    PastedUnconfirmed,
    /// Nothing read the text: restore at once, then type it.
    Blocked,
    /// Someone else owns the clipboard now: leave it. `typed` when the
    /// target never read our text.
    Replaced { typed: bool },
}

impl Outcome {
    pub fn restore(&self) -> bool {
        matches!(
            self,
            Outcome::Pasted | Outcome::PastedUnconfirmed | Outcome::Blocked
        )
    }

    pub fn type_text(&self) -> bool {
        matches!(self, Outcome::Blocked | Outcome::Replaced { typed: true })
    }
}

/// Decide once the target read the text or the wait timed out.
pub fn outcome(seen: Observed) -> Outcome {
    let read = seen.read_after_paste || seen.read_before_paste;
    if seen.replaced {
        return Outcome::Replaced { typed: !read };
    }
    if !seen.ctrl_v_sent {
        return Outcome::Blocked;
    }
    if seen.read_after_paste {
        Outcome::Pasted
    } else if seen.read_before_paste {
        Outcome::PastedUnconfirmed
    } else {
        Outcome::Blocked
    }
}

/// Windows mandatory integrity levels (the last RID of the label SID).
pub const INTEGRITY_MEDIUM: u32 = 0x2000;
pub const INTEGRITY_HIGH: u32 = 0x3000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetAccess {
    Allowed,
    /// The target runs at a higher integrity level (as administrator):
    /// Windows drops our Ctrl+V and typing (UIPI).
    Elevated,
    /// Its integrity level could not be read; usually also elevated.
    Unknown,
}

/// `target`: `None` when the process or its token could not be opened.
pub fn target_access(own: Option<u32>, target: Option<u32>) -> TargetAccess {
    match (own, target) {
        (Some(own), Some(target)) if target > own => TargetAccess::Elevated,
        (_, Some(_)) => TargetAccess::Allowed,
        (_, None) => TargetAccess::Unknown,
    }
}

/// What to do when nothing read the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhenBlocked {
    /// Type it with `SendInput`.
    Type,
    /// Typing would most likely be dropped silently too: tell the user.
    Report,
}

pub fn when_blocked(access: TargetAccess) -> WhenBlocked {
    match access {
        TargetAccess::Allowed => WhenBlocked::Type,
        TargetAccess::Elevated | TargetAccess::Unknown => WhenBlocked::Report,
    }
}

/// Message for the UI when the target program runs as administrator.
pub fn elevated_message(app: Option<&str>) -> String {
    let program = app
        .map(|a| format!("«{a}»"))
        .unwrap_or_else(|| "Programmet".to_string());
    format!(
        "{program} kører som administrator, og Windows lader ikke Tingskriver skrive i det. \
         Kør programmet uden administrator, eller kopiér teksten fra dikteringshistorikken."
    )
}

/// Message when nothing read the text and the target's level is unknown.
pub fn not_received_message(app: Option<&str>) -> String {
    let program = app
        .map(|a| format!("«{a}»"))
        .unwrap_or_else(|| "Programmet".to_string());
    format!(
        "{program} tog ikke imod teksten. Kører det som administrator? \
         Teksten står i dikteringshistorikken."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_to_save() {
        assert_eq!(format_kind(13, None), FormatKind::Memory); // CF_UNICODETEXT
        assert_eq!(format_kind(8, None), FormatKind::Memory); // CF_DIB
        assert_eq!(format_kind(17, None), FormatKind::Memory); // CF_DIBV5
        assert_eq!(format_kind(15, None), FormatKind::Memory); // CF_HDROP
        assert_eq!(format_kind(CF_ENHMETAFILE, None), FormatKind::EnhMetafile);
        assert_eq!(format_kind(2, None), FormatKind::Skip); // CF_BITMAP
        assert_eq!(format_kind(3, None), FormatKind::Skip); // CF_METAFILEPICT
        assert_eq!(format_kind(0xC123, Some("PNG")), FormatKind::Memory);
        assert_eq!(format_kind(0xC124, Some("HTML Format")), FormatKind::Memory);
        assert_eq!(
            format_kind(0xC125, Some("Ole Private Data")),
            FormatKind::Skip
        );
        assert_eq!(format_kind(0xC126, Some("DataObject")), FormatKind::Skip);
        assert_eq!(format_kind(0xC127, None), FormatKind::Memory);
    }

    #[test]
    fn large_or_partial_clipboards_are_typed_instead() {
        assert_eq!(check_saved(0, &[]), SaveCheck::Complete);
        // A 4K screenshot as DIB + DIBV5 + PNG is about 80 MB.
        assert_eq!(check_saved(80 * 1024 * 1024, &[]), SaveCheck::Complete);
        assert!(matches!(
            check_saved(MAX_SAVED_BYTES + 1, &[]),
            SaveCheck::TypeInstead(_)
        ));
        assert!(matches!(check_saved(10, &[8]), SaveCheck::TypeInstead(_)));
    }

    fn seen(before: bool, sent: bool, after: bool, replaced: bool) -> Observed {
        Observed {
            read_before_paste: before,
            ctrl_v_sent: sent,
            read_after_paste: after,
            replaced,
        }
    }

    #[test]
    fn read_by_target_restores_and_does_not_type() {
        let o = outcome(seen(false, true, true, false));
        assert_eq!(o, Outcome::Pasted);
        assert!(o.restore() && !o.type_text());
    }

    #[test]
    fn field_blocking_paste_gets_typed_text() {
        let o = outcome(seen(false, true, false, false));
        assert_eq!(o, Outcome::Blocked);
        assert!(o.restore() && o.type_text());
        // Ctrl+V could not even be sent.
        assert_eq!(outcome(seen(false, false, false, false)), Outcome::Blocked);
    }

    #[test]
    fn early_reader_never_causes_double_text() {
        let o = outcome(seen(true, true, false, false));
        assert_eq!(o, Outcome::PastedUnconfirmed);
        assert!(o.restore() && !o.type_text());
    }

    #[test]
    fn replaced_clipboard_is_left_alone() {
        let pasted = outcome(seen(false, true, true, true));
        assert_eq!(pasted, Outcome::Replaced { typed: false });
        assert!(!pasted.restore() && !pasted.type_text());
        let unread = outcome(seen(false, true, false, true));
        assert!(!unread.restore() && unread.type_text());
    }

    #[test]
    fn elevated_targets_are_detected() {
        assert_eq!(
            target_access(Some(INTEGRITY_MEDIUM), Some(INTEGRITY_HIGH)),
            TargetAccess::Elevated
        );
        assert_eq!(
            target_access(Some(INTEGRITY_MEDIUM), Some(INTEGRITY_MEDIUM)),
            TargetAccess::Allowed
        );
        assert_eq!(
            target_access(Some(INTEGRITY_HIGH), Some(INTEGRITY_HIGH)),
            TargetAccess::Allowed
        );
        assert_eq!(
            target_access(Some(INTEGRITY_MEDIUM), Some(0x1000)),
            TargetAccess::Allowed
        );
        assert_eq!(
            target_access(None, Some(INTEGRITY_HIGH)),
            TargetAccess::Allowed
        );
        assert_eq!(
            target_access(Some(INTEGRITY_MEDIUM), None),
            TargetAccess::Unknown
        );
    }

    #[test]
    fn blocked_paste_types_only_when_input_can_arrive() {
        assert_eq!(when_blocked(TargetAccess::Allowed), WhenBlocked::Type);
        assert_eq!(when_blocked(TargetAccess::Unknown), WhenBlocked::Report);
        assert_eq!(when_blocked(TargetAccess::Elevated), WhenBlocked::Report);
    }

    #[test]
    fn messages_name_the_program() {
        assert!(elevated_message(Some("regedit.exe"))
            .starts_with("«regedit.exe» kører som administrator"));
        assert!(elevated_message(None).starts_with("Programmet kører"));
        assert!(not_received_message(None).contains("administrator"));
    }
}
