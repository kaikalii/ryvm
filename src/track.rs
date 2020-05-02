#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    pub freq: f32,
    pub dur: f32,
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

const C4: f32 = 261.63;

impl Letter {
    pub fn freq(self, octave: u8) -> f32 {
        // *NOTES.get(&(self, octave)).unwrap()
        C4 * 2_f32.powf(f32::from(self.to_u8(octave)) / 12.0 - 4.0)
    }
    #[allow(dead_code)]
    pub fn to_u8(self, octave: u8) -> u8 {
        (octave + 1) * 12
            + match self {
                Letter::C => 0,
                Letter::Csh | Letter::Db => 1,
                Letter::D => 2,
                Letter::Dsh | Letter::Eb => 3,
                Letter::E => 4,
                Letter::F => 5,
                Letter::Fsh | Letter::Gb => 6,
                Letter::G => 7,
                Letter::Gsh | Letter::Ab => 8,
                Letter::A => 9,
                Letter::Ash | Letter::Bb => 10,
                Letter::B => 11,
            }
    }
    pub fn from_u8(u: u8) -> (Letter, u8) {
        static LETTERS: &[Letter] = &[
            Letter::C,
            Letter::Csh,
            Letter::D,
            Letter::Dsh,
            Letter::E,
            Letter::F,
            Letter::Fsh,
            Letter::G,
            Letter::Gsh,
            Letter::A,
            Letter::Ash,
            Letter::B,
        ];
        (LETTERS[(u % 12) as usize], (u / 12).max(1) - 1)
    }
}
