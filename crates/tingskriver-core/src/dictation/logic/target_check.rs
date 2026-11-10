//! May dictation write into the focused field? Decided from what UI
//! Automation says about it, right before the text is inserted.
//!
//! The Windows calls sit behind [`ElementProps`], so the decision runs on
//! fakes in tests. Nothing here reads the field's content.
//!
//! - A password field is refused, and the text is never saved to the history.
//! - A read-only or disabled field is refused.
//! - A property the field does not report (`None`) never blocks: browsers
//!   and Electron apps often leave out read-only, and a field must not be
//!   blocked by mistake. The same goes for a failed UI Automation call.
//! - The control type is not used: terminals, browsers and Electron apps
//!   report many types for a field that takes text.

use super::spoken::FieldId;

/// What UI Automation can tell about an element. `None` = not reported.
pub trait ElementProps {
    fn is_password(&self) -> Option<bool>;
    fn is_enabled(&self) -> Option<bool>;
    /// From the Value pattern's `IsReadOnly`.
    fn is_read_only(&self) -> Option<bool>;
}

/// The properties [`decide`] looks at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FieldProbe {
    pub password: Option<bool>,
    pub enabled: Option<bool>,
    pub read_only: Option<bool>,
}

impl FieldProbe {
    pub fn read(element: &dyn ElementProps) -> Self {
        Self {
            password: element.is_password(),
            enabled: element.is_enabled(),
            read_only: element.is_read_only(),
        }
    }
}

/// The focused field and what is known about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Focus {
    pub id: FieldId,
    pub probe: FieldProbe,
}

/// Why nothing was inserted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Password,
    ReadOnly,
    Disabled,
    /// Focus moved, or the field could not be identified.
    Changed,
}

impl Refusal {
    /// Shown in the app and returned as the insertion error. Never
    /// contains the field's content or the dictated text.
    pub fn message(self) -> &'static str {
        match self {
            Refusal::Password => "Tingskriver dikterer ikke i adgangskodefelter",
            Refusal::ReadOnly => "Feltet er skrivebeskyttet, så teksten blev ikke indsat",
            Refusal::Disabled => "Feltet er slået fra, så teksten blev ikke indsat",
            Refusal::Changed => "Tekstfeltet har skiftet eller kunne ikke identificeres",
        }
    }

    /// Short text for the pill.
    pub fn pill_text(self) -> &'static str {
        match self {
            Refusal::Password => "Ikke i adgangskodefelt",
            Refusal::ReadOnly => "Feltet er skrivebeskyttet",
            Refusal::Disabled => "Feltet er slået fra",
            Refusal::Changed => "Feltet skiftede",
        }
    }

    /// The refusal an insertion error came from, if it was one.
    pub fn from_message(message: &str) -> Option<Self> {
        [
            Refusal::Password,
            Refusal::ReadOnly,
            Refusal::Disabled,
            Refusal::Changed,
        ]
        .into_iter()
        .find(|r| r.message() == message)
    }

    /// Text refused for this reason must not be kept anywhere.
    pub fn keeps_text_out_of_history(self) -> bool {
        self == Refusal::Password
    }
}

/// May text go into a field with these properties?
pub fn decide(probe: &FieldProbe) -> Result<(), Refusal> {
    if probe.password == Some(true) {
        Err(Refusal::Password)
    } else if probe.enabled == Some(false) {
        Err(Refusal::Disabled)
    } else if probe.read_only == Some(true) {
        Err(Refusal::ReadOnly)
    } else {
        Ok(())
    }
}

