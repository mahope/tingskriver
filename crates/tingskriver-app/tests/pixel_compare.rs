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
