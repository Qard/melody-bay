use kira::{Frame, info::MockInfoBuilder, sound::SoundData};
use melody_bay::{
    AudioBuffer, AudioContext, GraphError, IndexedSequence, IndexedTrack, Instrument, Note,
    SequencerValidationError, TimedSequence, TimedTrack, TrackId, Velocity, Waveform,
};

fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= 0.0001,
        "expected {actual} to be close to {expected}"
    );
}

#[test]
fn note_exposes_midi_frequency_and_name_helpers() {
    let a4 = Note::from_midi(69);
    let c4 = Note::from_midi(60);

    assert_eq!(a4.midi_number(), 69);
    assert_close(a4.frequency(), 440.0);
    assert_eq!(a4.name(), "A4");
    assert_eq!(c4.name(), "C4");
}

#[test]
fn indexed_sequence_resolves_tempo_changes_to_timed_events() {
    let mut sequence = IndexedSequence::new(4);
    sequence.tempo_at(0, 120.0);
    sequence.tempo_at(4, 60.0);
    sequence.add_track(
        TrackId::named("lead"),
        IndexedTrack::new(Instrument::graph(constant_graph(0.5)))
            .note(0, Note::from_midi(60), 4)
            .note(4, Note::from_midi(64), 4),
    );

    let timed = sequence.resolve();
    let track = timed.track(TrackId::named("lead")).expect("lead track");

    assert_close(track.notes()[0].start_seconds as f32, 0.0);
    assert_close(track.notes()[0].duration_seconds as f32, 0.5);
    assert_close(track.notes()[1].start_seconds as f32, 0.5);
    assert_close(track.notes()[1].duration_seconds as f32, 1.0);
}

#[test]
fn ids_and_tempo_maps_expose_inspection_helpers() {
    let track = TrackId::named("lead");
    let mut sequence = IndexedSequence::new(8);
    sequence.tempo_at(0, 120.0);
    sequence.tempo_at(8, 90.0);

    assert_eq!(track.as_str(), "lead");
    assert_eq!(sequence.tempo_map.steps_per_beat(), 8);
    assert_eq!(sequence.tempo_map.tempo_events(), &[(0, 120.0), (8, 90.0)]);
    assert_close(sequence.tempo_map.bpm_at(8) as f32, 90.0);
}

#[test]
fn metadata_builders_and_indexed_accessors_are_public() {
    let track_id = TrackId::named("lead");
    let sequence = IndexedSequence::new(4)
        .title("Sketch")
        .composer("Composer")
        .with_track(
            track_id.clone(),
            IndexedTrack::new(Instrument::graph(constant_graph(0.5)))
                .note_with_velocity(0, Note::from_midi(60), 4, Velocity::new(0.75))
                .value_curve_at_index(0, "gain", [0.0, 1.0], 4),
        );
    let track = sequence.track(track_id.clone()).expect("lead track");

    assert_eq!(sequence.metadata().title.as_deref(), Some("Sketch"));
    assert_eq!(sequence.metadata().composer.as_deref(), Some("Composer"));
    assert!(sequence.tracks().contains_key(&track_id));
    assert_eq!(track.notes()[0].duration_indices, 4);
    assert_eq!(track.automation()[0].index, 0);
}

