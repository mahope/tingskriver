//! The shortcuts registered with `RegisterHotKey`, changed while running.
//!
//! [`Hotkeys::apply`] takes the wanted set, checks it with
//! `tingskriver_core::shortcuts::check`, leaves unchanged shortcuts alone,
//! unregisters the changed ones and registers the new ones. A shortcut
//! another program holds is reported as `TakenByOtherProgram`.

use std::collections::BTreeMap;

use tingskriver_core::dictation::logic::hotkey::Chord;
use tingskriver_core::shortcuts::{
    self, Conflict, ConflictKind, Registration, ShortcutAction, ShortcutSettings,
};

/// Why `RegisterHotKey` failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegisterError {
    /// Another program (or another Tingskriver) has it.
    Taken,
    Other(String),
}

/// `RegisterHotKey` / `UnregisterHotKey`, or a fake in tests.
pub trait Registrar {
    fn register(&mut self, id: i32, registration: Registration) -> Result<(), RegisterError>;
    fn unregister(&mut self, id: i32);
}

pub struct Hotkeys<R: Registrar> {
    registrar: R,
    active: BTreeMap<ShortcutAction, Chord>,
}

impl<R: Registrar> Hotkeys<R> {
    pub fn new(registrar: R) -> Self {
        Hotkeys {
            registrar,
            active: BTreeMap::new(),
        }
    }

    /// The action of a `WM_HOTKEY` id, if it is registered.
    pub fn action(&self, id: i32) -> Option<ShortcutAction> {
        ShortcutAction::from_id(id).filter(|a| self.active.contains_key(a))
    }

    /// The registered shortcuts.
    pub fn active(&self) -> &BTreeMap<ShortcutAction, Chord> {
        &self.active
    }

    /// Register `settings`. `dictation` is the dictation shortcut (for the
    /// conflict check only), `None` when dictation is off. Returns every
    /// problem; shortcuts with a blocking problem are not registered.
    pub fn apply(&mut self, dictation: Option<&str>, settings: &ShortcutSettings) -> Vec<Conflict> {
        let mut problems = shortcuts::check(dictation, settings);
        let mut wanted = BTreeMap::new();
        for action in ShortcutAction::REGISTERED {
            let Some(chord) = settings
                .get(action)
                .and_then(|text| Chord::parse(text).ok())
            else {
                continue;
            };
            if problems.iter().any(|p| p.action == action && p.blocks()) {
                continue;
            }
            wanted.insert(action, chord);
        }

        // Unregister first, so two shortcuts can swap keys.
        let stale: Vec<ShortcutAction> = self
            .active
            .iter()
            .filter(|(action, chord)| wanted.get(action) != Some(chord))
            .map(|(action, _)| *action)
            .collect();
        for action in stale {
            self.registrar.unregister(action.id());
            self.active.remove(&action);
        }

        for (action, chord) in wanted {
            if self.active.contains_key(&action) {
                continue;
            }
            let registration = match shortcuts::registration(chord) {
                Ok(r) => r,
                Err(reason) => {
                    problems.push(Conflict {
                        action,
                        shortcut: chord.label(),
                        kind: ConflictKind::Invalid(reason),
                    });
                    continue;
                }
            };
            match self.registrar.register(action.id(), registration) {
                Ok(()) => {
                    log::info!(
                        "Shortcut for {} registered: {}",
                        action.label(),
                        chord.label()
                    );
                    self.active.insert(action, chord);
                }
                Err(RegisterError::Taken) => {
                    log::warn!(
                        "Shortcut for {} is taken by another program: {}",
                        action.label(),
                        chord.label()
                    );
                    problems.push(Conflict {
                        action,
                        shortcut: chord.label(),
                        kind: ConflictKind::TakenByOtherProgram,
                    });
                }
                Err(RegisterError::Other(reason)) => {
                    log::warn!(
                        "Shortcut for {} could not be registered: {reason}",
                        action.label()
                    );
                    problems.push(Conflict {
                        action,
                        shortcut: chord.label(),
                        kind: ConflictKind::Invalid(format!(
                            "{} kunne ikke registreres: {reason}",
                            chord.label()
                        )),
                    });
                }
            }
        }
        problems
    }

