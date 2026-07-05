#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct EventId(u64);

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
struct SequenceTime(f64);

impl SequenceTime {
    #[must_use]
    pub fn seconds(seconds: f64) -> Self {
        Self(seconds.max(0.0))
    }

    #[must_use]
    pub fn as_seconds(self) -> f64 {
        self.0
    }
}

impl From<f64> for SequenceTime {
    fn from(value: f64) -> Self {
        Self::seconds(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Note(u8);

impl Note {
    #[must_use]
    pub fn from_midi(number: u8) -> Self {
        Self(number.min(127))
    }

    #[must_use]
    pub fn midi_number(self) -> u8 {
        self.0
    }

    #[must_use]
    pub fn frequency(self) -> f32 {
        440.0 * 2.0f32.powf((self.0 as f32 - 69.0) / 12.0)
    }

    #[must_use]
    pub fn name(self) -> String {
        const NAMES: [&str; 12] = [
            "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
        ];
        let pitch = self.0 as usize % 12;
        let octave = self.0 as i16 / 12 - 1;
        format!("{}{}", NAMES[pitch], octave)
    }
}

impl Default for Note {
    fn default() -> Self {
        Self::from_midi(69)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Velocity(f32);

impl Velocity {
    pub const MIN: Self = Self(0.0);
    pub const MAX: Self = Self(1.0);

    #[must_use]
    pub fn new(value: f32) -> Self {
        Self(value.clamp(0.0, 1.0))
    }

    #[must_use]
    pub fn value(self) -> f32 {
        self.0
    }
}

impl Default for Velocity {
    fn default() -> Self {
        Self::MAX
    }
}

impl From<f32> for Velocity {
    fn from(value: f32) -> Self {
        Self::new(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InstrumentId(String);

impl InstrumentId {
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TrackId(String);

impl TrackId {
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for TrackId {
    fn default() -> Self {
        Self::named("main")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SequenceMetadata {
    pub title: Option<String>,
    pub composer: Option<String>,
}

impl SequenceMetadata {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    #[must_use]
    pub fn composer(mut self, composer: impl Into<String>) -> Self {
        self.composer = Some(composer.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SampleLoop {
    pub start_seconds: f64,
    pub end_seconds: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SampleInstrument {
    pub buffer: AudioBuffer,
    pub root_note: Note,
    pub finetune_cents: f32,
    pub loop_range: Option<SampleLoop>,
    pub volume: f32,
    pub pan: f32,
    pub envelope: Option<SampleEnvelope>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SampleEnvelope {
    pub points: Vec<(f64, f32)>,
}

#[derive(Debug, Clone)]
pub enum Instrument {
    Graph {
        graph: AudioContext,
        base_note: Note,
    },
    Sample(SampleInstrument),
}

impl Instrument {
    #[must_use]
    pub fn graph(graph: AudioContext) -> Self {
        Self::Graph {
            graph,
            base_note: Note::default(),
        }
    }

    #[must_use]
    pub fn sample(buffer: AudioBuffer, root_note: Note) -> Self {
        Self::Sample(SampleInstrument {
            buffer,
            root_note,
            finetune_cents: 0.0,
            loop_range: None,
            volume: 1.0,
            pan: 0.0,
            envelope: None,
        })
    }

    #[must_use]
    pub fn base_note(mut self, base_note: Note) -> Self {
        if let Self::Graph {
            base_note: current, ..
        } = &mut self
        {
            *current = base_note;
        }
        self
    }

    #[must_use]
    pub fn finetune_cents(mut self, cents: f32) -> Self {
        if let Self::Sample(sample) = &mut self {
            sample.finetune_cents = if cents.is_finite() { cents } else { 0.0 };
        }
        self
    }

    #[must_use]
    pub fn loop_range(mut self, start_seconds: f64, end_seconds: f64) -> Self {
        if let Self::Sample(sample) = &mut self
            && start_seconds.is_finite()
            && end_seconds.is_finite()
        {
            sample.loop_range = Some(SampleLoop {
                start_seconds,
                end_seconds,
            });
        }
        self
    }

    #[must_use]
    pub fn volume(mut self, volume: f32) -> Self {
        if let Self::Sample(sample) = &mut self {
            sample.volume = volume.max(0.0);
        }
        self
    }

    #[must_use]
    pub fn pan(mut self, pan: f32) -> Self {
        if let Self::Sample(sample) = &mut self {
            sample.pan = pan.clamp(-1.0, 1.0);
        }
        self
    }

    #[must_use]
    pub fn envelope(mut self, envelope: SampleEnvelope) -> Self {
        if let Self::Sample(sample) = &mut self {
            sample.envelope = Some(envelope);
        }
        self
    }

    fn base_frequency(&self) -> f32 {
        match self {
            Self::Graph { base_note, .. } => base_note.frequency(),
            Self::Sample(sample) => {
                sample.root_note.frequency() * 2.0f32.powf(sample.finetune_cents / 1200.0)
            }
        }
    }

    fn audio_context(&self) -> AudioContext {
        match self {
            Self::Graph { graph, .. } => graph.clone(),
            Self::Sample(sample) => sample.to_audio_context(),
        }
    }
}

impl SampleInstrument {
    fn to_audio_context(&self) -> AudioContext {
        let mut graph = AudioContext::new();
        let source = graph.create_buffer_source();
        let _ = graph.label_node(&source, "source");
        let _ = source.try_set_buffer(self.buffer.clone());
        if let Some(loop_range) = &self.loop_range {
            source.set_looping(true);
            let _ = source.try_loop_range(loop_range.start_seconds, loop_range.end_seconds);
        }
        let envelope_gain = graph.create_gain();
        let channel_gain = graph.create_gain();
        let _ = graph.label_node(&channel_gain, "channel");
        let pan = graph.create_stereo_panner();
        if let Some(envelope) = &self.envelope {
            let mut points = envelope.points.to_vec();
            points.sort_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((first_time, first_value)) = points.first().copied() {
                let _ = envelope_gain
                    .gain()
                    .set_value_at_time(first_value, first_time.max(0.0));
                for (time, value) in points.into_iter().skip(1) {
                    let _ = envelope_gain
                        .gain()
                        .linear_ramp_to_value_at_time(value, time.max(0.0));
                }
            }
        } else {
            let _ = envelope_gain.gain().set_value(1.0);
        }
        let _ = channel_gain.gain().set_value(self.volume.max(0.0));
        let _ = pan.pan().set_value(self.pan.clamp(-1.0, 1.0));
        let _ = graph.connect(source, &envelope_gain);
        let _ = graph.connect(&envelope_gain, &channel_gain);
        let _ = graph.connect(&channel_gain, &pan);
        let _ = graph.connect(&pan, graph.destination());
        graph
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimedNoteEvent {
    pub start_seconds: f64,
    pub duration_seconds: f64,
    pub note: Note,
    pub velocity: Velocity,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AutomationShape {
    SetValue {
        value: f32,
    },
    LinearRamp {
        value: f32,
    },
    ValueCurve {
        values: Vec<f32>,
        duration_seconds: f64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimedAutomationEvent {
    pub time_seconds: f64,
    pub target: String,
    pub shape: AutomationShape,
}

#[derive(Debug, Clone)]
pub struct TimedTrack {
    pub instrument: Instrument,
    notes: Vec<TimedNoteEvent>,
    automation: Vec<TimedAutomationEvent>,
}

impl TimedTrack {
    #[must_use]
    pub fn new(instrument: Instrument) -> Self {
        Self {
            instrument,
            notes: Vec::new(),
            automation: Vec::new(),
        }
    }

    #[must_use]
    pub fn note_at(
        mut self,
        start_seconds: f64,
        note: Note,
        duration_seconds: f64,
        velocity: Velocity,
    ) -> Self {
        self.notes.push(TimedNoteEvent {
            start_seconds: start_seconds.max(0.0),
            duration_seconds: duration_seconds.max(0.0),
            note,
            velocity,
        });
        self
    }

    #[must_use]
    pub fn automation_at(
        mut self,
        time_seconds: f64,
        target: impl Into<String>,
        value: f32,
    ) -> Self {
        self.automation.push(TimedAutomationEvent {
            time_seconds,
            target: target.into(),
            shape: AutomationShape::SetValue { value },
        });
        self
    }

    #[must_use]
    pub fn linear_ramp_to_value_at(
        mut self,
        end_seconds: f64,
        target: impl Into<String>,
        value: f32,
    ) -> Self {
        self.automation.push(TimedAutomationEvent {
            time_seconds: end_seconds,
            target: target.into(),
            shape: AutomationShape::LinearRamp { value },
        });
        self
    }

    #[must_use]
    pub fn value_curve_at(
        mut self,
        start_seconds: f64,
        target: impl Into<String>,
        values: impl IntoIterator<Item = f32>,
        duration_seconds: f64,
    ) -> Self {
        self.automation.push(TimedAutomationEvent {
            time_seconds: start_seconds,
            target: target.into(),
            shape: AutomationShape::ValueCurve {
                values: values.into_iter().collect(),
                duration_seconds,
            },
        });
        self
    }

    #[must_use]
    pub fn notes(&self) -> &[TimedNoteEvent] {
        &self.notes
    }

    #[must_use]
    pub fn automation(&self) -> &[TimedAutomationEvent] {
        &self.automation
    }
}

#[derive(Debug, Clone)]
pub struct TimedSequence {
    pub metadata: SequenceMetadata,
    duration_seconds: Option<f64>,
    tracks: HashMap<TrackId, TimedTrack>,
}

impl TimedSequence {
    #[must_use]
    pub fn new() -> Self {
        Self {
            metadata: SequenceMetadata::default(),
            duration_seconds: None,
            tracks: HashMap::new(),
        }
    }

    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.metadata.title = Some(title.into());
        self
    }

    #[must_use]
    pub fn composer(mut self, composer: impl Into<String>) -> Self {
        self.metadata.composer = Some(composer.into());
        self
    }

    #[must_use]
    pub fn metadata(&self) -> &SequenceMetadata {
        &self.metadata
    }

    #[must_use]
    pub fn with_duration(mut self, duration_seconds: f64) -> Self {
        self.duration_seconds = Some(duration_seconds.max(0.0));
        self
    }

    #[must_use]
    pub fn duration_seconds(&self) -> f64 {
        self.resolved_duration_seconds()
    }

    #[must_use]
    pub fn with_track(mut self, id: TrackId, track: TimedTrack) -> Self {
        self.add_track(id, track);
        self
    }

    pub fn add_track(&mut self, id: TrackId, track: TimedTrack) {
        self.tracks.insert(id, track);
    }

    #[must_use]
    pub fn track(&self, id: TrackId) -> Option<&TimedTrack> {
        self.tracks.get(&id)
    }

    #[must_use]
    pub fn tracks(&self) -> &HashMap<TrackId, TimedTrack> {
        &self.tracks
    }

    /// Creates Kira sound data without validating sequencer automation.
    ///
    /// This mirrors Kira's ergonomic `SoundData` construction style. Invalid
    /// sequencer automation targets or events are ignored during scheduling.
    /// Use [`Self::try_sound_data`] or [`Self::validate`] when authoring code
    /// should report automation mistakes before playback.
    #[must_use]
    pub fn sound_data(&self) -> SequencerSoundData {
        SequencerSoundData {
            sequence: Arc::new(self.clone()),
            track_id: None,
            sample_rate: 44_100,
        }
    }

    /// Validates sequencer automation before creating Kira sound data.
    pub fn try_sound_data(&self) -> Result<SequencerSoundData, SequencerValidationError> {
        self.validate()?;
        Ok(self.sound_data())
    }

    /// Creates Kira sound data for one track without validating sequencer
    /// automation.
    ///
    /// Use [`Self::try_track_sound_data`] to validate automation on the
    /// requested track before playback.
    #[must_use]
    pub fn track_sound_data(&self, track_id: TrackId) -> SequencerSoundData {
        SequencerSoundData {
            sequence: Arc::new(self.clone()),
            track_id: Some(track_id),
            sample_rate: 44_100,
        }
    }

    /// Validates sequencer automation on the requested track before creating
    /// Kira sound data for that track.
    pub fn try_track_sound_data(
        &self,
        track_id: TrackId,
    ) -> Result<SequencerSoundData, SequencerValidationError> {
        self.validate_track(&track_id)?;
        Ok(self.track_sound_data(track_id))
    }

    /// Renders the sequence offline without validating sequencer automation.
    ///
    /// Use [`Self::try_render_offline`] to validate automation before
    /// rendering.
    #[must_use]
    pub fn render_offline(&self, sample_rate: u32) -> AudioBuffer {
        render_timed_sequence_offline(self, None, sample_rate)
    }

    /// Validates sequencer automation before rendering the sequence offline.
    pub fn try_render_offline(
        &self,
        sample_rate: u32,
    ) -> Result<AudioBuffer, SequencerValidationError> {
        self.validate()?;
        Ok(self.render_offline(sample_rate))
    }

    /// Renders one track offline without validating sequencer automation.
    ///
    /// Use [`Self::try_render_track_offline`] to validate automation on the
    /// requested track before rendering.
    #[must_use]
    pub fn render_track_offline(&self, track_id: TrackId, sample_rate: u32) -> AudioBuffer {
        render_timed_sequence_offline(self, Some(&track_id), sample_rate)
    }

    /// Validates sequencer automation on the requested track before rendering
    /// that track offline.
    pub fn try_render_track_offline(
        &self,
        track_id: TrackId,
        sample_rate: u32,
    ) -> Result<AudioBuffer, SequencerValidationError> {
        self.validate_track(&track_id)?;
        Ok(self.render_track_offline(track_id, sample_rate))
    }

    /// Validates sequencer automation across every track.
    pub fn validate(&self) -> Result<(), SequencerValidationError> {
        for (track_id, track) in &self.tracks {
            validate_timed_track(track_id, track)?;
        }
        Ok(())
    }

    fn validate_track(&self, track_id: &TrackId) -> Result<(), SequencerValidationError> {
        if let Some(track) = self.tracks.get(track_id) {
            validate_timed_track(track_id, track)?;
        }
        Ok(())
    }

    fn resolved_duration_seconds(&self) -> f64 {
        let note_duration = self
            .tracks
            .values()
            .flat_map(|track| track.notes.iter())
            .map(|note| note.start_seconds + note.duration_seconds)
            .fold(0.0, f64::max);
        let automation_duration = self
            .tracks
            .values()
            .flat_map(|track| track.automation.iter())
            .map(|automation| match &automation.shape {
                AutomationShape::ValueCurve {
                    duration_seconds, ..
                } => automation.time_seconds + duration_seconds,
                _ => automation.time_seconds,
            })
            .fold(0.0, f64::max);
        self.duration_seconds
            .unwrap_or(0.0)
            .max(note_duration)
            .max(automation_duration)
    }
}

impl Default for TimedSequence {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod timed_sequence_graph_native_tests {
    use super::*;

    #[test]
    fn compiled_graph_renders_partial_quantum() {
        let mut graph = AudioContext::new();
        let source = graph.create_constant_source();
        source.offset().set_value(0.5).unwrap();
        source.try_start(0.0).unwrap();
        graph
            .connect(source, graph.destination())
            .expect("source connects");

        let compiled = graph.compiled().expect("graph compiles");
        let mut runtime = compiled.runtime().expect("runtime builds");
        let info = MockInfoBuilder::new().build();
        let quantum = compiled.render_quantum_with_runtime(
            RenderQuantum {
                start: 0.0,
                global_start: 0.0,
                sample_dt: 1.0 / 48_000.0,
                frames: 17,
                commit_source_state: false,
            },
            None,
            &mut runtime,
            &info,
        );

        assert_eq!(quantum.len(), 17);
        assert!(quantum.iter().all(|frame| *frame == Frame::new(0.5, 0.5)));
    }

    #[test]
    fn sample_instruments_compile_to_webaudio_native_sample_voice() {
        let buffer = AudioBuffer::try_from_mono(8_000, 4, [0.0, 1.0, 0.0, -1.0]).unwrap();
        let sequence = TimedSequence::new().with_track(
            TrackId::named("sample"),
            TimedTrack::new(Instrument::sample(buffer, Note::from_midi(60))).note_at(
                0.0,
                Note::from_midi(60),
                0.25,
                Velocity::MAX,
            ),
        );

        let compiled =
            compile_timed_sequence_tracks(&sequence, None).expect("sample graph compiles");
        let compiled = compiled
            .get(&TrackId::named("sample"))
            .expect("sample track exists");
        assert!(
            compiled.sample_voice.is_some(),
            "sample instruments should render through a compiled WebAudio graph sample voice"
        );
    }

    #[test]
    fn sequencer_sound_data_stores_modern_sequence_directly() {
        let track_id = TrackId::named("lead");
        let sequence = TimedSequence::new().with_track(
            track_id.clone(),
            TimedTrack::new(Instrument::graph(AudioContext::new())),
        );

        let all_tracks = sequence.sound_data();
        assert_eq!(all_tracks.sample_rate, 44_100);
        assert!(all_tracks.track_id.is_none());
        assert!(Arc::ptr_eq(
            &all_tracks.sequence,
            &all_tracks.sequence.clone()
        ));

        let track = sequence
            .track_sound_data(track_id.clone())
            .sample_rate(22_050);
        assert_eq!(track.sample_rate, 22_050);
        assert_eq!(track.track_id.as_ref(), Some(&track_id));
        assert_eq!(track.sequence.tracks().len(), 1);
    }
}

fn validate_timed_track(
    track_id: &TrackId,
    track: &TimedTrack,
) -> Result<(), SequencerValidationError> {
    let graph = track.instrument.audio_context();
    for automation in &track.automation {
        graph.validate_sequencer_automation_target(track_id, automation)?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub enum SequencerValidationError {
    InvalidAutomationTarget {
        track_id: TrackId,
        target: String,
    },
    InvalidAutomationTime {
        track_id: TrackId,
        target: String,
        time_seconds: f64,
    },
    InvalidAutomationValue {
        track_id: TrackId,
        target: String,
        value: f32,
    },
    InvalidAutomationDuration {
        track_id: TrackId,
        target: String,
        duration_seconds: f64,
    },
}

impl fmt::Display for SequencerValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAutomationTarget { track_id, target } => write!(
                f,
                "invalid automation target '{target}' on track '{}'",
                track_id.as_str()
            ),
            Self::InvalidAutomationTime {
                track_id,
                target,
                time_seconds,
            } => write!(
                f,
                "invalid automation time {time_seconds} for target '{target}' on track '{}'",
                track_id.as_str()
            ),
            Self::InvalidAutomationValue {
                track_id,
                target,
                value,
            } => write!(
                f,
                "invalid automation value {value} for target '{target}' on track '{}'",
                track_id.as_str()
            ),
            Self::InvalidAutomationDuration {
                track_id,
                target,
                duration_seconds,
            } => write!(
                f,
                "invalid automation duration {duration_seconds} for target '{target}' on track '{}'",
                track_id.as_str()
            ),
        }
    }
}

impl std::error::Error for SequencerValidationError {}

#[derive(Debug)]
pub struct SequencerSoundData {
    sequence: Arc<TimedSequence>,
    track_id: Option<TrackId>,
    sample_rate: u32,
}

impl SequencerSoundData {
    #[must_use]
    pub fn sample_rate(mut self, sample_rate: u32) -> Self {
        self.sample_rate = sample_rate.max(1);
        self
    }
}

#[derive(Debug, Clone)]
pub struct SequencerSoundHandle {
    state: Arc<HandleState>,
}

impl SequencerSoundHandle {
    pub fn set_gain(&self, gain: f32) {
        self.state
            .gain_bits
            .store(gain.max(0.0).to_bits(), Ordering::Relaxed);
    }

    pub fn stop(&self) {
        self.state.stopped.store(true, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SequencerSoundError;

impl fmt::Display for SequencerSoundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("failed to build sequencer sound")
    }
}

impl std::error::Error for SequencerSoundError {}

impl SoundData for SequencerSoundData {
    type Error = SequencerSoundError;
    type Handle = SequencerSoundHandle;

    fn into_sound(self) -> Result<(Box<dyn Sound>, Self::Handle), Self::Error> {
        let state = Arc::new(HandleState::default());
        let handle = SequencerSoundHandle {
            state: Arc::clone(&state),
        };
        let sound = SequencerSound::new(self.sequence, self.track_id, self.sample_rate, state)
            .map_err(|_| SequencerSoundError)?;
        Ok((Box::new(sound), handle))
    }
}

#[derive(Debug)]
struct SequencerSound {
    sequence: Arc<TimedSequence>,
    track_id: Option<TrackId>,
    sample_rate: u32,
    elapsed: f64,
    compiled_tracks: HashMap<TrackId, CompiledGraph>,
    scheduled_notes: Vec<ScheduledTimedNote>,
    next_scheduled_note: usize,
    active_notes: Vec<ActiveTimedNote>,
    state: Arc<HandleState>,
}

impl SequencerSound {
    fn new(
        sequence: Arc<TimedSequence>,
        track_id: Option<TrackId>,
        sample_rate: u32,
        state: Arc<HandleState>,
    ) -> Result<Self, GraphError> {
        let compiled_tracks = compile_timed_sequence_tracks(&sequence, track_id.as_ref())?;
        let scheduled_notes = timed_sequence_note_schedule(&sequence, track_id.as_ref());
        let active_capacity = sequence.tracks.len().max(1);
        Ok(Self {
            sequence,
            track_id,
            sample_rate: sample_rate.max(1),
            elapsed: 0.0,
            compiled_tracks,
            scheduled_notes,
            next_scheduled_note: 0,
            active_notes: Vec::with_capacity(active_capacity),
            state,
        })
    }

    fn render_quantum(
        &mut self,
        quantum_start: f64,
        sample_dt: f64,
        frames: usize,
        info: &Info,
    ) -> Vec<Frame> {
        render_timed_sequence_quantum(
            TimedSequenceQuantum {
                track_id: self.track_id.as_ref(),
                quantum_start,
                sample_dt,
                frames,
                compiled_tracks: &self.compiled_tracks,
                scheduled_notes: &self.scheduled_notes,
                handle_state: &self.state,
                info,
            },
            TimedSequenceRenderState {
                next_scheduled_note: &mut self.next_scheduled_note,
                active_notes: &mut self.active_notes,
            },
        )
    }
}

impl Sound for SequencerSound {
    fn process(&mut self, out: &mut [Frame], dt: f64, info: &Info) {
        let sample_dt = if dt.is_finite() && dt > 0.0 {
            dt
        } else {
            1.0 / self.sample_rate as f64
        };
        let mut offset = 0;
        while offset < out.len() {
            let quantum_frames = (out.len() - offset).min(RENDER_QUANTUM_SIZE_USIZE);
            let quantum = self.render_quantum(self.elapsed, sample_dt, quantum_frames, info);
            out[offset..offset + quantum_frames].copy_from_slice(&quantum);
            self.elapsed += sample_dt * quantum_frames as f64;
            offset += quantum_frames;
        }
    }

    fn finished(&self) -> bool {
        self.state.stopped.load(Ordering::Relaxed)
            || self.elapsed >= self.sequence.resolved_duration_seconds()
    }
}

#[derive(Debug)]
struct ActiveTimedNote {
    track_id: TrackId,
    note: NoteEvent,
    runtime: VoiceRuntime,
}

struct TimedSequenceQuantum<'a> {
    track_id: Option<&'a TrackId>,
    quantum_start: f64,
    sample_dt: f64,
    frames: usize,
    compiled_tracks: &'a HashMap<TrackId, CompiledGraph>,
    scheduled_notes: &'a [ScheduledTimedNote],
    handle_state: &'a HandleState,
    info: &'a Info<'a>,
}

struct TimedSequenceRenderState<'a> {
    next_scheduled_note: &'a mut usize,
    active_notes: &'a mut Vec<ActiveTimedNote>,
}

#[derive(Debug, Clone)]
struct ScheduledTimedNote {
    track_id: TrackId,
    note: NoteEvent,
}

fn compile_timed_sequence_tracks(
    sequence: &TimedSequence,
    track_id: Option<&TrackId>,
) -> Result<HashMap<TrackId, CompiledGraph>, GraphError> {
    let mut compiled = HashMap::new();
    for (candidate_id, track) in &sequence.tracks {
        if track_id.is_some_and(|track_id| candidate_id != track_id) {
            continue;
        }
        let mut graph = track.instrument.audio_context();
        for automation in &track.automation {
            graph.schedule_named_param_automation(automation);
        }
        compiled.insert(candidate_id.clone(), graph.compiled()?);
    }
    Ok(compiled)
}

fn timed_sequence_note_schedule(
    sequence: &TimedSequence,
    track_id: Option<&TrackId>,
) -> Vec<ScheduledTimedNote> {
    let mut notes = Vec::new();
    for (candidate_id, track) in &sequence.tracks {
        if track_id.is_some_and(|track_id| candidate_id != track_id) {
            continue;
        }
        for (index, note) in track.notes.iter().enumerate() {
            notes.push(ScheduledTimedNote {
                track_id: candidate_id.clone(),
                note: NoteEvent {
                    id: EventId(index as u64 + 1),
                    start: SequenceTime::seconds(note.start_seconds),
                    duration: note.duration_seconds,
                    frequency: note.note.frequency(),
                    base_frequency: track.instrument.base_frequency(),
                    velocity: note.velocity.value(),
                },
            });
        }
    }
    notes.sort_by(|left, right| {
        left.note
            .start
            .as_seconds()
            .total_cmp(&right.note.start.as_seconds())
    });
    notes
}

fn render_timed_sequence_offline(
    sequence: &TimedSequence,
    track_id: Option<&TrackId>,
    sample_rate: u32,
) -> AudioBuffer {
    let sample_rate = sample_rate.max(1);
    let frame_count =
        (sequence.resolved_duration_seconds().max(0.0) * sample_rate as f64).ceil() as usize;
    let compiled_tracks = compile_timed_sequence_tracks(sequence, track_id).unwrap_or_default();
    let scheduled_notes = timed_sequence_note_schedule(sequence, track_id);
    let mut next_scheduled_note = 0;
    let mut active_notes = Vec::with_capacity(sequence.tracks.len().max(1));
    let state = HandleState::default();
    let info = MockInfoBuilder::new().build();
    let sample_dt = 1.0 / sample_rate as f64;
    let mut frames = Vec::with_capacity(frame_count);
    let mut index = 0;
    while index < frame_count {
        let quantum_frames = (frame_count - index).min(RENDER_QUANTUM_SIZE_USIZE);
        let quantum = render_timed_sequence_quantum(
            TimedSequenceQuantum {
                track_id,
                quantum_start: index as f64 * sample_dt,
                sample_dt,
                frames: quantum_frames,
                compiled_tracks: &compiled_tracks,
                scheduled_notes: &scheduled_notes,
                handle_state: &state,
                info: &info,
            },
            TimedSequenceRenderState {
                next_scheduled_note: &mut next_scheduled_note,
                active_notes: &mut active_notes,
            },
        );
        frames.extend_from_slice(&quantum);
        index += quantum_frames;
    }
    AudioBuffer::from_frames(sample_rate, &frames)
}

fn render_timed_sequence_quantum(
    quantum: TimedSequenceQuantum<'_>,
    state: TimedSequenceRenderState<'_>,
) -> Vec<Frame> {
    let TimedSequenceQuantum {
        track_id,
        quantum_start,
        sample_dt,
        frames,
        compiled_tracks,
        scheduled_notes,
        handle_state,
        info,
    } = quantum;
    let TimedSequenceRenderState {
        next_scheduled_note,
        active_notes,
    } = state;
    let frames = frames.min(RENDER_QUANTUM_SIZE_USIZE);
    let mut rendered = vec![Frame::ZERO; frames];
    if handle_state.stopped.load(Ordering::Relaxed) {
        return rendered;
    }
    let live_gain = f32::from_bits(handle_state.gain_bits.load(Ordering::Relaxed));
    let mut frame_offset = 0;
    while frame_offset < frames {
        let segment_start = quantum_start + frame_offset as f64 * sample_dt;
        while let Some(scheduled) = scheduled_notes.get(*next_scheduled_note) {
            if scheduled.note.start.as_seconds() > segment_start {
                break;
            }
            if scheduled.note.active_at(segment_start) {
                active_notes.push(ActiveTimedNote {
                    track_id: scheduled.track_id.clone(),
                    note: scheduled.note.clone(),
                    runtime: VoiceRuntime::default(),
                });
            }
            *next_scheduled_note += 1;
        }
        active_notes.retain(|active| {
            track_id.is_none_or(|track_id| &active.track_id == track_id)
                && active.note.active_at(segment_start)
        });

        let mut next_boundary = frames;
        if let Some(next) = scheduled_notes.get(*next_scheduled_note) {
            let next_start = next.note.start.as_seconds();
            if next_start > segment_start {
                next_boundary = next_boundary.min(frame_index_for_time(
                    next_start,
                    quantum_start,
                    sample_dt,
                    frames,
                ));
            }
        }
        for active in active_notes.iter() {
            let note_end = active.note.start.as_seconds() + active.note.duration;
            if note_end > segment_start {
                next_boundary = next_boundary.min(frame_index_for_time(
                    note_end,
                    quantum_start,
                    sample_dt,
                    frames,
                ));
            }
        }
        let segment_end = next_boundary.max(frame_offset + 1).min(frames);
        let segment_frames = segment_end - frame_offset;

        for active in active_notes.iter_mut() {
            let Some(compiled) = compiled_tracks.get(&active.track_id) else {
                continue;
            };
            let note_frames = compiled.render_note_quantum(
                &active.note,
                segment_start,
                sample_dt,
                segment_frames,
                &mut active.runtime,
                info,
            );
            for (frame, sample) in note_frames.into_iter().enumerate() {
                rendered[frame_offset + frame] += sample;
            }
        }
        frame_offset = segment_end;
    }
    for frame in &mut rendered {
        *frame *= live_gain;
    }
    rendered
}

#[derive(Debug, Clone, PartialEq)]
pub struct TempoMap {
    steps_per_beat: u32,
    events: Vec<(u64, f64)>,
}

impl TempoMap {
    #[must_use]
    pub fn new(steps_per_beat: u32) -> Self {
        Self {
            steps_per_beat: steps_per_beat.max(1),
            events: vec![(0, 120.0)],
        }
    }

    pub fn tempo_at(&mut self, index: u64, bpm: f64) {
        let bpm = if bpm.is_finite() && bpm > 0.0 {
            bpm
        } else {
            120.0
        };
        if let Some((_, existing)) = self.events.iter_mut().find(|(event, _)| *event == index) {
            *existing = bpm;
        } else {
            self.events.push((index, bpm));
            self.events.sort_by_key(|(index, _)| *index);
        }
    }

    #[must_use]
    pub fn steps_per_beat(&self) -> u32 {
        self.steps_per_beat
    }

    #[must_use]
    pub fn tempo_events(&self) -> &[(u64, f64)] {
        &self.events
    }

    #[must_use]
    pub fn bpm_at(&self, index: u64) -> f64 {
        self.events
            .iter()
            .take_while(|(event_index, _)| *event_index <= index)
            .last()
            .map_or(120.0, |(_, bpm)| *bpm)
    }

    #[must_use]
    pub fn seconds_at(&self, index: u64) -> f64 {
        self.seconds_between(0, index)
    }

    #[must_use]
    pub fn seconds_between(&self, start_index: u64, end_index: u64) -> f64 {
        if end_index <= start_index {
            return 0.0;
        }
        let mut seconds = 0.0;
        let mut cursor = start_index;
        let mut bpm = self.bpm_at(start_index);
        for (event_index, event_bpm) in self.events.iter().copied() {
            if event_index <= start_index {
                continue;
            }
            if event_index >= end_index {
                break;
            }
            seconds += self.segment_seconds(cursor, event_index, bpm);
            cursor = event_index;
            bpm = event_bpm;
        }
        seconds + self.segment_seconds(cursor, end_index, bpm)
    }

    fn segment_seconds(&self, start_index: u64, end_index: u64, bpm: f64) -> f64 {
        let steps = end_index.saturating_sub(start_index) as f64;
        let beats = steps / self.steps_per_beat as f64;
        beats * 60.0 / bpm
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct IndexedNoteEvent {
    pub start_index: u64,
    pub duration_indices: u64,
    pub note: Note,
    pub velocity: Velocity,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IndexedAutomationEvent {
    pub index: u64,
    pub target: String,
    pub shape: IndexedAutomationShape,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IndexedAutomationShape {
    SetValue {
        value: f32,
    },
    LinearRamp {
        value: f32,
    },
    ValueCurve {
        values: Vec<f32>,
        duration_indices: u64,
    },
}

#[derive(Debug, Clone)]
pub struct IndexedTrack {
    pub instrument: Instrument,
    notes: Vec<IndexedNoteEvent>,
    automation: Vec<IndexedAutomationEvent>,
}

impl IndexedTrack {
    #[must_use]
    pub fn new(instrument: Instrument) -> Self {
        Self {
            instrument,
            notes: Vec::new(),
            automation: Vec::new(),
        }
    }

    #[must_use]
    pub fn note(mut self, start_index: u64, note: Note, duration_indices: u64) -> Self {
        self.notes.push(IndexedNoteEvent {
            start_index,
            duration_indices,
            note,
            velocity: Velocity::MAX,
        });
        self
    }

    #[must_use]
    pub fn note_with_velocity(
        mut self,
        start_index: u64,
        note: Note,
        duration_indices: u64,
        velocity: Velocity,
    ) -> Self {
        self.notes.push(IndexedNoteEvent {
            start_index,
            duration_indices,
            note,
            velocity,
        });
        self
    }

    #[must_use]
    pub fn automation_at(mut self, index: u64, target: impl Into<String>, value: f32) -> Self {
        self.automation.push(IndexedAutomationEvent {
            index,
            target: target.into(),
            shape: IndexedAutomationShape::SetValue { value },
        });
        self
    }

    #[must_use]
    pub fn linear_ramp_to_value_at_index(
        mut self,
        end_index: u64,
        target: impl Into<String>,
        value: f32,
    ) -> Self {
        self.automation.push(IndexedAutomationEvent {
            index: end_index,
            target: target.into(),
            shape: IndexedAutomationShape::LinearRamp { value },
        });
        self
    }

    #[must_use]
    pub fn value_curve_at_index(
        mut self,
        start_index: u64,
        target: impl Into<String>,
        values: impl IntoIterator<Item = f32>,
        duration_indices: u64,
    ) -> Self {
        self.automation.push(IndexedAutomationEvent {
            index: start_index,
            target: target.into(),
            shape: IndexedAutomationShape::ValueCurve {
                values: values.into_iter().collect(),
                duration_indices,
            },
        });
        self
    }

    #[must_use]
    pub fn notes(&self) -> &[IndexedNoteEvent] {
        &self.notes
    }

    #[must_use]
    pub fn automation(&self) -> &[IndexedAutomationEvent] {
        &self.automation
    }
}

#[derive(Debug, Clone)]
pub struct IndexedSequence {
    pub metadata: SequenceMetadata,
    pub tempo_map: TempoMap,
    tracks: HashMap<TrackId, IndexedTrack>,
}

impl IndexedSequence {
    #[must_use]
    pub fn new(steps_per_beat: u32) -> Self {
        Self {
            metadata: SequenceMetadata::default(),
            tempo_map: TempoMap::new(steps_per_beat),
            tracks: HashMap::new(),
        }
    }

    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.metadata.title = Some(title.into());
        self
    }

    #[must_use]
    pub fn composer(mut self, composer: impl Into<String>) -> Self {
        self.metadata.composer = Some(composer.into());
        self
    }

    #[must_use]
    pub fn metadata(&self) -> &SequenceMetadata {
        &self.metadata
    }

    pub fn tempo_at(&mut self, index: u64, bpm: f64) {
        self.tempo_map.tempo_at(index, bpm);
    }

    pub fn add_track(&mut self, id: TrackId, track: IndexedTrack) {
        self.tracks.insert(id, track);
    }

    #[must_use]
    pub fn with_track(mut self, id: TrackId, track: IndexedTrack) -> Self {
        self.add_track(id, track);
        self
    }

    #[must_use]
    pub fn track(&self, id: TrackId) -> Option<&IndexedTrack> {
        self.tracks.get(&id)
    }

    #[must_use]
    pub fn tracks(&self) -> &HashMap<TrackId, IndexedTrack> {
        &self.tracks
    }

    #[must_use]
    pub fn resolve(&self) -> TimedSequence {
        let mut timed = TimedSequence {
            metadata: self.metadata.clone(),
            duration_seconds: None,
            tracks: HashMap::new(),
        };
        for (track_id, track) in &self.tracks {
            let mut timed_track = TimedTrack::new(track.instrument.clone());
            for note in &track.notes {
                let start_seconds = self.tempo_map.seconds_at(note.start_index);
                let duration_seconds = self.tempo_map.seconds_between(
                    note.start_index,
                    note.start_index.saturating_add(note.duration_indices),
                );
                timed_track =
                    timed_track.note_at(start_seconds, note.note, duration_seconds, note.velocity);
            }
            for automation in &track.automation {
                let time_seconds = self.tempo_map.seconds_at(automation.index);
                timed_track = match &automation.shape {
                    IndexedAutomationShape::SetValue { value } => {
                        timed_track.automation_at(time_seconds, automation.target.clone(), *value)
                    }
                    IndexedAutomationShape::LinearRamp { value } => timed_track
                        .linear_ramp_to_value_at(time_seconds, automation.target.clone(), *value),
                    IndexedAutomationShape::ValueCurve {
                        values,
                        duration_indices,
                    } => timed_track.value_curve_at(
                        time_seconds,
                        automation.target.clone(),
                        values.iter().copied(),
                        self.tempo_map.seconds_between(
                            automation.index,
                            automation.index.saturating_add(*duration_indices),
                        ),
                    ),
                };
            }
            timed.add_track(track_id.clone(), timed_track);
        }
        timed
    }
}
