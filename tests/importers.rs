use std::sync::{Arc, Mutex};

use kira::backend::{Backend, Renderer};
use kira::info::MockInfoBuilder;
use kira::sound::SoundData;
use kira::{AudioManager, AudioManagerSettings};
use melody_bay::{
    AudioBuffer, ImportWarningKind, ImportedFormat, IndexedAutomationShape, IndexedSequence,
    IndexedTrack, Instrument, MidiImport, ModImport, Note, TimedSequence, TimedTrack, TrackId,
    Velocity, XmImport, gm_drum, gm_instrument, import_midi, import_mod, import_xm,
};

#[test]
fn midi_imports_format_zero_running_status_and_tempo() {
    let imported = MidiImport::from_bytes(&format_zero_midi()).expect("valid midi");

    assert_eq!(imported.source_format, ImportedFormat::Midi);
    assert_eq!(imported.sequence.tempo_map.steps_per_beat(), 96);
    assert_eq!(imported.sequence.tempo_map.tempo_events(), &[(0, 120.0)]);

    let track = imported
        .sequence
        .track(TrackId::named("midi-ch01-program005"))
        .expect("program track");
    assert_eq!(track.notes().len(), 1);
    assert_eq!(track.notes()[0].start_index, 0);
    assert_eq!(track.notes()[0].duration_indices, 96);
    assert_eq!(track.notes()[0].note, Note::from_midi(60));
    assert!((track.notes()[0].velocity.value() - (64.0 / 127.0)).abs() < 0.0001);

    let timed = imported.sequence.resolve();
    assert!(timed.duration_seconds() > 0.49);
}

#[test]
fn midi_imports_format_one_sustain_and_controller_automation() {
    let imported = import_midi(&format_one_midi()).expect("valid midi");
    let track = imported
        .sequence
        .track(TrackId::named("midi-ch02-program000"))
        .expect("channel two track");

    assert_eq!(track.notes().len(), 1);
    assert_eq!(track.notes()[0].duration_indices, 144);
    assert!(
        track
            .automation()
            .iter()
            .any(|event| event.target == "gain")
    );
    assert!(track.automation().iter().any(|event| event.target == "pan"));
    assert!(
        track
            .automation()
            .iter()
            .any(|event| event.target == "playback_rate")
    );
}

#[test]
fn midi_import_rejects_smpte_time_division() {
    let mut midi = format_zero_midi();
    midi[12] = 0xE7;
    midi[13] = 0x28;

    let imported = import_midi(&midi).expect("SMPTE timing is imported on a deterministic grid");
    assert_eq!(imported.sequence.tempo_map.steps_per_beat(), 40);
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| matches!(warning.kind, ImportWarningKind::ApproximatedTiming { .. }))
    );
}

#[test]
fn midi_imports_format_two_with_flattening_warning() {
    let mut midi = midi_header(2, 2, 96);
    midi.extend(track_chunk(&[
        0x00, 0x90, 0x3c, 0x40, 0x60, 0x80, 0x3c, 0x00, 0x00, 0xff, 0x2f, 0x00,
    ]));
    midi.extend(track_chunk(&[
        0x00, 0x91, 0x40, 0x40, 0x30, 0x81, 0x40, 0x00, 0x00, 0xff, 0x2f, 0x00,
    ]));

    let imported = import_midi(&midi).expect("format 2 midi");

    assert_eq!(imported.sequence.tracks().len(), 2);
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| matches!(warning.kind, ImportWarningKind::ApproximatedTiming { .. }))
    );
}

#[test]
fn midi_imports_overlapping_same_note_with_stack() {
    let mut midi = midi_header(0, 1, 96);
    midi.extend(track_chunk(&[
        0x00, 0x90, 0x3c, 0x40, 0x18, 0x90, 0x3c, 0x50, 0x18, 0x80, 0x3c, 0x00, 0x18, 0x80, 0x3c,
        0x00, 0x00, 0xff, 0x2f, 0x00,
    ]));

    let imported = import_midi(&midi).expect("overlapping midi notes");
    let track = imported
        .sequence
        .track(TrackId::named("midi-ch01-program000"))
        .expect("program track");

    assert_eq!(track.notes().len(), 2);
    assert_eq!(track.notes()[0].start_index, 0);
    assert_eq!(track.notes()[0].duration_indices, 48);
    assert_eq!(track.notes()[1].start_index, 24);
    assert_eq!(track.notes()[1].duration_indices, 48);
}

#[test]
fn midi_structures_unsupported_voice_warnings() {
    let mut midi = midi_header(0, 1, 96);
    midi.extend(track_chunk(&[
        0x00, 0xa0, 0x3c, 0x20, 0x00, 0xd0, 0x40, 0x00, 0xff, 0x2f, 0x00,
    ]));

    let imported = import_midi(&midi).expect("voice messages are consumed");

    assert!(imported.warnings.iter().any(|warning| matches!(
        warning.kind,
        ImportWarningKind::DroppedControllerOrAutomation { .. }
    )));
}

#[test]
fn midi_expression_controller_maps_to_gain_automation() {
    let mut midi = midi_header(0, 1, 96);
    midi.extend(track_chunk(&[
        0x00, 0xb0, 0x0b, 0x40, 0x00, 0x90, 0x3c, 0x40, 0x60, 0x80, 0x3c, 0x00, 0x00, 0xff, 0x2f,
        0x00,
    ]));

    let imported = import_midi(&midi).expect("expression midi");
    let track = imported
        .sequence
        .track(TrackId::named("midi-ch01-program000"))
        .expect("program track");

    assert!(track.automation().iter().any(|event| {
        event.target == "gain"
            && matches!(
                event.shape,
                IndexedAutomationShape::SetValue { value } if (value - 64.0 / 127.0).abs() < 0.0001
            )
    }));
}

