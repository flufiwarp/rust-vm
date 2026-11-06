// Scratch's legacy MIDI mappings. The frontend receives ordinary one-based
// Scratch instrument/drum numbers, just like the modern Music blocks.
pub(super) fn midi_instrument(value: f64) -> f64 {
    const INSTRUMENTS: [u8; 128] = [
        1, 1, 1, 1, 2, 2, 4, 4, 17, 17, 17, 16, 19, 16, 17, 17, 3, 3, 3, 3, 3, 3, 3, 3, 4, 4, 5, 5,
        5, 5, 5, 5, 6, 6, 6, 6, 6, 6, 6, 6, 8, 8, 8, 8, 8, 7, 8, 19, 8, 8, 8, 8, 15, 15, 15, 19, 9,
        9, 9, 9, 9, 9, 9, 9, 11, 11, 11, 11, 14, 14, 14, 10, 12, 12, 13, 13, 13, 13, 12, 12, 20,
        20, 20, 20, 20, 20, 20, 20, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21,
        4, 4, 4, 4, 17, 14, 8, 10, 17, 17, 18, 19, 1, 1, 1, 1, 21, 21, 21, 21, 21, 21, 21, 21,
    ];
    let program = (value + 0.5).floor();
    if (1.0..=128.0).contains(&program) {
        INSTRUMENTS[program as usize - 1] as f64
    } else {
        21.0
    } // Scratch wraps the invalid mapping's -1 index to its last instrument.
}

pub(super) fn midi_drum(value: f64) -> f64 {
    const DRUMS: [u8; 47] = [
        1, 1, 2, 0, 7, 0, 1, 5, 1, 5, 1, 4, 1, 1, 3, 1, 4, 3, 4, 6, 3, 10, 3, 16, 4, 12, 12, 13,
        13, 13, 12, 12, 10, 10, 14, 14, 17, 17, 15, 15, 8, 9, 9, 17, 17, 11, 11,
    ];
    let note = (value + 0.5).floor();
    if (35.0..=81.0).contains(&note) {
        DRUMS[note as usize - 35] as f64 + 1.0
    } else {
        3.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midi_mappings_preserve_scratch_rounding_and_fallbacks() {
        for (input, expected) in [
            (1.0, 1.0),
            (9.0, 17.0),
            (128.0, 21.0),
            (8.5, 17.0),
            (0.0, 21.0),
            (129.0, 21.0),
        ] {
            assert_eq!(midi_instrument(input), expected);
        }
        for (input, expected) in [
            (35.0, 2.0),
            (38.0, 1.0),
            (81.0, 12.0),
            (37.5, 1.0),
            (34.0, 3.0),
            (82.0, 3.0),
        ] {
            assert_eq!(midi_drum(input), expected);
        }
    }
}
