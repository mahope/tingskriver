use super::State;

pub(super) fn microphone_permission(states: [State; 4]) -> State {
    if states.contains(&State::Missing) {
        State::Missing
    } else if states.iter().all(|state| *state == State::Ok) {
        State::Ok
    } else {
        State::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn microphone_requires_root_and_desktop_consent_at_both_levels() {
        assert_eq!(microphone_permission([State::Ok; 4]), State::Ok);
        for index in 0..4 {
            for state in [State::Missing, State::Unknown, State::NotApplicable] {
                let mut states = [State::Ok; 4];
                states[index] = state;
                assert_eq!(
                    microphone_permission(states),
                    if state == State::Missing {
                        State::Missing
                    } else {
                        State::Unknown
                    }
                );
            }
        }
        for index in 0..4 {
            let mut states = [State::Unknown; 4];
            states[index] = State::Missing;
            assert_eq!(microphone_permission(states), State::Missing);
        }
    }
}