#[test]
fn midi_rpn_pitch_bend_range_changes_playback_rate_conversion() {
    let mut midi = midi_header(0, 1, 96);
    midi.extend(track_chunk(&[
        0x00, 0xb0, 0x65, 0x00, 0x00, 0xb0, 0x64, 0x00, 0x00, 0xb0, 0x06, 0x0c, 0x00, 0xe0, 0x7f,
        0x7f, 0x00, 0x90, 0x3c, 0x40, 0x60, 0x80, 0x3c, 0x00, 0x00, 0xff, 0x2f, 0x00,
    ]));

    let imported = import_midi(&midi).expect("rpn pitch bend midi");
    let track = imported
        .sequence
        .track(TrackId::named("midi-ch01-program000"))
        .expect("program track");

    assert!(track.automation().iter().any(|event| {
        event.target == "playback_rate"
            && matches!(
                event.shape,
                IndexedAutomationShape::SetValue { value } if (value - 2.0).abs() < 0.001
            )
    }));
}

#[test]
fn midi_meta_events_become_metadata_or_structured_warnings() {
    let mut midi = midi_header(0, 1, 96);
    midi.extend(track_chunk(&[
        0x00, 0xff, 0x03, 0x05, b't', b'i', b't', b'l', b'e', 0x00, 0xff, 0x02, 0x09, b'c', b'o',
        b'p', b'y', b'r', b'i', b'g', b'h', b't', 0x00, 0xff, 0x05, 0x05, b'l', b'y', b'r', b'i',
        b'c', 0x00, 0xff, 0x2f, 0x00,
    ]));

    let imported = import_midi(&midi).expect("metadata midi");

    assert_eq!(imported.metadata.title.as_deref(), Some("title"));
    assert_eq!(imported.metadata.composer.as_deref(), Some("copyright"));
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| matches!(warning.kind, ImportWarningKind::UnsupportedEvent { .. }))
    );
}

#[test]
fn gm_instruments_are_deterministic_instruments() {
    assert!(matches!(gm_instrument(0), Instrument::Graph { .. }));
    assert!(matches!(gm_instrument(24), Instrument::Graph { .. }));
    assert!(matches!(gm_drum(36), Instrument::Graph { .. }));
}

#[test]
fn gm_piano_fallback_decays_instead_of_sustaining_static_oscillators() {
    let note = Note::from_midi(60);
    let track = IndexedTrack::new(gm_instrument(0)).note(0, note, 16);
    let mut sequence = IndexedSequence::new(4).with_track(TrackId::named("piano"), track);
    sequence.tempo_at(0, 120.0);

    let rendered = sequence.resolve().render_offline(44_100);
    let left = rendered.channel_data(0).expect("left channel");
    let early = rms_window(left, 4_410, 8_820);
    let late = rms_window(left, 66_150, 70_560);
    let dc_offset = mean_window(left, 0, left.len()).abs();

    assert!(
        late < early * 0.25,
        "piano fallback should decay during a held note; early={early}, late={late}"
    );
    assert!(
        dc_offset < 0.005,
        "piano fallback should not introduce audible DC rumble; dc_offset={dc_offset}"
    );
}

#[test]
fn gm_piano_fallback_does_not_build_reverb_like_sustain() {
    let note = Note::from_midi(60);
    let track = IndexedTrack::new(gm_instrument(0)).note(0, note, 16);
    let mut sequence = IndexedSequence::new(4).with_track(TrackId::named("piano"), track);
    sequence.tempo_at(0, 120.0);

    let rendered = sequence.resolve().render_offline(44_100);
    let left = rendered.channel_data(0).expect("left channel");
    let early = rms_window(left, 1_000, 4_000);
    let tail = rms_window(left, 13_230, 17_640);

    assert!(
        tail < early * 0.08,
        "piano fallback should decay quickly enough for dense MIDI; early={early}, tail={tail}"
    );
}

#[test]
fn gm_piano_short_notes_fade_before_note_off() {
    let sample_rate = 44_100;
    let note_duration = 0.125;
    let note = Note::from_midi(60);
    let sequence = TimedSequence::new().with_duration(0.2).with_track(
        TrackId::named("piano"),
        TimedTrack::new(gm_instrument(0)).note_at(0.0, note, note_duration, Velocity::MAX),
    );

    let rendered = sequence.render_offline(sample_rate);
    let left = rendered.channel_data(0).expect("left channel");
    let end_index = (note_duration * f64::from(sample_rate)) as usize;

    assert!(
        left[end_index - 1].abs() < 0.005,
        "short MIDI notes should fade before note-off instead of hard-cutting samples; last_sample={}",
        left[end_index - 1]
    );
    assert!(
        left[end_index].abs() < 0.0001,
        "note should still be silent after note-off; after_end={}",
        left[end_index]
    );
}

