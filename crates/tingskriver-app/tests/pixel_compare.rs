pub fn valid_viewport(actual: (u32, u32), expected: (u32, u32), minimum: (u32, u32)) -> bool {
    actual == expected && actual.0 >= minimum.0 && actual.1 >= minimum.1
}

pub fn changed_pixels(expected: &[u8], actual: &[u8], tolerance: u8) -> usize {
    assert_eq!(expected.len(), actual.len());
    assert_eq!(expected.len() % 4, 0);
    expected
        .chunks_exact(4)
        .zip(actual.chunks_exact(4))
        .filter(|(a, b)| {
            a.iter()
                .zip(b.iter())
                .any(|(a, b)| a.abs_diff(*b) > tolerance)
        })
        .count()
}

#[cfg(test)]
mod tests {
    #[test]
    fn accepts_supported_viewport_clamped_by_native_desktop() {
        assert!(super::valid_viewport((1028, 749), (1028, 749), (640, 420)));
    }

    #[test]
    fn rejects_uninitialized_and_wrong_size_render_targets() {
        assert!(!super::valid_viewport((1, 1), (1, 1), (640, 420)));
        assert!(!super::valid_viewport((1, 1), (1100, 760), (640, 420)));
        assert!(!super::valid_viewport(
            (2200, 1520),
            (1100, 760),
            (640, 420)
        ));
        assert!(!super::valid_viewport((639, 419), (639, 419), (640, 420)));
        assert!(super::valid_viewport((1100, 760), (1100, 760), (640, 420)));
        assert!(super::valid_viewport(
            (2200, 1520),
            (2200, 1520),
            (1280, 840)
        ));
    }

    #[test]
    fn identical_pixels_pass() {
        assert_eq!(
            super::changed_pixels(&[10, 20, 30, 255], &[10, 20, 30, 255], 2),
            0
        );
    }

    #[test]
    fn antialiasing_noise_is_tolerated_but_changed_color_fails() {
        assert_eq!(
            super::changed_pixels(&[10, 20, 30, 255], &[12, 19, 30, 255], 2),
            0
        );
        assert_eq!(
            super::changed_pixels(&[10, 20, 30, 255], &[40, 20, 30, 255], 2),
            1
        );
    }
}
