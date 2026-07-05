use std::{convert::Infallible, error::Error, thread, time::Duration};

use kira::{
    AudioManager, AudioManagerSettings, Frame,
    backend::cpal::CpalBackend,
    info::Info,
    sound::{Sound, SoundData},
};
use melody_bay::{AnalyserNode, AudioContext, BiquadFilterType, StereoPannerOptions};

const SAMPLE_RATE: u32 = 48_000;

struct ProceduralPhrase {
    notes: Vec<(f32, f64)>,
}

impl SoundData for ProceduralPhrase {
    type Error = Infallible;
    type Handle = ();

    fn into_sound(self) -> Result<(Box<dyn Sound>, Self::Handle), Self::Error> {
        Ok((
            Box::new(ProceduralPhraseSound {
                notes: self.notes,
                note_index: 0,
                note_elapsed: 0.0,
                phase: 0.0,
            }),
            (),
        ))
    }
}

struct ProceduralPhraseSound {
    notes: Vec<(f32, f64)>,
    note_index: usize,
    note_elapsed: f64,
    phase: f32,
}

impl Sound for ProceduralPhraseSound {
    fn process(&mut self, out: &mut [Frame], dt: f64, _info: &Info) {
        let sample_dt = if dt.is_finite() && dt > 0.0 {
            dt
        } else {
            1.0 / SAMPLE_RATE as f64
        };
        for frame in out {
            let Some((frequency, duration)) = self.notes.get(self.note_index).copied() else {
                *frame = Frame::ZERO;
                continue;
            };
            let attack = (self.note_elapsed / 0.02).min(1.0) as f32;
            let release = ((duration - self.note_elapsed) / 0.08).clamp(0.0, 1.0) as f32;
            let envelope = attack.min(release);
            let sample = self.phase.sin() * envelope * 0.16;
            *frame = Frame::from_mono(sample);
            self.phase = (self.phase + std::f32::consts::TAU * frequency * sample_dt as f32)
                .rem_euclid(std::f32::consts::TAU);
            self.note_elapsed += sample_dt;
            if self.note_elapsed >= duration {
                self.note_elapsed = 0.0;
                self.note_index += 1;
            }
        }
    }

    fn finished(&self) -> bool {
        self.note_index >= self.notes.len()
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut manager = AudioManager::<CpalBackend>::new(AudioManagerSettings::default())?;
    let mut context = AudioContext::try_new_with_sample_rate(SAMPLE_RATE)?;
    let analyser = build_bridge(&mut context)?;
    let handle = manager.play(context.sound_data().sample_rate(SAMPLE_RATE))?;
    for tick in 0..6 {
        thread::sleep(Duration::from_millis(500));
        println!(
            "t={:.1}s bridge peak={:.3} rms={:.3}",
            (tick + 1) as f32 * 0.5,
            analyser.peak(),
            analyser.rms()
        );
    }
    handle.stop();
    Ok(())
}

fn build_bridge(context: &mut AudioContext) -> Result<AnalyserNode, Box<dyn Error>> {
    let source = context.create_sound_data_source(ProceduralPhrase {
        notes: vec![(261.63, 0.35), (329.63, 0.35), (392.0, 0.5), (523.25, 0.7)],
    });
    source.try_start(0.0)?;
    source.try_stop(2.2)?;

    let filter = context.create_biquad_filter();
    filter.set_type(BiquadFilterType::Lowpass);
    filter.frequency().set_value_at_time(700.0, 0.0)?;
    filter
        .frequency()
        .linear_ramp_to_value_at_time(2_200.0, 1.6)?;
    let pan = context.try_create_stereo_panner_with_options(StereoPannerOptions { pan: -0.3 })?;
    pan.pan().linear_ramp_to_value_at_time(0.3, 2.0)?;
    let analyser = context.create_analyser();
    let output = context.create_gain();
    output.gain().set_value(0.65)?;

    context.connect(&source, &filter)?;
    context.connect(&filter, &pan)?;
    context.connect(&pan, &output)?;
    context.connect(&output, &analyser)?;
    context.connect(&analyser, context.destination())?;
    Ok(analyser)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kira::{info::MockInfoBuilder, sound::SoundData};

    #[test]
    fn bridge_sound_data_outputs_audio() {
        let data = ProceduralPhrase {
            notes: vec![(220.0, 0.2), (330.0, 0.2)],
        };
        let (mut sound, _) = data.into_sound().unwrap();
        let info = MockInfoBuilder::new().build();
        let mut out = vec![Frame::ZERO; 4_800];
        sound.process(&mut out, 1.0 / SAMPLE_RATE as f64, &info);
        assert!(out.iter().any(|frame| frame.left.abs() > 0.001));
    }

    #[test]
    fn bridge_graph_exposes_analysis() {
        let mut context = AudioContext::try_new_with_sample_rate(SAMPLE_RATE).unwrap();
        let analyser = build_bridge(&mut context).unwrap();
        assert!(analyser.frequency_bin_count() > 0);
    }
}