#[test]
fn example_midi_fallback_render_does_not_accumulate_dc_rumble() {
    let imported = import_midi(include_bytes!("../examples/assets/bach_wtk1_prelude1.mid"))
        .expect("example midi imports");
    let sequence = trim_timed_sequence(imported.sequence.resolve(), 20.0);
    let rendered = sequence.render_offline(44_100);
    let left = rendered.channel_data(0).expect("left channel");
    let dc_offset = mean_window(left, 0, left.len()).abs();

    assert!(
        dc_offset < 0.005,
        "example MIDI fallback should not produce low-frequency DC rumble; dc_offset={dc_offset}"
    );
}

#[test]
fn example_midi_sound_data_matches_offline_render_over_time() {
    let imported = import_midi(include_bytes!("../examples/assets/bach_wtk1_prelude1.mid"))
        .expect("example midi imports");
    let sequence = trim_timed_sequence(imported.sequence.resolve(), 8.0);
    let offline = sequence.render_offline(48_000);
    let live = render_sequence_sound_data(sequence, 48_000, 257);
    let offline_left = offline.channel_data(0).expect("offline left");
    let live_left = live.channel_data(0).expect("live left");

    assert_eq!(offline_left.len(), live_left.len());
    for second in 0..8 {
        let start = second * 48_000;
        let end = start + 48_000;
        let offline_rms = rms_window(offline_left, start, end);
        let live_rms = rms_window(live_left, start, end);
        assert!(
            (offline_rms - live_rms).abs() < 0.0005,
            "live SoundData render diverged from offline render in second {second}; offline={offline_rms}, live={live_rms}"
        );
    }
}

#[test]
fn example_mod_sound_data_matches_kira_manager_render() {
    let imported = import_mod(include_bytes!("../examples/assets/elektric_funk.mod"))
        .expect("example mod imports");
    let sequence = trim_timed_sequence(imported.sequence.resolve(), 4.0);
    let direct = render_sequence_sound_data(sequence.clone(), 48_000, 257);
    let manager = render_sequence_through_kira_manager(sequence, 48_000, 4 * 48_000);
    let direct_left = direct.channel_data(0).expect("direct left");
    let manager_left = manager.channel_data(0).expect("manager left");

    assert_eq!(direct_left.len(), manager_left.len());
    for second in 0..4 {
        let start = second * 48_000;
        let end = start + 48_000;
        let direct_rms = rms_window(direct_left, start, end);
        let manager_rms = rms_window(manager_left, start, end);
        assert!(
            (direct_rms - manager_rms).abs() < 0.0005,
            "Kira manager render diverged from direct imported MOD render in second {second}; direct={direct_rms}, manager={manager_rms}"
        );
    }
}

#[test]
fn sequenced_sample_instrument_retriggers_after_first_note_ends() {
    let sample = AudioBuffer::try_from_mono(8_000, 4, [1.0, 0.5, -0.5, 0.0]).unwrap();
    let note = Note::from_midi(60);
    let track = IndexedTrack::new(Instrument::sample(sample, note))
        .note(0, note, 1)
        .note(2, note, 1);
    let mut sequence = IndexedSequence::new(1).with_track(TrackId::named("sample"), track);
    sequence.tempo_at(0, 60.0);

    let rendered = sequence.resolve().render_offline(8_000);
    let left = rendered.channel_data(0).expect("left channel");

    assert!(
        left[16_000].abs() > 0.5,
        "second sequenced sample note should retrigger after the first sample reaches its end"
    );
}

fn render_sequence_sound_data(
    sequence: TimedSequence,
    sample_rate: u32,
    chunk_size: usize,
) -> AudioBuffer {
    let frame_count = (sequence.duration_seconds() * f64::from(sample_rate)).ceil() as usize;
    let (mut sound, _handle) = sequence
        .sound_data()
        .sample_rate(sample_rate)
        .into_sound()
        .expect("sequence sound data builds");
    let info = MockInfoBuilder::new().build();
    let mut left = Vec::with_capacity(frame_count);
    let mut right = Vec::with_capacity(frame_count);
    let mut rendered = 0usize;
    while rendered < frame_count {
        let chunk = (frame_count - rendered).min(chunk_size);
        let mut out = vec![kira::Frame::ZERO; chunk];
        sound.process(&mut out, 1.0 / f64::from(sample_rate), &info);
        for frame in out {
            left.push(frame.left);
            right.push(frame.right);
        }
        rendered += chunk;
    }
    AudioBuffer::try_from_stereo(sample_rate, frame_count, left, right)
        .expect("valid live render buffer")
}

fn render_sequence_through_kira_manager(
    sequence: TimedSequence,
    sample_rate: u32,
    frame_count: usize,
) -> AudioBuffer {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let mut manager = AudioManager::<CapturingBackend>::new(AudioManagerSettings {
        backend_settings: CapturingBackendSettings {
            sample_rate,
            captured: captured.clone(),
        },
        ..Default::default()
    })
    .expect("capture backend starts");
    manager
        .play(sequence.sound_data())
        .expect("sequence sound starts");
    manager.backend_mut().process(frame_count);
    let captured = captured.lock().expect("captured mutex poisoned");
    let left = captured
        .chunks_exact(2)
        .map(|channels| channels[0])
        .collect::<Vec<_>>();
    let right = captured
        .chunks_exact(2)
        .map(|channels| channels[1])
        .collect::<Vec<_>>();
    AudioBuffer::try_from_stereo(sample_rate, frame_count, left, right)
        .expect("valid manager render buffer")
}

#[derive(Clone)]
struct CapturingBackendSettings {
    sample_rate: u32,
    captured: Arc<Mutex<Vec<f32>>>,
}

