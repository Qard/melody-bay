use std::{error::Error, thread, time::Duration};

use kira::{AudioManager, AudioManagerSettings, backend::cpal::CpalBackend};
use melody_bay::{
    AudioBuffer, AudioContext, BiquadFilterType, IndexedSequence, IndexedTrack, Instrument, Note,
    SampleEnvelope, TrackId, Velocity, Waveform,
};

const SAMPLE_RATE: u32 = 48_000;

fn main() -> Result<(), Box<dyn Error>> {
    let mut manager = AudioManager::<CpalBackend>::new(AudioManagerSettings::default())?;
    let sequence = build_arrangement()?;
    let timed = sequence.resolve();
    println!(
        "Playing {:?} by {:?}: {} tracks, {:.2}s",
        sequence.metadata().title,
        sequence.metadata().composer,
        sequence.tracks().len(),
        timed.duration_seconds()
    );
    let handle = manager.play(timed.sound_data().sample_rate(SAMPLE_RATE))?;
    thread::sleep(Duration::from_secs_f64(timed.duration_seconds() + 0.4));
    handle.stop();
    Ok(())
}

fn build_arrangement() -> Result<IndexedSequence, Box<dyn Error>> {
    let mut sequence = IndexedSequence::new(4)
        .title("Curated Sequencer Arrangement")
        .composer("melody-bay examples");
    sequence.tempo_at(0, 132.0);
    sequence.tempo_at(16, 96.0);
    sequence.tempo_at(24, 144.0);

    sequence.add_track(
        TrackId::named("lead.graph"),
        IndexedTrack::new(Instrument::graph(lead_graph()?))
            .note_with_velocity(0, Note::from_midi(60), 3, Velocity::new(0.55))
            .note_with_velocity(4, Note::from_midi(64), 3, Velocity::new(0.75))
            .note_with_velocity(8, Note::from_midi(67), 4, Velocity::new(0.85))
            .note_with_velocity(16, Note::from_midi(72), 6, Velocity::new(0.95))
            .automation_at(0, "voice.gain", 0.18)
            .linear_ramp_to_value_at_index(16, "voice.gain", 0.11)
            .value_curve_at_index(20, "filter.frequency", [700.0, 1_800.0, 900.0], 8),
    );

    sequence.add_track(
        TrackId::named("pad.graph"),
        IndexedTrack::new(Instrument::graph(pad_graph()?))
            .note_with_velocity(0, Note::from_midi(48), 16, Velocity::new(0.5))
            .note_with_velocity(16, Note::from_midi(53), 12, Velocity::new(0.55))
            .linear_ramp_to_value_at_index(12, "pad.gain", 0.08)
            .linear_ramp_to_value_at_index(28, "pad.gain", 0.0),
    );

    sequence.add_track(
        TrackId::named("sample.kick"),
        IndexedTrack::new(kick_instrument()?)
            .note_with_velocity(0, Note::from_midi(36), 1, Velocity::new(0.95))
            .note_with_velocity(8, Note::from_midi(36), 1, Velocity::new(0.8))
            .note_with_velocity(16, Note::from_midi(36), 1, Velocity::new(0.95))
            .note_with_velocity(24, Note::from_midi(36), 1, Velocity::new(0.8)),
    );

    Ok(sequence)
}

fn lead_graph() -> Result<AudioContext, Box<dyn Error>> {
    let mut graph = AudioContext::try_new_with_sample_rate(SAMPLE_RATE)?;
    let osc = graph.create_oscillator();
    osc.set_type(Waveform::Sawtooth);
    let filter = graph.create_biquad_filter();
    filter.set_type(BiquadFilterType::Lowpass);
    filter.frequency().set_value(1_200.0)?;
    filter.q().set_value(0.8)?;
    let gain = graph.create_gain();
    gain.gain().set_value(0.16)?;
    graph.label_node(&gain, "voice")?;
    graph.label_node(&filter, "filter")?;
    graph.connect(osc, &filter)?;
    graph.connect(&filter, &gain)?;
    graph.connect(&gain, graph.destination())?;
    Ok(graph)
}

fn pad_graph() -> Result<AudioContext, Box<dyn Error>> {
    let mut graph = AudioContext::try_new_with_sample_rate(SAMPLE_RATE)?;
    let pad_gain = graph.create_gain();
    pad_gain.gain().set_value(0.08)?;
    graph.label_node(&pad_gain, "pad")?;
    for (frequency, detune) in [(220.0, -7.0), (277.18, 0.0), (329.63, 5.0)] {
        let osc = graph.create_oscillator();
        osc.set_type(Waveform::Triangle);
        osc.frequency().set_value(frequency)?;
        osc.detune().set_value(detune)?;
        graph.connect(osc, &pad_gain)?;
    }
    graph.connect(&pad_gain, graph.destination())?;
    Ok(graph)
}

fn kick_instrument() -> Result<Instrument, Box<dyn Error>> {
    let samples = (0..2_400).map(|i| {
        let t = i as f32 / SAMPLE_RATE as f32;
        let pitch = 90.0 - t * 120.0;
        let env = (1.0 - t * 18.0).max(0.0);
        (t * std::f32::consts::TAU * pitch).sin() * env
    });
    let buffer = AudioBuffer::try_from_mono(SAMPLE_RATE, 2_400, samples)?;
    Ok(Instrument::sample(buffer, Note::from_midi(36))
        .volume(0.75)
        .envelope(SampleEnvelope {
            points: vec![(0.0, 1.0), (0.08, 0.0)],
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrangement_resolves_and_renders_audio() {
        let sequence = build_arrangement().unwrap();
        assert_eq!(sequence.tracks().len(), 3);
        let rendered = sequence.resolve().try_render_offline(SAMPLE_RATE).unwrap();
        let left = rendered.channel_data(0).unwrap();
        assert!(left.iter().any(|sample| sample.abs() > 0.001));
    }

    #[test]
    fn arrangement_has_metadata_and_named_tracks() {
        let sequence = build_arrangement().unwrap();
        assert_eq!(
            sequence.metadata().title.as_deref(),
            Some("Curated Sequencer Arrangement")
        );
        assert!(sequence.track(TrackId::named("lead.graph")).is_some());
    }
}
