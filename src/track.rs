use std::{collections::HashMap, str::FromStr};

use once_cell::sync::Lazy;

use crate::SampleType;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    pub freq: SampleType,
    pub dur: SampleType,
}

impl Note {
    pub fn new(freq: SampleType, dur: SampleType) -> Note {
        Note { freq, dur }
    }
    pub fn letter(letter: Letter, octave: u8, dur: SampleType) -> Note {
        Note {
            freq: freq(letter, octave),
            dur,
        }
    }
}

pub fn freq(letter: Letter, octave: u8) -> SampleType {
    *NOTES.get(&(letter, octave)).unwrap()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Letter {
    C,
    Csh,
    Db,
    D,
    Dsh,
    Eb,
    E,
    F,
    Fsh,
    Gb,
    G,
    Gsh,
    Ab,
    A,
    Ash,
    Bb,
    B,
}

macro_rules! notes {
    ($(($letter:ident, $octave:literal) = $val:literal,)*) => {
        pub static NOTES: Lazy<HashMap<(Letter, u8), SampleType>> = Lazy::new(|| {
            let mut map = HashMap::new();
            $(map.insert((Letter::$letter, $octave), $val);)*
            map
        });

        impl FromStr for Letter {
            type Err = ();
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $(stringify!($letter) => Ok(Letter::$letter)),*,
                    _ => Err(())
                }
            }
        }
    };
}

notes!(
    (C, 0) = 16.35,
    (Csh, 0) = 17.32,
    (Db, 0) = 17.32,
    (D, 0) = 18.35,
    (Dsh, 0) = 19.45,
    (Eb, 0) = 19.45,
    (E, 0) = 20.60,
    (F, 0) = 21.83,
    (Fsh, 0) = 23.12,
    (Gb, 0) = 23.12,
    (G, 0) = 24.50,
    (Gsh, 0) = 25.96,
    (Ab, 0) = 25.96,
    (A, 0) = 27.50,
    (Ash, 0) = 29.14,
    (Bb, 0) = 29.14,
    (B, 0) = 30.87,
    (C, 1) = 32.70,
    (Csh, 1) = 34.65,
    (Db, 1) = 34.65,
    (D, 1) = 36.71,
    (Dsh, 1) = 38.89,
    (Eb, 1) = 38.89,
    (E, 1) = 41.20,
    (F, 1) = 43.65,
    (Fsh, 1) = 46.25,
    (Gb, 1) = 46.25,
    (G, 1) = 49.00,
    (Gsh, 1) = 51.91,
    (Ab, 1) = 51.91,
    (A, 1) = 55.00,
    (Ash, 1) = 58.27,
    (Bb, 1) = 58.27,
    (B, 1) = 61.74,
    (C, 2) = 65.41,
    (Csh, 2) = 69.30,
    (Db, 2) = 69.30,
    (D, 2) = 73.42,
    (Dsh, 2) = 77.78,
    (Eb, 2) = 77.78,
    (E, 2) = 82.41,
    (F, 2) = 87.31,
    (Fsh, 2) = 92.50,
    (Gb, 2) = 92.50,
    (G, 2) = 98.00,
    (Gsh, 2) = 103.83,
    (Ab, 2) = 103.83,
    (A, 2) = 110.00,
    (Ash, 2) = 116.54,
    (Bb, 2) = 116.54,
    (B, 2) = 123.47,
    (C, 3) = 130.81,
    (Csh, 3) = 138.59,
    (Db, 3) = 138.59,
    (D, 3) = 146.83,
    (Dsh, 3) = 155.56,
    (Eb, 3) = 155.56,
    (E, 3) = 164.81,
    (F, 3) = 174.61,
    (Fsh, 3) = 185.00,
    (Gb, 3) = 185.00,
    (G, 3) = 196.00,
    (Gsh, 3) = 207.65,
    (Ab, 3) = 207.65,
    (A, 3) = 220.00,
    (Ash, 3) = 233.08,
    (Bb, 3) = 233.08,
    (B, 3) = 246.94,
    (C, 4) = 261.63,
    (Csh, 4) = 277.18,
    (Db, 4) = 277.18,
    (D, 4) = 293.66,
    (Dsh, 4) = 311.13,
    (Eb, 4) = 311.13,
    (E, 4) = 329.63,
    (F, 4) = 349.23,
    (Fsh, 4) = 369.99,
    (Gb, 4) = 369.99,
    (G, 4) = 392.00,
    (Gsh, 4) = 415.30,
    (Ab, 4) = 415.30,
    (A, 4) = 440.00,
    (Ash, 4) = 466.16,
    (Bb, 4) = 466.16,
    (B, 4) = 493.88,
    (C, 5) = 523.25,
    (Csh, 5) = 554.37,
    (Db, 5) = 554.37,
    (D, 5) = 587.33,
    (Dsh, 5) = 622.25,
    (Eb, 5) = 622.25,
    (E, 5) = 659.25,
    (F, 5) = 698.46,
    (Fsh, 5) = 739.99,
    (Gb, 5) = 739.99,
    (G, 5) = 783.99,
    (Gsh, 5) = 830.61,
    (Ab, 5) = 830.61,
    (A, 5) = 880.00,
    (Ash, 5) = 932.33,
    (Bb, 5) = 932.33,
    (B, 5) = 987.77,
    (C, 6) = 1046.50,
    (Csh, 6) = 1108.73,
    (Db, 6) = 1108.73,
    (D, 6) = 1174.66,
    (Dsh, 6) = 1244.51,
    (Eb, 6) = 1244.51,
    (E, 6) = 1318.51,
    (F, 6) = 1396.91,
    (Fsh, 6) = 1479.98,
    (Gb, 6) = 1479.98,
    (G, 6) = 1567.98,
    (Gsh, 6) = 1661.22,
    (Ab, 6) = 1661.22,
    (A, 6) = 1760.00,
    (Ash, 6) = 1864.66,
    (Bb, 6) = 1864.66,
    (B, 6) = 1975.53,
    (C, 7) = 2093.00,
    (Csh, 7) = 2217.46,
    (Db, 7) = 2217.46,
    (D, 7) = 2349.32,
    (Dsh, 7) = 2489.02,
    (Eb, 7) = 2489.02,
    (E, 7) = 2637.02,
    (F, 7) = 2793.83,
    (Fsh, 7) = 2959.96,
    (Gb, 7) = 2959.96,
    (G, 7) = 3135.96,
    (Gsh, 7) = 3322.44,
    (Ab, 7) = 3322.44,
    (A, 7) = 3520.00,
    (Ash, 7) = 3729.31,
    (Bb, 7) = 3729.31,
    (B, 7) = 3951.07,
    (C, 8) = 4186.01,
    (Csh, 8) = 4434.92,
    (Db, 8) = 4434.92,
    (D, 8) = 4698.63,
    (Dsh, 8) = 4978.03,
    (Eb, 8) = 4978.03,
    (E, 8) = 5274.04,
    (F, 8) = 5587.65,
    (Fsh, 8) = 5919.91,
    (Gb, 8) = 5919.91,
    (G, 8) = 6271.93,
    (Gsh, 8) = 6644.88,
    (Ab, 8) = 6644.88,
    (A, 8) = 7040.00,
    (Ash, 8) = 7458.62,
    (Bb, 8) = 7458.62,
    (B, 8) = 7902.13,
);
