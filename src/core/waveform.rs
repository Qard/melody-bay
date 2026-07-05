#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waveform {
    Sine,
    Square,
    Sawtooth,
    Triangle,
}

fn oscillator_sample_phase(waveform: Waveform, phase: f32) -> f32 {
    let phase = phase.rem_euclid(1.0);
    match waveform {
        Waveform::Sine => (phase * TAU).sin(),
        Waveform::Square => {
            if phase < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        Waveform::Sawtooth => phase * 2.0 - 1.0,
        Waveform::Triangle => 1.0 - (phase * 4.0 - 1.0).abs(),
    }
}