    /// Unregister everything.
    pub fn clear(&mut self) {
        for action in std::mem::take(&mut self.active).into_keys() {
            self.registrar.unregister(action.id());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::rc::Rc;
    use tingskriver_core::shortcuts::flags;

    /// Fakes the system: a global table of taken (modifiers, key) pairs.
    #[derive(Clone, Default)]
    struct FakeSystem {
        taken_by_others: Rc<RefCell<BTreeSet<(u32, u32)>>>,
        ours: Rc<RefCell<BTreeMap<i32, (u32, u32)>>>,
        calls: Rc<RefCell<Vec<String>>>,
    }

    impl Registrar for FakeSystem {
        fn register(&mut self, id: i32, r: Registration) -> Result<(), RegisterError> {
            let key = (r.modifiers & !flags::MOD_NOREPEAT, r.vk);
            self.calls.borrow_mut().push(format!("register {id}"));
            if self.taken_by_others.borrow().contains(&key)
                || self.ours.borrow().values().any(|k| *k == key)
            {
                return Err(RegisterError::Taken);
            }
            self.ours.borrow_mut().insert(id, key);
            Ok(())
        }

        fn unregister(&mut self, id: i32) {
            self.calls.borrow_mut().push(format!("unregister {id}"));
            self.ours.borrow_mut().remove(&id);
        }
    }

    fn settings(
        new_recording: Option<&str>,
        ask: Option<&str>,
        open_window: Option<&str>,
    ) -> ShortcutSettings {
        ShortcutSettings {
            new_recording: new_recording.map(String::from),
            ask: ask.map(String::from),
            open_window: open_window.map(String::from),
        }
    }

    fn key(text: &str) -> (u32, u32) {
        let r = shortcuts::registration(Chord::parse(text).unwrap()).unwrap();
        (r.modifiers & !flags::MOD_NOREPEAT, r.vk)
    }

    #[test]
    fn registers_and_maps_ids_back() {
        let system = FakeSystem::default();
        let mut hotkeys = Hotkeys::new(system.clone());
        let problems = hotkeys.apply(
            Some("Ctrl+Win"),
            &settings(Some("Ctrl+Alt+N"), None, Some("Ctrl+Alt+T")),
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(
            hotkeys.action(ShortcutAction::NewRecording.id()),
            Some(ShortcutAction::NewRecording)
        );
        assert_eq!(
            hotkeys.action(ShortcutAction::OpenWindow.id()),
            Some(ShortcutAction::OpenWindow)
        );
        assert_eq!(hotkeys.action(ShortcutAction::Ask.id()), None);
        assert_eq!(hotkeys.action(99), None);
        assert_eq!(
            system.ours.borrow().get(&ShortcutAction::NewRecording.id()),
            Some(&key("Ctrl+Alt+N"))
        );
    }

    #[test]
    fn a_changed_shortcut_replaces_the_old_one_without_touching_the_rest() {
        let system = FakeSystem::default();
        let mut hotkeys = Hotkeys::new(system.clone());
        hotkeys.apply(
            None,
            &settings(Some("Ctrl+Alt+N"), None, Some("Ctrl+Alt+T")),
        );
        system.calls.borrow_mut().clear();

        let problems = hotkeys.apply(
            None,
            &settings(Some("Ctrl+Alt+M"), None, Some("Ctrl+Alt+T")),
        );
        assert!(problems.is_empty());
        assert_eq!(*system.calls.borrow(), vec!["unregister 2", "register 2"]);
        assert_eq!(system.ours.borrow().get(&2), Some(&key("Ctrl+Alt+M")));
        // The old key is free again for other programs.
        assert!(!system
            .ours
            .borrow()
            .values()
            .any(|k| *k == key("Ctrl+Alt+N")));

        // Applying the same set again changes nothing.
        system.calls.borrow_mut().clear();
        hotkeys.apply(
            None,
            &settings(Some("Ctrl+Alt+M"), None, Some("Ctrl+Alt+T")),
        );
        assert!(system.calls.borrow().is_empty());
    }

    #[test]
    fn two_shortcuts_can_swap_keys() {
        let system = FakeSystem::default();
        let mut hotkeys = Hotkeys::new(system.clone());
        hotkeys.apply(
            None,
            &settings(Some("Ctrl+Alt+N"), None, Some("Ctrl+Alt+T")),
        );
        let problems = hotkeys.apply(
            None,
            &settings(Some("Ctrl+Alt+T"), None, Some("Ctrl+Alt+N")),
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(system.ours.borrow().get(&2), Some(&key("Ctrl+Alt+T")));
        assert_eq!(system.ours.borrow().get(&4), Some(&key("Ctrl+Alt+N")));
    }

    #[test]
    fn taken_and_blocked_shortcuts_are_reported_and_skipped() {
        let system = FakeSystem::default();
        system
            .taken_by_others
            .borrow_mut()
            .insert(key("Ctrl+Alt+Space"));
        let mut hotkeys = Hotkeys::new(system.clone());
        let problems = hotkeys.apply(
            Some("Ctrl+Win"),
            &settings(Some("Win+L"), Some("Ctrl+Alt+Space"), Some("F9")),
        );
        let kinds: Vec<_> = problems
            .iter()
            .map(|p| (p.action, p.kind.clone()))
            .collect();
        assert!(
            kinds.contains(&(ShortcutAction::Ask, ConflictKind::TakenByOtherProgram)),
            "{kinds:?}"
        );
        assert!(kinds
            .iter()
            .any(|(a, k)| *a == ShortcutAction::NewRecording
                && matches!(k, ConflictKind::Windows(_))));
        // A warning (F9 alone) still registers.
        assert!(kinds.contains(&(ShortcutAction::OpenWindow, ConflictKind::NoModifier)));
        assert_eq!(
            hotkeys.active().keys().copied().collect::<Vec<_>>(),
            vec![ShortcutAction::OpenWindow]
        );
        // Win+L was never handed to Windows.
        assert!(!system.calls.borrow().contains(&"register 2".to_string()));
    }

    #[test]
    fn removing_a_shortcut_unregisters_it_and_clear_removes_all() {
        let system = FakeSystem::default();
        let mut hotkeys = Hotkeys::new(system.clone());
        hotkeys.apply(
            None,
            &settings(Some("Ctrl+Alt+N"), Some("Ctrl+Alt+Space"), None),
        );
        hotkeys.apply(None, &settings(None, Some("Ctrl+Alt+Space"), None));
        assert_eq!(
            system.ours.borrow().keys().copied().collect::<Vec<_>>(),
            vec![3]
        );
        hotkeys.clear();
        assert!(system.ours.borrow().is_empty());
        assert!(hotkeys.active().is_empty());
    }
}