impl Default for CapturingBackendSettings {
    fn default() -> Self {
        Self {
            sample_rate: 44_100,
            captured: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

struct CapturingBackend {
    renderer: Option<Renderer>,
    captured: Arc<Mutex<Vec<f32>>>,
}

impl CapturingBackend {
    fn process(&mut self, frames: usize) {
        let renderer = self.renderer.as_mut().expect("renderer has started");
        let mut out = vec![0.0; frames * 2];
        renderer.on_start_processing();
        renderer.process(&mut out, 2);
        self.captured
            .lock()
            .expect("captured mutex poisoned")
            .extend(out);
    }
}

impl Backend for CapturingBackend {
    type Settings = CapturingBackendSettings;
    type Error = ();

    fn setup(
        settings: Self::Settings,
        _internal_buffer_size: usize,
    ) -> Result<(Self, u32), Self::Error> {
        Ok((
            Self {
                renderer: None,
                captured: settings.captured,
            },
            settings.sample_rate,
        ))
    }

    fn start(&mut self, renderer: Renderer) -> Result<(), Self::Error> {
        self.renderer = Some(renderer);
        Ok(())
    }
}

#[test]
fn mod_imports_embedded_sample_and_pattern_note() {
    let imported = ModImport::from_bytes(&minimal_mod()).expect("valid mod");

    assert_eq!(imported.source_format, ImportedFormat::Mod);
    assert_eq!(imported.sequence.tempo_map.steps_per_beat(), 24);
    let track = imported
        .sequence
        .track(TrackId::named("mod-ch01-inst01"))
        .expect("sample track");
    assert_eq!(track.notes().len(), 1);
    assert_eq!(track.notes()[0].start_index, 0);
    assert_eq!(track.notes()[0].duration_indices, 64 * 6);
    let timed = imported.sequence.resolve();
    assert!((timed.duration_seconds() - 7.68).abs() < 0.001);

    let Instrument::Sample(sample) = &track.instrument else {
        panic!("expected sample instrument");
    };
    assert_eq!(sample.buffer.sample_rate(), 8_363);
    assert_eq!(sample.buffer.length(), 2);
    assert!((sample.volume - 0.4).abs() < 0.0001);
    assert!((sample.pan + 1.0).abs() < 0.0001);
    assert!(sample.loop_range.is_none());
}

#[test]
fn mod_ignores_two_byte_no_loop_sentinel() {
    let imported = ModImport::from_bytes(&minimal_mod()).expect("valid mod");
    let track = imported
        .sequence
        .track(TrackId::named("mod-ch01-inst01"))
        .expect("sample track");
    let Instrument::Sample(sample) = &track.instrument else {
        panic!("expected sample instrument");
    };

    assert!(sample.loop_range.is_none());
}

#[test]
fn mod_note_cut_closes_looped_sample_note() {
    let mut module = minimal_mod();
    let sample_header = 20;
    module[sample_header + 28..sample_header + 30].copy_from_slice(&2u16.to_be_bytes());
    module.extend([0x00, 0x00]);
    let row_1_channel_1 = 1084 + 4 * 4;
    module[row_1_channel_1 + 2] = 0x0e;
    module[row_1_channel_1 + 3] = 0xc0;

    let imported = import_mod(&module).expect("mod with note cut");
    let track = imported
        .sequence
        .track(TrackId::named("mod-ch01-inst01"))
        .expect("sample track");

    assert_eq!(track.notes()[0].duration_indices, 6);
}

#[test]
fn mod_note_without_instrument_reuses_channel_instrument() {
    let mut module = minimal_mod();
    let row_1_channel_1 = 1084 + 4 * 4;
    let period = 404u16;
    module[row_1_channel_1] = (period >> 8) as u8 & 0x0f;
    module[row_1_channel_1 + 1] = period as u8;

    let imported = import_mod(&module).expect("mod with instrument memory");
    let track = imported
        .sequence
        .track(TrackId::named("mod-ch01-inst01"))
        .expect("sample track");

    assert_eq!(track.notes().len(), 2);
    assert_eq!(track.notes()[1].note, Note::from_midi(61));
}

#[test]
fn mod_imports_high_octave_periods_without_clamping_to_b3() {
    let mut module = minimal_mod();
    let period = 180u16;
    module[1084] = (period >> 8) as u8 & 0x0f;
    module[1085] = period as u8;

    let imported = import_mod(&module).expect("mod with high period");
    let track = imported
        .sequence
        .track(TrackId::named("mod-ch01-inst01"))
        .expect("sample track");

    assert_eq!(track.notes()[0].note, Note::from_midi(75));
}

#[test]
fn mod_pattern_break_stops_current_pattern_rows() {
    let mut module = minimal_mod();
    let row_1_channel_1 = 1084 + 4 * 4;
    module[row_1_channel_1 + 2] = 0x0d;

    let imported = import_mod(&module).expect("mod with pattern break");
    let track = imported
        .sequence
        .track(TrackId::named("mod-ch01-inst01"))
        .expect("sample track");

    assert_eq!(track.notes()[0].duration_indices, 2 * 6);
    assert!(
        !imported
            .warnings
            .iter()
            .any(|warning| warning.message.contains("d00"))
    );
}

#[test]
fn xm_imports_sample_metadata_envelope_and_pattern_note() {
    let imported = XmImport::from_bytes(&minimal_xm()).expect("valid xm");

    assert_eq!(imported.source_format, ImportedFormat::Xm);
    assert_eq!(imported.sequence.tempo_map.steps_per_beat(), 24);
    let track = imported
        .sequence
        .track(TrackId::named("xm-ch01-inst01"))
        .expect("instrument track");
    assert_eq!(track.notes()[0].note, Note::from_midi(60));
    assert_eq!(track.notes()[0].duration_indices, 6);

    let Instrument::Sample(sample) = &track.instrument else {
        panic!("expected sample instrument");
    };
    assert_eq!(sample.buffer.length(), 4);
    assert_eq!(sample.root_note, Note::from_midi(60));
    assert!((sample.volume - 0.5).abs() < 0.0001);
    assert!(sample.loop_range.is_some());
    assert_eq!(
        sample.envelope.as_ref().expect("envelope").points,
        vec![(0.0, 1.0), (0.12, 0.0)]
    );
}

#[test]
fn xm_relative_note_and_finetune_raise_sample_pitch() {
    let mut xm = minimal_xm();
    let sample_header = xm.len() - 4 - 40;
    xm[sample_header + 13] = 64u8;
    xm[sample_header + 16] = 12u8;

    let imported = XmImport::from_bytes(&xm).expect("valid xm");
    let track = imported
        .sequence
        .track(TrackId::named("xm-ch01-inst01"))
        .expect("instrument track");
    let Instrument::Sample(sample) = &track.instrument else {
        panic!("expected sample instrument");
    };

    assert_eq!(sample.root_note, Note::from_midi(48));
    assert!((sample.finetune_cents + 50.0).abs() < 0.0001);
}

#[test]
fn xm_notes_sustain_until_pattern_end() {
    let mut xm = minimal_xm();
    xm[341..343].copy_from_slice(&4u16.to_le_bytes());
    xm[343..345].copy_from_slice(&20u16.to_le_bytes());
    xm.splice(350..350, vec![0u8; 15]);

    let imported = import_xm(&xm).expect("multi-row xm");
    let track = imported
        .sequence
        .track(TrackId::named("xm-ch01-inst01"))
        .expect("instrument track");

    assert_eq!(track.notes()[0].duration_indices, 4 * 6);
}

#[test]
fn xm_key_off_closes_looped_sample_note() {
    let mut xm = minimal_xm();
    xm[341..343].copy_from_slice(&4u16.to_le_bytes());
    xm[343..345].copy_from_slice(&20u16.to_le_bytes());
    xm.splice(350..350, vec![0u8; 15]);
    xm[350] = 97;

    let imported = import_xm(&xm).expect("xm with key-off");
    let track = imported
        .sequence
        .track(TrackId::named("xm-ch01-inst01"))
        .expect("instrument track");

    assert_eq!(track.notes()[0].duration_indices, 6);
}

#[test]
fn xm_keymap_notes_use_their_selected_sample_instrument() {
    let mut xm = minimal_xm();
    let instrument_offset = 350usize;
    xm[instrument_offset + 27..instrument_offset + 29].copy_from_slice(&2u16.to_le_bytes());
    xm[instrument_offset + 33 + 48] = 1;
    let sample_data_start = xm.len() - 4;
    let first_sample_header = sample_data_start - 40;
    let mut second_sample_header = xm[first_sample_header..sample_data_start].to_vec();
    second_sample_header[12] = 64;
    second_sample_header[18..22].copy_from_slice(b"smp2");
    xm.splice(sample_data_start..sample_data_start, second_sample_header);
    xm.extend([0, 64, 0, 0]);

    let imported = import_xm(&xm).expect("xm with two keymapped samples");
    let track = imported
        .sequence
        .track(TrackId::named("xm-ch01-inst01-samp02"))
        .expect("selected sample track");
    let Instrument::Sample(sample) = &track.instrument else {
        panic!("expected sample instrument");
    };

    assert_eq!(track.notes()[0].note, Note::from_midi(60));
    assert!((sample.volume - 1.0).abs() < 0.0001);
}

#[test]
fn xm_note_without_instrument_reuses_channel_instrument() {
    let mut xm = minimal_xm();
    xm[341..343].copy_from_slice(&4u16.to_le_bytes());
    xm[343..345].copy_from_slice(&20u16.to_le_bytes());
    xm.splice(350..350, vec![0u8; 15]);
    xm[350] = 50;

    let imported = import_xm(&xm).expect("xm with instrument memory");
    let track = imported
        .sequence
        .track(TrackId::named("xm-ch01-inst01"))
        .expect("instrument track");

    assert_eq!(track.notes().len(), 2);
    assert_eq!(track.notes()[1].note, Note::from_midi(61));
}

#[test]
fn xm_pattern_break_stops_current_pattern_rows() {
    let mut xm = minimal_xm();
    xm[341..343].copy_from_slice(&4u16.to_le_bytes());
    xm[343..345].copy_from_slice(&20u16.to_le_bytes());
    xm.splice(350..350, vec![0u8; 15]);
    xm[358] = 0x0d;

    let imported = import_xm(&xm).expect("xm with pattern break");
    let track = imported
        .sequence
        .track(TrackId::named("xm-ch01-inst01"))
        .expect("instrument track");

    assert_eq!(track.notes()[0].duration_indices, 3 * 6);
    assert!(
        !imported
            .warnings
            .iter()
            .any(|warning| warning.message.contains("d00"))
    );
}

#[test]
fn convenience_functions_match_struct_importers() {
    assert_eq!(
        import_mod(&minimal_mod()).unwrap().source_format,
        ModImport::from_bytes(&minimal_mod()).unwrap().source_format
    );
    assert_eq!(
        import_xm(&minimal_xm()).unwrap().source_format,
        XmImport::from_bytes(&minimal_xm()).unwrap().source_format
    );
}

#[test]
fn mod_imports_15_sample_legacy_modules() {
    let imported = import_mod(&minimal_15_sample_mod()).expect("15 sample mod");

    assert_eq!(imported.source_format, ImportedFormat::Mod);
    assert!(
        imported
            .sequence
            .track(TrackId::named("mod-ch01-inst01"))
            .is_some()
    );
}

#[test]
fn mod_effects_emit_automation_instead_of_generic_ignored_warning() {
    let mut module = minimal_mod();
    let pattern_offset = 1084;
    module[pattern_offset + 2] = 0x1c;
    module[pattern_offset + 3] = 0xc0;
    module[pattern_offset + 7] = 0x20;
    module[pattern_offset + 6] = 0xa0;

    let imported = import_mod(&module).expect("mod with volume effects");
    let track = imported
        .sequence
        .track(TrackId::named("mod-ch01-inst01"))
        .expect("sample track");

    assert!(
        track
            .automation()
            .iter()
            .any(|event| event.target == "channel.gain")
    );
    assert!(track.automation().iter().any(|event| {
        event.target == "channel.gain"
            && matches!(
                event.shape,
                IndexedAutomationShape::SetValue { value } if (value - 0.4).abs() < 0.0001
            )
    }));
    assert!(
        !imported
            .warnings
            .iter()
            .any(|warning| warning.message.contains("approximated or ignored"))
    );
}

#[test]
fn mod_panning_effect_maps_to_pan_automation() {
    let mut module = minimal_mod();
    let pattern_offset = 1084;
    module[pattern_offset + 2] = 0x18;
    module[pattern_offset + 3] = 0xff;

    let imported = import_mod(&module).expect("mod with panning effect");
    let track = imported
        .sequence
        .track(TrackId::named("mod-ch01-inst01"))
        .expect("sample track");

    assert!(track.automation().iter().any(|event| {
        event.target == "pan"
            && matches!(
                event.shape,
                IndexedAutomationShape::SetValue { value } if (value - 1.0).abs() < 0.0001
            )
    }));
}

#[test]
fn mod_pitch_effects_emit_playback_rate_automation_without_unsupported_warnings() {
    let mut module = minimal_mod();
    let pattern_offset = 1084;
    module[pattern_offset + 2] = 0x14;
    module[pattern_offset + 3] = 0x02;
    let row_1_channel_1 = pattern_offset + 4 * 4;
    module[row_1_channel_1 + 2] = 0x20;
    module[row_1_channel_1 + 3] = 0x02;
    let row_2_channel_1 = pattern_offset + 2 * 4 * 4;
    module[row_2_channel_1 + 2] = 0x40;
    module[row_2_channel_1 + 3] = 0x47;

    let imported = import_mod(&module).expect("mod with pitch effects");
    let track = imported
        .sequence
        .track(TrackId::named("mod-ch01-inst01"))
        .expect("sample track");

    assert!(track.automation().iter().any(|event| {
        event.target == "source.playbackRate"
            && matches!(event.shape, IndexedAutomationShape::ValueCurve { .. })
    }));
    assert!(!imported.warnings.iter().any(|warning| {
        warning.message.contains("unsupported Mod effect 1")
            || warning.message.contains("unsupported Mod effect 2")
            || warning.message.contains("unsupported Mod effect 4")
    }));
}

#[test]
fn mod_periods_emit_exact_playback_rate_correction() {
    let mut module = minimal_mod();
    let period = 420u16;
    module[1084] = (period >> 8) as u8 & 0x0f;
    module[1085] = period as u8;

    let imported = import_mod(&module).expect("mod with non-table period");
    let track = imported
        .sequence
        .track(TrackId::named("mod-ch01-inst01"))
        .expect("sample track");

    assert!(track.automation().iter().any(|event| {
        event.target == "source.playbackRate"
            && matches!(
                event.shape,
                IndexedAutomationShape::SetValue { value } if (value - 1.019).abs() < 0.01
            )
    }));
}

#[test]
fn mod_extended_note_delay_and_retrigger_use_tick_grid() {
    let mut module = minimal_mod();
    let pattern_offset = 1084;
    module[pattern_offset + 2] = 0x1e;
    module[pattern_offset + 3] = 0xd2;
    let row_1_channel_1 = pattern_offset + 4 * 4;
    module[row_1_channel_1 + 2] = 0x1e;
    module[row_1_channel_1 + 3] = 0x93;

    let imported = import_mod(&module).expect("mod with extended timing effects");
    let track = imported
        .sequence
        .track(TrackId::named("mod-ch01-inst01"))
        .expect("sample track");

    assert_eq!(track.notes()[0].start_index, 2);
    assert!(track.notes().iter().any(|note| note.start_index == 6 + 3));
    assert!(
        !imported
            .warnings
            .iter()
            .any(|warning| warning.message.contains("ed2") || warning.message.contains("e93"))
    );
}

#[test]
fn mod_sample_offset_import_validates_without_source_offset_automation() {
    let mut module = minimal_mod();
    let pattern_offset = 1084;
    module[pattern_offset + 2] = 0x19;
    module[pattern_offset + 3] = 0x01;

    let imported = import_mod(&module).expect("mod with sample offset");
    let timed = imported.sequence.resolve();

    assert!(
        timed
            .tracks()
            .values()
            .flat_map(|track| track.automation())
            .all(|event| event.target != "source.offset")
    );
    timed
        .try_sound_data()
        .expect("imported sample offset sequence should validate");
}

#[test]
fn xm_imports_16_bit_delta_samples() {
    let imported = import_xm(&minimal_xm_16_bit()).expect("16-bit xm");
    let track = imported
        .sequence
        .track(TrackId::named("xm-ch01-inst01"))
        .expect("instrument track");
    let Instrument::Sample(sample) = &track.instrument else {
        panic!("expected sample instrument");
    };

    assert_eq!(sample.buffer.length(), 2);
}

#[test]
fn xm_sample_offset_import_validates_without_source_offset_automation() {
    let mut xm = minimal_xm();
    xm[348] = 0x09;
    xm[349] = 0x01;

    let imported = import_xm(&xm).expect("xm with sample offset");
    let timed = imported.sequence.resolve();

    assert!(
        timed
            .tracks()
            .values()
            .flat_map(|track| track.automation())
            .all(|event| event.target != "source.offset")
    );
    timed
        .try_sound_data()
        .expect("imported sample offset sequence should validate");
}

#[test]
fn xm_volume_column_panning_maps_to_pan_automation() {
    let mut xm = minimal_xm();
    xm[347] = 0xc0;

    let imported = import_xm(&xm).expect("xm with volume-column panning");
    let track = imported
        .sequence
        .track(TrackId::named("xm-ch01-inst01"))
        .expect("instrument track");

    assert!(track.automation().iter().any(|event| {
        event.target == "pan"
            && matches!(
                event.shape,
                IndexedAutomationShape::SetValue { value } if (value + 1.0).abs() < 0.0001
            )
    }));
}

fn format_zero_midi() -> Vec<u8> {
    let mut bytes = midi_header(0, 1, 96);
    bytes.extend(track_chunk(&[
        0x00, 0xff, 0x51, 0x03, 0x07, 0xa1, 0x20, 0x00, 0xc0, 0x05, 0x00, 0x90, 0x3c, 0x40, 0x60,
        0x3c, 0x00, 0x00, 0xff, 0x2f, 0x00,
    ]));
    bytes
}

fn format_one_midi() -> Vec<u8> {
    let mut bytes = midi_header(1, 2, 96);
    bytes.extend(track_chunk(&[
        0x00, 0xff, 0x51, 0x03, 0x07, 0xa1, 0x20, 0x00, 0xff, 0x2f, 0x00,
    ]));
    bytes.extend(track_chunk(&[
        0x00, 0xb1, 0x07, 0x40, 0x00, 0xb1, 0x0a, 0x20, 0x00, 0xe1, 0x00, 0x50, 0x00, 0x91, 0x40,
        0x7f, 0x30, 0xb1, 0x40, 0x7f, 0x30, 0x81, 0x40, 0x00, 0x30, 0xb1, 0x40, 0x00, 0x00, 0xff,
        0x2f, 0x00,
    ]));
    bytes
}

fn midi_header(format: u16, tracks: u16, division: u16) -> Vec<u8> {
    let mut bytes = b"MThd".to_vec();
    bytes.extend(6u32.to_be_bytes());
    bytes.extend(format.to_be_bytes());
    bytes.extend(tracks.to_be_bytes());
    bytes.extend(division.to_be_bytes());
    bytes
}

fn track_chunk(data: &[u8]) -> Vec<u8> {
    let mut bytes = b"MTrk".to_vec();
    bytes.extend((data.len() as u32).to_be_bytes());
    bytes.extend(data);
    bytes
}

fn minimal_mod() -> Vec<u8> {
    let mut bytes = vec![0u8; 20];
    let mut sample = [0u8; 30];
    sample[..5].copy_from_slice(b"pluck");
    sample[22..24].copy_from_slice(&1u16.to_be_bytes());
    sample[25] = 64;
    sample[26..28].copy_from_slice(&0u16.to_be_bytes());
    sample[28..30].copy_from_slice(&1u16.to_be_bytes());
    bytes.extend(sample);
    bytes.extend(vec![0u8; 30 * 30]);
    bytes.push(1);
    bytes.push(0);
    bytes.extend(vec![0u8; 128]);
    bytes.extend(b"M.K.");

    let mut pattern = vec![0u8; 64 * 4 * 4];
    let period = 428u16;
    pattern[0] = (period >> 8) as u8 & 0x0f;
    pattern[1] = period as u8;
    pattern[2] = 0x10;
    pattern[3] = 0x00;
    bytes.extend(pattern);
    bytes.extend([0x00, 0x7f]);
    bytes
}

fn minimal_15_sample_mod() -> Vec<u8> {
    let mut bytes = vec![0u8; 20];
    let mut sample = [0u8; 30];
    sample[..5].copy_from_slice(b"pluck");
    sample[22..24].copy_from_slice(&1u16.to_be_bytes());
    sample[25] = 64;
    bytes.extend(sample);
    bytes.extend(vec![0u8; 14 * 30]);
    bytes.push(1);
    bytes.push(0);
    bytes.extend(vec![0u8; 128]);

    let mut pattern = vec![0u8; 64 * 4 * 4];
    let period = 428u16;
    pattern[0] = (period >> 8) as u8 & 0x0f;
    pattern[1] = period as u8;
    pattern[2] = 0x10;
    pattern[3] = 0x00;
    bytes.extend(pattern);
    bytes.extend([0x00, 0x7f]);
    bytes
}

fn minimal_xm() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend(b"Extended Module: ");
    bytes.extend(fixed_bytes("tiny", 20));
    bytes.push(0x1a);
    bytes.extend(fixed_bytes("melody-bay", 20));
    bytes.extend(0x0104u16.to_le_bytes());
    bytes.extend(276u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(0u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(0u16.to_le_bytes());
    bytes.extend(6u16.to_le_bytes());
    bytes.extend(125u16.to_le_bytes());
    bytes.push(0);
    bytes.extend(vec![0u8; 255]);

    bytes.extend(9u32.to_le_bytes());
    bytes.push(0);
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(5u16.to_le_bytes());
    bytes.extend([0x31, 1, 0, 0, 0]);

    bytes.extend(263u32.to_le_bytes());
    bytes.extend(fixed_bytes("inst", 22));
    bytes.push(0);
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(40u32.to_le_bytes());
    bytes.extend([0u8; 96]);
    bytes.extend(0u16.to_le_bytes());
    bytes.extend(64u16.to_le_bytes());
    bytes.extend(6u16.to_le_bytes());
    bytes.extend(0u16.to_le_bytes());
    bytes.extend([0u8; 40]);
    bytes.extend([0u8; 48]);
    bytes.push(2);
    bytes.push(0);
    bytes.push(0);
    bytes.push(0);
    bytes.push(0);
    bytes.push(0);
    bytes.push(0);
    bytes.push(0);
    bytes.push(1);
    bytes.push(0);
    bytes.extend([0u8; 4]);
    bytes.extend(0u16.to_le_bytes());
    bytes.extend([0u8; 22]);

    bytes.extend(4u32.to_le_bytes());
    bytes.extend(1u32.to_le_bytes());
    bytes.extend(2u32.to_le_bytes());
    bytes.push(32);
    bytes.push(0);
    bytes.push(1);
    bytes.push(0);
    bytes.push(0);
    bytes.push(0);
    bytes.extend(fixed_bytes("samp", 22));
    bytes.extend([0, 127, 128, 0]);
    bytes
}

fn minimal_xm_16_bit() -> Vec<u8> {
    let mut bytes = minimal_xm();
    let sample_header = bytes.len() - 4 - 40;
    bytes[sample_header + 4..sample_header + 8].copy_from_slice(&0u32.to_le_bytes());
    bytes[sample_header + 8..sample_header + 12].copy_from_slice(&0u32.to_le_bytes());
    bytes[sample_header + 12] = 64;
    bytes[sample_header + 14] = 16;
    let data_start = bytes.len() - 4;
    bytes[data_start..].copy_from_slice(&[0, 0, 0, 64]);
    bytes
}

fn fixed_bytes(text: &str, len: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; len];
    bytes[..text.len()].copy_from_slice(text.as_bytes());
    bytes
}

fn trim_timed_sequence(sequence: TimedSequence, duration_seconds: f64) -> TimedSequence {
    let mut trimmed = TimedSequence::new().with_duration(duration_seconds);
    for (id, track) in sequence.tracks() {
        let mut trimmed_track = TimedTrack::new(track.instrument.clone());
        for note in track.notes() {
            if note.start_seconds < duration_seconds {
                trimmed_track = trimmed_track.note_at(
                    note.start_seconds,
                    note.note,
                    note.duration_seconds
                        .min(duration_seconds - note.start_seconds)
                        .max(0.0),
                    note.velocity,
                );
            }
        }
        for automation in track.automation() {
            if automation.time_seconds < duration_seconds {
                trimmed_track = match &automation.shape {
                    melody_bay::AutomationShape::SetValue { value } => trimmed_track.automation_at(
                        automation.time_seconds,
                        automation.target.clone(),
                        *value,
                    ),
                    melody_bay::AutomationShape::LinearRamp { value } => trimmed_track
                        .linear_ramp_to_value_at(
                            automation.time_seconds,
                            automation.target.clone(),
                            *value,
                        ),
                    melody_bay::AutomationShape::ValueCurve {
                        values,
                        duration_seconds,
                    } => trimmed_track.value_curve_at(
                        automation.time_seconds,
                        automation.target.clone(),
                        values.clone(),
                        *duration_seconds,
                    ),
                };
            }
        }
        trimmed.add_track(id.clone(), trimmed_track);
    }
    trimmed
}

fn rms_window(samples: &[f32], start: usize, end: usize) -> f32 {
    let end = end.min(samples.len());
    let start = start.min(end);
    let count = end.saturating_sub(start).max(1);
    let sum = samples[start..end]
        .iter()
        .map(|sample| f64::from(*sample) * f64::from(*sample))
        .sum::<f64>();
    (sum / count as f64).sqrt() as f32
}

fn mean_window(samples: &[f32], start: usize, end: usize) -> f32 {
    let end = end.min(samples.len());
    let start = start.min(end);
    let count = end.saturating_sub(start).max(1);
    let sum = samples[start..end]
        .iter()
        .map(|sample| f64::from(*sample))
        .sum::<f64>();
    (sum / count as f64) as f32
}