#[test]
fn timed_sequence_renders_notes_at_exact_sample_positions() {
    let sequence = TimedSequence::new().with_track(
        TrackId::default(),
        TimedTrack::new(Instrument::graph(constant_graph(1.0))).note_at(
            0.25,
            Note::from_midi(69),
            0.25,
            Velocity::MAX,
        ),
    );
    let (mut sound, _) = sequence
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("sequence sound data should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 3];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 1.0);
    assert_close(out[2].left, 0.0);
}

#[test]
fn timed_sequence_explicit_duration_is_a_render_floor() {
    let short_note = TimedSequence::new().with_duration(1.0).with_track(
        TrackId::default(),
        TimedTrack::new(Instrument::graph(constant_graph(1.0))).note_at(
            0.0,
            Note::from_midi(69),
            0.25,
            Velocity::MAX,
        ),
    );
    let automation_only = TimedSequence::new().with_duration(0.5).with_track(
        TrackId::default(),
        TimedTrack::new(Instrument::graph(constant_graph(1.0))),
    );
    let long_note = TimedSequence::new().with_duration(0.25).with_track(
        TrackId::default(),
        TimedTrack::new(Instrument::graph(constant_graph(1.0))).note_at(
            0.0,
            Note::from_midi(69),
            1.0,
            Velocity::MAX,
        ),
    );

    assert_eq!(short_note.render_offline(4).length(), 4);
    assert_eq!(automation_only.render_offline(4).length(), 2);
    assert_eq!(long_note.render_offline(4).length(), 4);
}

#[test]
fn timed_sequence_can_render_and_play_individual_tracks() {
    let lead = TrackId::named("lead");
    let bass = TrackId::named("bass");
    let sequence = TimedSequence::new()
        .with_track(
            lead.clone(),
            TimedTrack::new(Instrument::graph(constant_graph(0.25))).note_at(
                0.0,
                Note::from_midi(69),
                0.25,
                Velocity::MAX,
            ),
        )
        .with_track(
            bass.clone(),
            TimedTrack::new(Instrument::graph(constant_graph(0.75))).note_at(
                0.0,
                Note::from_midi(69),
                0.25,
                Velocity::MAX,
            ),
        );

    let lead_buffer = sequence.render_track_offline(lead.clone(), 4);
    let full_buffer = sequence.render_offline(4);
    assert_close(lead_buffer.channel_data(0).unwrap()[0], 0.25);
    assert_close(full_buffer.channel_data(0).unwrap()[0], 1.0);

    let (mut lead_sound, _) = sequence
        .track_sound_data(lead)
        .sample_rate(4)
        .into_sound()
        .expect("track sound data should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 1];

    lead_sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.25);
}

#[test]
fn graph_instruments_transpose_pitched_oscillators_from_base_note() {
    let mut graph = AudioContext::new();
    let osc = graph.create_oscillator();
    osc.set_type(Waveform::Sine);
    osc.frequency().set_value(1.0).unwrap();
    let gain = graph.create_gain();
    gain.gain().set_value(1.0).unwrap();
    graph.connect(osc, &gain).unwrap();
    graph.connect(&gain, graph.destination()).unwrap();

    let sequence = TimedSequence::new().with_track(
        TrackId::default(),
        TimedTrack::new(Instrument::graph(graph).base_note(Note::from_midi(69))).note_at(
            0.0,
            Note::from_midi(81),
            0.5,
            Velocity::MAX,
        ),
    );
    let (mut sound, _) = sequence
        .sound_data()
        .sample_rate(8)
        .into_sound()
        .expect("sequence sound data should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 3];

    sound.process(&mut out, 0.125, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 1.0);
    assert_close(out[2].left, 0.0);
}

#[test]
fn timed_automation_applies_to_matching_graph_params() {
    let sequence = TimedSequence::new().with_track(
        TrackId::default(),
        TimedTrack::new(Instrument::graph(constant_graph(1.0)))
            .note_at(0.0, Note::from_midi(69), 1.0, Velocity::MAX)
            .automation_at(0.5, "gain", 0.25),
    );
    let (mut sound, _) = sequence
        .sound_data()
        .sample_rate(2)
        .into_sound()
        .expect("sequence sound data should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 2];

    sound.process(&mut out, 0.5, &info);

    assert_close(out[0].left, 1.0);
    assert_close(out[1].left, 0.25);
}

#[test]
fn timed_automation_is_track_global_for_delayed_notes() {
    let sequence = TimedSequence::new().with_track(
        TrackId::default(),
        TimedTrack::new(Instrument::graph(constant_graph(1.0)))
            .automation_at(0.5, "gain", 0.25)
            .note_at(1.0, Note::from_midi(69), 0.5, Velocity::MAX),
    );
    let (mut sound, _) = sequence
        .sound_data()
        .sample_rate(2)
        .into_sound()
        .expect("sequence sound data should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 3];

    sound.process(&mut out, 0.5, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 0.0);
    assert_close(out[2].left, 0.25);
}

#[test]
fn automation_targets_can_select_a_matching_param_by_index() {
    let mut graph = AudioContext::new();
    let source = graph.create_constant_source();
    let first_gain = graph.create_gain();
    let second_gain = graph.create_gain();
    graph.connect(source, &first_gain).unwrap();
    graph.connect(&first_gain, &second_gain).unwrap();
    graph.connect(&second_gain, graph.destination()).unwrap();
    let sequence = TimedSequence::new().with_track(
        TrackId::default(),
        TimedTrack::new(Instrument::graph(graph))
            .note_at(0.0, Note::from_midi(69), 0.25, Velocity::MAX)
            .automation_at(0.0, "gain#1", 0.25),
    );
    let (mut sound, _) = sequence
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("sequence sound data should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.25);
}

#[test]
fn automation_targets_can_select_a_param_by_node_label() {
    let mut graph = AudioContext::new();
    let source = graph.create_constant_source();
    let first_gain = graph.create_gain();
    let second_gain = graph.create_gain();
    graph.label_node(&second_gain, "output").unwrap();
    graph.connect(source, &first_gain).unwrap();
    graph.connect(&first_gain, &second_gain).unwrap();
    graph.connect(&second_gain, graph.destination()).unwrap();
    let sequence = TimedSequence::new().with_track(
        TrackId::default(),
        TimedTrack::new(Instrument::graph(graph))
            .note_at(0.0, Note::from_midi(69), 0.25, Velocity::MAX)
            .automation_at(0.0, "output.gain", 0.25),
    );
    let (mut sound, _) = sequence
        .try_sound_data()
        .expect("labeled automation target should validate")
        .sample_rate(4)
        .into_sound()
        .expect("sequence sound data should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.25);
}

#[test]
fn graph_node_labels_must_be_non_empty_and_unique() {
    let mut graph = AudioContext::new();
    let first_gain = graph.create_gain();
    let second_gain = graph.create_gain();

    assert_eq!(
        graph.label_node(&first_gain, ""),
        Err(GraphError::InvalidNodeLabel)
    );
    graph.label_node(&first_gain, "output").unwrap();
    assert_eq!(
        graph.label_node(&second_gain, "output"),
        Err(GraphError::InvalidNodeLabel)
    );
}

#[test]
fn invalid_automation_targets_are_reported_before_rendering() {
    let bad_name = TimedSequence::new().with_track(
        TrackId::named("lead"),
        TimedTrack::new(Instrument::graph(constant_graph(1.0))).automation_at(
            0.0,
            "missing_param",
            0.5,
        ),
    );
    let bad_index = TimedSequence::new().with_track(
        TrackId::named("lead"),
        TimedTrack::new(Instrument::graph(constant_graph(1.0))).automation_at(0.0, "gain#4", 0.5),
    );

    assert!(matches!(
        bad_name.validate(),
        Err(SequencerValidationError::InvalidAutomationTarget { target, .. })
            if target == "missing_param"
    ));
    assert!(matches!(
        bad_index.try_sound_data(),
        Err(SequencerValidationError::InvalidAutomationTarget { target, .. })
            if target == "gain#4"
    ));
}

#[test]
fn track_specific_try_rendering_validates_only_the_requested_track() {
    let lead = TrackId::named("lead");
    let broken = TrackId::named("broken");
    let sequence = TimedSequence::new()
        .with_track(
            lead.clone(),
            TimedTrack::new(Instrument::graph(constant_graph(0.25))).note_at(
                0.0,
                Note::from_midi(69),
                0.25,
                Velocity::MAX,
            ),
        )
        .with_track(
            broken.clone(),
            TimedTrack::new(Instrument::graph(constant_graph(1.0))).automation_at(
                0.0,
                "missing_param",
                1.0,
            ),
        );

    let lead_buffer = sequence
        .try_render_track_offline(lead.clone(), 4)
        .expect("valid requested track should render");
    assert_close(lead_buffer.channel_data(0).unwrap()[0], 0.25);
    assert!(matches!(
        sequence.try_track_sound_data(broken),
        Err(SequencerValidationError::InvalidAutomationTarget { .. })
    ));
}

#[test]
fn invalid_automation_times_values_and_durations_are_reported_before_rendering() {
    let bad_time = TimedSequence::new().with_track(
        TrackId::named("lead"),
        TimedTrack::new(Instrument::graph(constant_graph(1.0))).automation_at(-0.25, "gain", 0.5),
    );
    let bad_value = TimedSequence::new().with_track(
        TrackId::named("lead"),
        TimedTrack::new(Instrument::graph(constant_graph(1.0))).automation_at(
            0.0,
            "gain",
            f32::NAN,
        ),
    );
    let bad_duration = TimedSequence::new().with_track(
        TrackId::named("lead"),
        TimedTrack::new(Instrument::graph(constant_graph(1.0))).value_curve_at(
            0.0,
            "gain",
            [0.0, 1.0],
            0.0,
        ),
    );

    assert!(matches!(
        bad_time.try_sound_data(),
        Err(SequencerValidationError::InvalidAutomationTime {
            time_seconds,
            ..
        }) if time_seconds == -0.25
    ));
    assert!(matches!(
        bad_value.try_render_offline(4),
        Err(SequencerValidationError::InvalidAutomationValue { value, .. })
            if value.is_nan()
    ));
    assert!(matches!(
        bad_duration.try_track_sound_data(TrackId::named("lead")),
        Err(SequencerValidationError::InvalidAutomationDuration {
            duration_seconds,
            ..
        }) if duration_seconds == 0.0
    ));
}

#[test]
fn timed_automation_supports_linear_ramps() {
    let sequence = TimedSequence::new().with_track(
        TrackId::default(),
        TimedTrack::new(Instrument::graph(constant_graph(1.0)))
            .note_at(0.0, Note::from_midi(69), 1.0, Velocity::MAX)
            .automation_at(0.0, "gain", 0.0)
            .linear_ramp_to_value_at(1.0, "gain", 1.0),
    );
    let (mut sound, _) = sequence
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("sequence sound data should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 3];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 0.25);
    assert_close(out[2].left, 0.5);
}

#[test]
fn timed_automation_supports_value_curves() {
    let sequence = TimedSequence::new().with_track(
        TrackId::default(),
        TimedTrack::new(Instrument::graph(constant_graph(1.0)))
            .note_at(0.0, Note::from_midi(69), 1.0, Velocity::MAX)
            .value_curve_at(0.0, "gain", [0.0, 0.5, 1.0], 1.0),
    );
    let (mut sound, _) = sequence
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("sequence sound data should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 3];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 0.25);
    assert_close(out[2].left, 0.5);
}

#[test]
fn indexed_automation_resolves_through_the_tempo_map() {
    let mut sequence = IndexedSequence::new(2);
    sequence.tempo_at(0, 60.0);
    sequence.add_track(
        TrackId::default(),
        IndexedTrack::new(Instrument::graph(constant_graph(1.0)))
            .note(0, Note::from_midi(69), 4)
            .automation_at(1, "gain", 0.5),
    );
    let timed = sequence.resolve();
    let automation = timed
        .track(TrackId::default())
        .expect("default track")
        .automation();

    assert_close(automation[0].time_seconds as f32, 0.5);

    let (mut sound, _) = timed
        .sound_data()
        .sample_rate(2)
        .into_sound()
        .expect("sequence sound data should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 2];

    sound.process(&mut out, 0.5, &info);

    assert_close(out[0].left, 1.0);
    assert_close(out[1].left, 0.5);
}

#[test]
fn indexed_linear_automation_resolves_to_timed_ramps() {
    let mut sequence = IndexedSequence::new(2);
    sequence.tempo_at(0, 60.0);
    sequence.add_track(
        TrackId::default(),
        IndexedTrack::new(Instrument::graph(constant_graph(1.0)))
            .note(0, Note::from_midi(69), 4)
            .automation_at(0, "gain", 0.0)
            .linear_ramp_to_value_at_index(2, "gain", 1.0),
    );
    let timed = sequence.resolve();
    let automation = timed
        .track(TrackId::default())
        .expect("default track")
        .automation();

    assert_close(automation[1].time_seconds as f32, 1.0);

    let (mut sound, _) = timed
        .sound_data()
        .sample_rate(2)
        .into_sound()
        .expect("sequence sound data should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 2];

    sound.process(&mut out, 0.5, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 0.5);
}

#[test]
fn graph_instruments_transpose_buffer_source_playback_rates_from_base_note() {
    let buffer = AudioBuffer::try_from_mono(3_000, 4, [0.0, 1.0, 0.5, 0.25]).unwrap();
    let mut graph = AudioContext::new();
    let source = graph.create_buffer_source();
    source.try_set_buffer(buffer).unwrap();
    let gain = graph.create_gain();
    graph.connect(source, &gain).unwrap();
    graph.connect(&gain, graph.destination()).unwrap();

    let sequence = TimedSequence::new().with_track(
        TrackId::default(),
        TimedTrack::new(Instrument::graph(graph).base_note(Note::from_midi(69))).note_at(
            0.0,
            Note::from_midi(81),
            2.0 / 3_000.0,
            Velocity::MAX,
        ),
    );
    let (mut sound, _) = sequence
        .sound_data()
        .sample_rate(3_000)
        .into_sound()
        .expect("sequence sound data should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 2];

    sound.process(&mut out, 1.0 / 3_000.0, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 0.5);
}

#[test]
fn graph_instruments_do_not_transpose_modulation_only_sources() {
    let mut graph = AudioContext::new();
    let carrier = graph.create_constant_source();
    let modulator = graph.create_oscillator();
    modulator.set_type(Waveform::Sine);
    modulator.frequency().set_value(1.0).unwrap();
    let gain = graph.create_gain();
    gain.gain().set_value(0.0).unwrap();
    graph.connect(carrier, &gain).unwrap();
    graph.connect_param(modulator, gain.gain()).unwrap();
    graph.connect(&gain, graph.destination()).unwrap();

    let sequence = TimedSequence::new().with_track(
        TrackId::default(),
        TimedTrack::new(Instrument::graph(graph).base_note(Note::from_midi(69))).note_at(
            0.0,
            Note::from_midi(81),
            0.5,
            Velocity::MAX,
        ),
    );
    let (mut sound, _) = sequence
        .sound_data()
        .sample_rate(8)
        .into_sound()
        .expect("sequence sound data should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 2];

    sound.process(&mut out, 0.125, &info);

    assert_close(out[1].left, std::f32::consts::FRAC_1_SQRT_2);
}

#[test]
fn graph_instrument_velocity_is_applied_per_voice() {
    let sequence = TimedSequence::new().with_track(
        TrackId::default(),
        TimedTrack::new(Instrument::graph(constant_graph(1.0)))
            .note_at(0.0, Note::from_midi(69), 0.25, Velocity::new(0.25))
            .note_at(0.25, Note::from_midi(69), 0.25, Velocity::MAX),
    );
    let (mut sound, _) = sequence
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("sequence sound data should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 2];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.25);
    assert_close(out[1].left, 1.0);
}

#[test]
fn sample_instruments_transpose_from_root_note_and_apply_volume() {
    let buffer = AudioBuffer::try_from_mono(3_000, 4, [0.0, 1.0, 0.5, 0.25]).unwrap();
    let sequence = TimedSequence::new().with_track(
        TrackId::default(),
        TimedTrack::new(Instrument::sample(buffer, Note::from_midi(69)).volume(0.5)).note_at(
            0.0,
            Note::from_midi(81),
            2.0 / 3_000.0,
            Velocity::MAX,
        ),
    );
    let (mut sound, _) = sequence
        .sound_data()
        .sample_rate(3_000)
        .into_sound()
        .expect("sequence sound data should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 2];

    sound.process(&mut out, 1.0 / 3_000.0, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 0.25 * std::f32::consts::FRAC_1_SQRT_2);
}

#[test]
fn late_sample_instrument_notes_start_at_sample_beginning() {
    let buffer = AudioBuffer::try_from_mono(3_000, 3, [0.0, 1.0, 0.0]).unwrap();
    let sequence = TimedSequence::new().with_track(
        TrackId::default(),
        TimedTrack::new(Instrument::sample(buffer, Note::from_midi(69))).note_at(
            1.0 / 3_000.0,
            Note::from_midi(69),
            2.0 / 3_000.0,
            Velocity::MAX,
        ),
    );
    let (mut sound, _) = sequence
        .sound_data()
        .sample_rate(3_000)
        .into_sound()
        .expect("sequence sound data should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 3];

    sound.process(&mut out, 1.0 / 3_000.0, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 0.0);
    assert_close(out[2].left, std::f32::consts::FRAC_1_SQRT_2);
}

#[test]
fn sample_instruments_respect_buffer_sample_rate_at_kira_output_rate() {
    let buffer = AudioBuffer::try_from_mono(8_363, 3, [0.0, 1.0, 0.0]).unwrap();
    let sequence = TimedSequence::new().with_track(
        TrackId::default(),
        TimedTrack::new(Instrument::sample(buffer, Note::from_midi(60))).note_at(
            0.0,
            Note::from_midi(60),
            16.0 / 44_100.0,
            Velocity::MAX,
        ),
    );
    let (mut sound, _) = sequence
        .sound_data()
        .sample_rate(44_100)
        .into_sound()
        .expect("sequence sound data should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 16];

    sound.process(&mut out, 1.0 / 44_100.0, &info);

    let (peak_index, peak) = out
        .iter()
        .enumerate()
        .map(|(index, frame)| (index, frame.left.abs()))
        .max_by(|(_, left), (_, right)| left.total_cmp(right))
        .expect("non-empty render");

    assert!(
        (5..=6).contains(&peak_index),
        "expected imported 8363 Hz sample peak around 44100 / 8363 frames, got {peak_index}"
    );
    assert!(peak > 0.55, "expected audible sample peak, got {peak}");
}

#[test]
fn sample_instruments_apply_pan_envelopes_loops_and_finetune() {
    let buffer = AudioBuffer::try_from_mono(3_000, 3, [0.0, 1.0, 0.5]).unwrap();
    let sequence = TimedSequence::new().with_track(
        TrackId::default(),
        TimedTrack::new(
            Instrument::sample(buffer, Note::from_midi(69))
                .pan(-1.0)
                .finetune_cents(1200.0)
                .loop_range(1.0 / 3_000.0, 3.0 / 3_000.0)
                .envelope(melody_bay::SampleEnvelope {
                    points: vec![(0.0, 0.0), (1.0 / 3_000.0, 1.0)],
                }),
        )
        .note_at(0.0, Note::from_midi(93), 4.0 / 3_000.0, Velocity::MAX),
    );
    let (mut sound, _) = sequence
        .sound_data()
        .sample_rate(3_000)
        .into_sound()
        .expect("sequence sound data should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 4];

    sound.process(&mut out, 1.0 / 3_000.0, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 0.5);
    assert_close(out[2].left, 0.5);
    assert_close(out[3].left, 0.25);
    assert_close(out[3].right, 0.0);
}

fn constant_graph(value: f32) -> AudioContext {
    let mut graph = AudioContext::new();
    let source = graph.create_constant_source();
    let gain = graph.create_gain();
    gain.gain().set_value(value).unwrap();
    graph.connect(source, &gain).unwrap();
    graph.connect(&gain, graph.destination()).unwrap();
    graph
}