/// The check right before inserting: the field now in focus must be fine
/// and the same one as when the insertion began (`expected`). The
/// properties are checked first, so a password field that took focus in
/// between is reported as such.
pub fn verify(expected: Option<&FieldId>, current: Option<&Focus>) -> Result<(), Refusal> {
    if let Some(current) = current {
        decide(&current.probe)?;
    }
    super::spoken::require_field(expected, current.map(|f| &f.id)).map_err(|_| Refusal::Changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        password: Option<bool>,
        enabled: Option<bool>,
        read_only: Option<bool>,
    }

    impl ElementProps for Fake {
        fn is_password(&self) -> Option<bool> {
            self.password
        }
        fn is_enabled(&self) -> Option<bool> {
            self.enabled
        }
        fn is_read_only(&self) -> Option<bool> {
            self.read_only
        }
    }

    fn decide_fake(fake: Fake) -> Result<(), Refusal> {
        decide(&FieldProbe::read(&fake))
    }

    fn id(element: i32) -> FieldId {
        FieldId {
            process: "chrome.exe".into(),
            window: 1,
            control: 2,
            element: Some(vec![42, element]),
        }
    }

    fn focus(element: i32, fake: Fake) -> Focus {
        Focus {
            id: id(element),
            probe: FieldProbe::read(&fake),
        }
    }

    #[test]
    fn password_field_is_refused() {
        let fake = Fake {
            password: Some(true),
            enabled: Some(true),
            read_only: Some(false),
        };
        assert_eq!(decide_fake(fake), Err(Refusal::Password));
    }

    #[test]
    fn password_wins_over_other_reasons() {
        let fake = Fake {
            password: Some(true),
            enabled: Some(false),
            read_only: Some(true),
        };
        assert_eq!(decide_fake(fake), Err(Refusal::Password));
    }

    #[test]
    fn read_only_field_is_refused() {
        let fake = Fake {
            password: Some(false),
            enabled: Some(true),
            read_only: Some(true),
        };
        assert_eq!(decide_fake(fake), Err(Refusal::ReadOnly));
    }

    #[test]
    fn disabled_field_is_refused() {
        let fake = Fake {
            password: Some(false),
            enabled: Some(false),
            read_only: Some(false),
        };
        assert_eq!(decide_fake(fake), Err(Refusal::Disabled));
    }

    #[test]
    fn ordinary_field_is_allowed() {
        let fake = Fake {
            password: Some(false),
            enabled: Some(true),
            read_only: Some(false),
        };
        assert_eq!(decide_fake(fake), Ok(()));
    }

    #[test]
    fn field_without_read_only_information_is_allowed() {
        let fake = Fake {
            password: Some(false),
            enabled: Some(true),
            read_only: None,
        };
        assert_eq!(decide_fake(fake), Ok(()));
    }

    #[test]
    fn field_that_reports_nothing_is_allowed() {
        assert_eq!(decide_fake(Fake::default()), Ok(()));
    }

    #[test]
    fn same_field_passes_verification() {
        let current = focus(
            7,
            Fake {
                password: Some(false),
                ..Fake::default()
            },
        );
        assert_eq!(verify(Some(&id(7)), Some(&current)), Ok(()));
    }

    #[test]
    fn focus_moving_to_another_field_is_refused() {
        let current = focus(8, Fake::default());
        assert_eq!(verify(Some(&id(7)), Some(&current)), Err(Refusal::Changed));
    }

    #[test]
    fn unknown_field_is_refused() {
        assert_eq!(verify(Some(&id(7)), None), Err(Refusal::Changed));
        assert_eq!(verify(None, None), Err(Refusal::Changed));
    }

    #[test]
    fn password_field_that_took_focus_is_reported_as_password() {
        let current = focus(
            8,
            Fake {
                password: Some(true),
                ..Fake::default()
            },
        );
        assert_eq!(verify(Some(&id(7)), Some(&current)), Err(Refusal::Password));
    }

    #[test]
    fn refusal_round_trips_through_its_message() {
        for r in [
            Refusal::Password,
            Refusal::ReadOnly,
            Refusal::Disabled,
            Refusal::Changed,
        ] {
            assert_eq!(Refusal::from_message(r.message()), Some(r));
            assert!(!r.message().is_empty() && !r.pill_text().is_empty());
        }
        assert_eq!(Refusal::from_message("Forgrundsvinduet skiftede"), None);
    }

    #[test]
    fn only_password_text_is_kept_out_of_history() {
        assert!(Refusal::Password.keeps_text_out_of_history());
        assert!(!Refusal::ReadOnly.keeps_text_out_of_history());
    }
}
