use kira::{Frame, info::MockInfoBuilder, sound::Sound, sound::SoundData};
use melody_bay::{
    AudioBuffer, AudioContext, BiquadFilterType, ChannelCountMode, ChannelInterpretation,
    DistanceModel, GraphError, OfflineAudioContext, Oversample, PanningModel, PeriodicWave,
    Waveform,
};

fn left_samples(buffer: &AudioBuffer) -> &[f32] {
    buffer.channel_data(0).expect("left channel")
}

fn worklet_input_channel(inputs: &[Vec<Vec<f32>>], port: usize, channel: usize) -> Option<&[f32]> {
    inputs
        .get(port)
        .and_then(|port| port.get(channel))
        .map(Vec::as_slice)
}

fn worklet_output_channel_mut(
    outputs: &mut [Vec<Vec<f32>>],
    port: usize,
    channel: usize,
) -> Option<&mut [f32]> {
    outputs
        .get_mut(port)
        .and_then(|port| port.get_mut(channel))
        .map(Vec::as_mut_slice)
}

struct TestSoundData {
    frames: Vec<Frame>,
}

impl SoundData for TestSoundData {
    type Error = std::convert::Infallible;
    type Handle = ();

    fn into_sound(self) -> Result<(Box<dyn Sound>, Self::Handle), Self::Error> {
        Ok((
            Box::new(TestSound {
                frames: self.frames,
                cursor: 0,
            }),
            (),
        ))
    }
}

struct TestSound {
    frames: Vec<Frame>,
    cursor: usize,
}

impl Sound for TestSound {
    fn process(&mut self, out: &mut [Frame], _dt: f64, _info: &kira::info::Info) {
        for frame in out {
            *frame = self.frames.get(self.cursor).copied().unwrap_or(Frame::ZERO);
            self.cursor += 1;
        }
    }

    fn finished(&self) -> bool {
        self.cursor >= self.frames.len()
    }
}

fn audio_buffer_from_mono(
    sample_rate: u32,
    length: usize,
    samples: impl IntoIterator<Item = f32>,
) -> AudioBuffer {
    audio_buffer_from_channels(sample_rate, length, [samples])
}

fn audio_buffer_from_stereo(
    sample_rate: u32,
    length: usize,
    left: impl IntoIterator<Item = f32>,
    right: impl IntoIterator<Item = f32>,
) -> AudioBuffer {
    audio_buffer_from_channels(
        sample_rate,
        length,
        [
            left.into_iter().collect::<Vec<_>>(),
            right.into_iter().collect(),
        ],
    )
}

fn audio_buffer_from_channels<I>(
    sample_rate: u32,
    length: usize,
    channels: impl IntoIterator<Item = I>,
) -> AudioBuffer
where
    I: IntoIterator<Item = f32>,
{
    if sample_rate >= 3_000 {
        return AudioBuffer::try_from_channels(sample_rate, length, channels).unwrap();
    }
    let channels = channels
        .into_iter()
        .map(|samples| {
            let mut channel = samples.into_iter().take(length).collect::<Vec<_>>();
            channel.resize(length, 0.0);
            channel
        })
        .collect::<Vec<_>>();
    let fixture_rate = 3_000;
    let fixture_length =
        ((length as f64 / sample_rate.max(1) as f64) * fixture_rate as f64).ceil() as usize;
    let fixture_length = fixture_length.max(1);
    let resampled = channels
        .iter()
        .map(|channel| {
            (0..fixture_length)
                .map(|index| {
                    let source_index = ((index as f64 / fixture_rate as f64)
                        * sample_rate.max(1) as f64)
                        .floor() as usize;
                    channel
                        .get(source_index.min(length.saturating_sub(1)))
                        .copied()
                        .unwrap_or(0.0)
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    AudioBuffer::try_from_channels(fixture_rate, fixture_length, resampled).unwrap()
}

fn process_live_graph(context: &AudioContext, sample_rate: u32, frames: usize) {
    let (mut sound, _) = context
        .sound_data()
        .sample_rate(sample_rate)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = vec![Frame::ZERO; frames];
    sound.process(&mut out, 1.0 / sample_rate as f64, &info);
}

fn render_context_offline(
    context: &AudioContext,
    sample_rate: u32,
    frames: usize,
) -> Result<AudioBuffer, GraphError> {
    render_context_offline_channels(context, sample_rate, frames, 2)
}

fn render_context_offline_channels(
    context: &AudioContext,
    sample_rate: u32,
    frames: usize,
    channels: usize,
) -> Result<AudioBuffer, GraphError> {
    if context.state() == melody_bay::OfflineAudioContextState::Closed {
        return Err(GraphError::ContextClosed);
    }
    if channels <= 2 {
        let render_rate = sample_rate.max(1);
        let (mut sound, _) = context
            .sound_data()
            .sample_rate(render_rate)
            .into_sound()
            .map_err(|_| GraphError::InvalidAudioBuffer)?;
        let info = MockInfoBuilder::new().build();
        let mut out = vec![Frame::ZERO; frames];
        sound.process(&mut out, 1.0 / render_rate as f64, &info);
        let left = out.iter().map(|frame| frame.left).collect::<Vec<_>>();
        let right = out.iter().map(|frame| frame.right).collect::<Vec<_>>();
        if sample_rate < 3_000 {
            return AudioBuffer::try_from_stereo(3_000, frames, left, right);
        }
        return AudioBuffer::try_from_stereo(sample_rate, frames, left, right);
    }
    Err(GraphError::InvalidChannelCount)
}

fn started_constant_source(context: &mut AudioContext) -> melody_bay::ConstantSourceNode {
    let source = context.create_constant_source();
    source.try_start(0.0).unwrap();
    source
}

fn started_buffer_source(context: &mut AudioContext) -> melody_bay::AudioBufferSourceNode {
    let source = context.create_buffer_source();
    source.try_start(0.0).unwrap();
    source
}

fn started_buffer_source_with_buffer(
    context: &mut AudioContext,
    buffer: AudioBuffer,
) -> melody_bay::AudioBufferSourceNode {
    let source = context.create_buffer_source();
    source.try_set_buffer(buffer).unwrap();
    source.try_start(0.0).unwrap();
    source
}

fn started_offline_constant_source(
    context: &mut OfflineAudioContext,
) -> melody_bay::ConstantSourceNode {
    let source = context.create_constant_source();
    source.try_start(0.0).unwrap();
    source
}

fn started_offline_buffer_source(
    context: &mut OfflineAudioContext,
) -> melody_bay::AudioBufferSourceNode {
    let source = context.create_buffer_source();
    source.try_start(0.0).unwrap();
    source
}

fn started_offline_buffer_source_with_buffer(
    context: &mut OfflineAudioContext,
    buffer: AudioBuffer,
) -> melody_bay::AudioBufferSourceNode {
    let source = context.create_buffer_source();
    source.try_set_buffer(buffer).unwrap();
    source.try_start(0.0).unwrap();
    source
}

fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= 0.0001,
        "expected {actual} to be close to {expected}"
    );
}

#[test]
fn audio_context_factories_use_webaudio_defaults() {
    let mut context = AudioContext::new();

    let oscillator = context.create_oscillator();
    let source = context.create_buffer_source();
    let filter = context.create_biquad_filter();
    let convolver = context.create_convolver();
    let delay = context.create_delay();
    let shaper = context.create_wave_shaper();
    let splitter = context.create_channel_splitter();
    let merger = context.create_channel_merger();

    assert_eq!(oscillator.type_value(), melody_bay::Waveform::Sine);
    assert_eq!(oscillator.frequency_value(), 440.0);
    assert_eq!(source.buffer_value(), None);
    assert_eq!(filter.type_value(), BiquadFilterType::Lowpass);
    assert_eq!(delay.delay_time_value(), 0.0);
    assert_eq!(delay.max_delay_time_value(), 1.0);
    assert_eq!(convolver.buffer_value(), None);
    assert!(convolver.normalize_value());
    assert_eq!(shaper.curve_value(), None);
    assert_eq!(shaper.oversample_value(), Oversample::None);
    assert_eq!(context.node_info(&splitter).unwrap().number_of_outputs, 6);
    assert_eq!(context.node_info(&merger).unwrap().number_of_inputs, 6);
}

#[test]
fn nullable_source_nodes_follow_webaudio_silence_or_passthrough() {
    let mut context = AudioContext::new();
    let source = started_buffer_source(&mut context);
    context.connect(&source, context.destination()).unwrap();
    let buffer = render_context_offline(&context, 4, 4).unwrap();
    assert_eq!(left_samples(&buffer), &[0.0, 0.0, 0.0, 0.0]);

    let mut context = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut context,
        audio_buffer_from_mono(4, 4, [0.25, 0.5, 0.75, 1.0]),
    );
    let shaper = context.create_wave_shaper();
    context.connect(&source, &shaper).unwrap();
    context.connect(&shaper, context.destination()).unwrap();
    let buffer = render_context_offline(&context, 4, 4).unwrap();
    assert_eq!(left_samples(&buffer), &[0.25, 0.5, 0.75, 1.0]);

    let mut context = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut context,
        audio_buffer_from_mono(4, 4, [0.25, 0.5, 0.75, 1.0]),
    );
    let convolver = context.create_convolver();
    context.connect(&source, &convolver).unwrap();
    context.connect(&convolver, context.destination()).unwrap();
    let buffer = render_context_offline(&context, 4, 4).unwrap();
    assert_eq!(left_samples(&buffer), &[0.0, 0.0, 0.0, 0.0]);
}

#[test]
fn nullable_node_attributes_can_be_cleared_after_being_set() {
    let mut context = AudioContext::new();
    let source = context.create_buffer_source();
    source
        .try_set_buffer(audio_buffer_from_mono(3_000, 1, [1.0]))
        .unwrap();
    source.clear_buffer();
    source.try_start(0.0).unwrap();
    context.connect(&source, context.destination()).unwrap();
    assert_eq!(source.buffer_value(), None);
    assert_eq!(
        left_samples(&render_context_offline(&context, 4, 1).unwrap()),
        &[0.0]
    );

    let mut context = AudioContext::new();
    let source =
        started_buffer_source_with_buffer(&mut context, audio_buffer_from_mono(4, 1, [1.0]));
    let convolver = context.create_convolver();
    convolver
        .try_buffer(audio_buffer_from_mono(44_100, 1, [1.0]))
        .unwrap();
    convolver.clear_buffer();
    context.connect(&source, &convolver).unwrap();
    context.connect(&convolver, context.destination()).unwrap();
    assert_eq!(convolver.buffer_value(), None);
    assert_eq!(
        left_samples(&render_context_offline(&context, 4, 1).unwrap()),
        &[0.0]
    );

    let mut context = AudioContext::new();
    let source =
        started_buffer_source_with_buffer(&mut context, audio_buffer_from_mono(4, 1, [0.5]));
    let shaper = context.create_wave_shaper();
    shaper.clear_curve();
    shaper.try_curve([-1.0, 1.0]).unwrap();
    shaper.clear_curve();
    context.connect(&source, &shaper).unwrap();
    context.connect(&shaper, context.destination()).unwrap();
    assert_eq!(shaper.curve_value(), None);
    assert_eq!(
        left_samples(&render_context_offline(&context, 4, 1).unwrap()),
        &[0.5]
    );
}

#[test]
fn convolver_exposes_node_owned_attribute_setters() {
    let mut context = AudioContext::new();
    let convolver = context.create_convolver();

    assert!(convolver.normalize_value());
    convolver.set_normalize(false);
    assert!(!convolver.normalize_value());
    convolver
        .try_buffer(audio_buffer_from_mono(44_100, 1, [1.0]))
        .unwrap();
    convolver.clear_buffer();
    assert_eq!(convolver.buffer_value(), None);
}

#[test]
fn wave_shaper_exposes_node_owned_attribute_setters() {
    let mut context = AudioContext::new();
    let shaper = context.create_wave_shaper();

    assert_eq!(shaper.oversample_value(), Oversample::None);
    shaper.set_oversample(Oversample::TwoX);
    assert_eq!(shaper.oversample_value(), Oversample::TwoX);
    shaper.try_curve([-1.0, 1.0]).unwrap();
    shaper.clear_curve();
    assert_eq!(shaper.curve_value(), None);
}

#[test]
fn waveshaper_try_curve_validates_webaudio_curve_values() {
    let mut context = AudioContext::new();
    let shaper = context.create_wave_shaper();

    assert_eq!(
        shaper.try_curve([]),
        Err(GraphError::InvalidWaveShaperCurve)
    );
    assert_eq!(
        shaper.try_curve([0.0, f32::NAN]),
        Err(GraphError::InvalidWaveShaperCurve)
    );
    assert_eq!(
        shaper.try_curve([0.0, f32::INFINITY]),
        Err(GraphError::InvalidWaveShaperCurve)
    );
    assert_eq!(shaper.curve_value(), None);

    shaper.try_curve([-1.0, 1.0]).unwrap();
    assert_eq!(shaper.curve_value(), Some(vec![-1.0, 1.0]));
}

#[test]
fn context_try_create_buffer_rejects_invalid_webaudio_arguments() {
    let context = AudioContext::new();

    assert_eq!(
        context.try_create_buffer(0, 1, 44_100),
        Err(GraphError::InvalidAudioBuffer)
    );
    assert_eq!(
        context.try_create_buffer(1, 1, 2_999),
        Err(GraphError::InvalidAudioBuffer)
    );
    assert_eq!(
        context.try_create_buffer(1, 1, 384_001),
        Err(GraphError::InvalidAudioBuffer)
    );
    assert_eq!(
        context.try_create_buffer(1, 0, 44_100),
        Err(GraphError::InvalidAudioBuffer)
    );

    let buffer = context.try_create_buffer(2, 4, 44_100).unwrap();
    assert_eq!(buffer.number_of_channels(), 2);
    assert_eq!(buffer.length(), 4);
    assert_eq!(buffer.sample_rate(), 44_100);
}

#[test]
fn audio_buffer_try_constructors_reject_invalid_webaudio_arguments() {
    assert_eq!(
        AudioBuffer::try_from_mono(0, 1, [0.0]).err(),
        Some(GraphError::InvalidAudioBuffer)
    );
    assert_eq!(
        AudioBuffer::try_from_mono(44_100, 0, []).err(),
        Some(GraphError::InvalidAudioBuffer)
    );
    assert_eq!(
        AudioBuffer::try_from_channels(44_100, 1, std::iter::empty::<[f32; 1]>()).err(),
        Some(GraphError::InvalidAudioBuffer)
    );
    assert_eq!(
        AudioBuffer::try_from_channels(44_100, 1, [[0.0]; 33]).err(),
        Some(GraphError::InvalidAudioBuffer)
    );

    let buffer = AudioBuffer::try_from_stereo(44_100, 2, [0.25], [0.5, 0.75]).unwrap();
    assert_eq!(buffer.sample_rate(), 44_100);
    assert_eq!(buffer.number_of_channels(), 2);
    assert_eq!(buffer.channel_data(0), Some(&[0.25, 0.0][..]));
    assert_eq!(buffer.channel_data(1), Some(&[0.5, 0.75][..]));

    assert_eq!(
        AudioBuffer::try_from_frames(0, &[Frame::ZERO]).err(),
        Some(GraphError::InvalidAudioBuffer)
    );
    assert_eq!(
        AudioBuffer::try_from_frames(44_100, &[]).err(),
        Some(GraphError::InvalidAudioBuffer)
    );
    let buffer =
        AudioBuffer::try_from_frames(44_100, &[Frame::new(0.25, 0.5), Frame::new(0.75, 1.0)])
            .unwrap();
    assert_eq!(buffer.number_of_channels(), 2);
    assert_eq!(buffer.channel_data(0), Some(&[0.25, 0.75][..]));
    assert_eq!(buffer.channel_data(1), Some(&[0.5, 1.0][..]));
}

#[test]
fn offline_context_try_new_rejects_invalid_webaudio_arguments() {
    assert_eq!(
        OfflineAudioContext::try_new(0, 1, 44_100).err(),
        Some(GraphError::InvalidAudioBuffer)
    );
    assert_eq!(
        OfflineAudioContext::try_new(1, 0, 44_100).err(),
        Some(GraphError::InvalidAudioBuffer)
    );
    assert_eq!(
        OfflineAudioContext::try_new(1, 1, 2_999).err(),
        Some(GraphError::InvalidAudioBuffer)
    );

    let context = OfflineAudioContext::try_new(2, 4, 44_100).unwrap();
    assert_eq!(context.number_of_channels(), 2);
    assert_eq!(context.length(), 4);
    assert_eq!(context.sample_rate(), 44_100);
}

#[test]
fn offline_context_suspend_validates_webaudio_suspend_times() {
    let mut context = OfflineAudioContext::try_new(1, 4, 4_000).unwrap();

    assert_eq!(context.suspend(-0.0001), Err(GraphError::NegativeTime));
    assert_eq!(
        context.suspend(f64::NAN),
        Err(GraphError::InvalidAutomationValue)
    );
    assert_eq!(
        context.suspend(f64::INFINITY),
        Err(GraphError::InvalidAutomationValue)
    );
    assert_eq!(
        context.suspend(4.0 / 4_000.0),
        Err(GraphError::InvalidState)
    );

    context.suspend(1.0 / 4_000.0).unwrap();
    assert_eq!(
        context.suspend(1.0 / 4_000.0),
        Err(GraphError::InvalidState)
    );
}

#[test]
fn offline_context_resume_rejects_before_rendering_starts() {
    let mut context = OfflineAudioContext::try_new(1, 4, 4_000).unwrap();

    assert_eq!(context.resume(), Err(GraphError::InvalidState));
}

#[test]
fn audio_context_try_new_with_options_rejects_invalid_sample_rates() {
    assert_eq!(
        AudioContext::try_new_with_sample_rate(2_999).err(),
        Some(GraphError::InvalidAudioBuffer)
    );
    assert_eq!(
        AudioContext::try_new_with_sample_rate(384_001).err(),
        Some(GraphError::InvalidAudioBuffer)
    );

    let context = AudioContext::try_new_with_options(melody_bay::AudioContextOptions {
        sample_rate: Some(48_000),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(context.sample_rate(), 48_000);
}

#[test]
fn audio_context_exposes_webaudio_context_timing_surface() {
    let context = AudioContext::try_new_with_options(melody_bay::AudioContextOptions {
        sample_rate: Some(48_000),
        latency_hint: Some(melody_bay::AudioContextLatencyHint::Interactive),
    })
    .unwrap();

    assert_eq!(context.render_quantum_size(), 128);
    assert_eq!(
        context.latency_hint(),
        Some(melody_bay::AudioContextLatencyHint::Interactive)
    );
    assert_eq!(context.base_latency(), 0.0);
    assert_eq!(context.output_latency(), 0.0);

    let timestamp = context.get_output_timestamp();
    assert_eq!(timestamp.context_time, context.current_time());
    assert_eq!(timestamp.performance_time, context.current_time());

    let playback_context = AudioContext::try_new_with_options(melody_bay::AudioContextOptions {
        sample_rate: Some(44_100),
        latency_hint: Some(melody_bay::AudioContextLatencyHint::Seconds(0.25)),
    })
    .unwrap();
    assert_eq!(
        playback_context.latency_hint(),
        Some(melody_bay::AudioContextLatencyHint::Seconds(0.25))
    );
    assert_eq!(
        AudioContext::try_new_with_options(melody_bay::AudioContextOptions {
            sample_rate: Some(44_100),
            latency_hint: Some(melody_bay::AudioContextLatencyHint::Seconds(f64::INFINITY)),
        })
        .err(),
        Some(GraphError::InvalidAutomationValue)
    );
}

#[test]
fn offline_context_exposes_webaudio_options_and_destination_limits() {
    let context =
        OfflineAudioContext::try_new_with_options(melody_bay::OfflineAudioContextOptions {
            number_of_channels: 4,
            length: 128,
            sample_rate: 48_000,
            render_size_hint: None,
        })
        .unwrap();

    assert_eq!(context.number_of_channels(), 4);
    assert_eq!(context.length(), 128);
    assert_eq!(context.sample_rate(), 48_000);
    assert_eq!(context.render_quantum_size(), 128);
    assert_eq!(context.destination().max_channel_count(), 4);

    let hinted =
        OfflineAudioContext::try_new_with_options(melody_bay::OfflineAudioContextOptions {
            number_of_channels: 2,
            length: 128,
            sample_rate: 48_000,
            render_size_hint: Some(256),
        })
        .unwrap();
    assert_eq!(hinted.render_quantum_size(), 128);
    assert_eq!(
        OfflineAudioContext::try_new_with_options(melody_bay::OfflineAudioContextOptions {
            number_of_channels: 2,
            length: 128,
            sample_rate: 48_000,
            render_size_hint: Some(0),
        })
        .err(),
        Some(GraphError::InvalidAudioBuffer)
    );
}

#[test]
fn source_lifecycle_matches_webaudio_start_and_stop_rules() {
    let mut context = AudioContext::new();
    let source = context.create_oscillator();

    assert_eq!(source.try_stop(0.0), Err(GraphError::SourceNotStarted));
    assert_eq!(source.try_start(-0.1), Err(GraphError::NegativeTime));
    assert_eq!(
        source.try_start(f64::INFINITY),
        Err(GraphError::InvalidAutomationValue)
    );
    assert_eq!(
        source.try_start(f64::NAN),
        Err(GraphError::InvalidAutomationValue)
    );
    assert_eq!(source.try_start(0.0), Ok(()));
    assert_eq!(source.try_start(0.1), Err(GraphError::SourceAlreadyStarted));
    assert_eq!(source.try_stop(-0.1), Err(GraphError::NegativeTime));
    assert_eq!(
        source.try_stop(f64::INFINITY),
        Err(GraphError::InvalidAutomationValue)
    );
    assert_eq!(source.try_stop(0.5), Ok(()));
    assert_eq!(source.try_stop(0.25), Ok(()));

    let mut context = AudioContext::new();
    let source = context.create_oscillator();
    source.try_start(0.5).unwrap();
    assert_eq!(source.try_stop(0.25), Ok(()));

    let mut context = AudioContext::new();
    let source = context.create_buffer_source();
    assert_eq!(
        source.try_start_with_offset(-0.1, 0.0),
        Err(GraphError::NegativeTime)
    );
    assert_eq!(
        source.try_start_with_offset(0.0, -0.1),
        Err(GraphError::NegativeTime)
    );
    assert_eq!(
        source.try_start_with_offset_and_duration(0.0, 0.0, -0.1),
        Err(GraphError::NegativeTime)
    );
    assert_eq!(
        source.try_start_with_offset(f64::INFINITY, 0.0),
        Err(GraphError::InvalidAutomationValue)
    );
    assert_eq!(
        source.try_start_with_offset(0.0, f64::INFINITY),
        Err(GraphError::InvalidAutomationValue)
    );
    assert_eq!(
        source.try_start_with_offset_and_duration(0.0, 0.0, f64::INFINITY),
        Err(GraphError::InvalidAutomationValue)
    );

    let mut context = AudioContext::new();
    let source = context.create_buffer_source();
    source.try_start_with_offset(0.5, 0.0).unwrap();
    assert_eq!(source.try_stop(0.25), Ok(()));

    let mut context = AudioContext::new();
    let source = context.create_sound_data_source(TestSoundData {
        frames: vec![Frame::from_mono(0.25)],
    });
    assert_eq!(source.try_stop(0.0), Err(GraphError::SourceNotStarted));
    assert_eq!(source.try_start(-0.1), Err(GraphError::NegativeTime));
    assert_eq!(
        source.try_start(f64::INFINITY),
        Err(GraphError::InvalidAutomationValue)
    );
    assert_eq!(
        source.try_start(f64::NAN),
        Err(GraphError::InvalidAutomationValue)
    );
    assert_eq!(source.try_start(0.0), Ok(()));
    assert_eq!(source.try_start(0.1), Err(GraphError::SourceAlreadyStarted));
    assert_eq!(source.try_stop(-0.1), Err(GraphError::NegativeTime));
    assert_eq!(
        source.try_stop(f64::INFINITY),
        Err(GraphError::InvalidAutomationValue)
    );
    assert_eq!(source.try_stop(0.5), Ok(()));
    assert!(!source.ended());
}

#[test]
fn audio_buffer_source_buffer_assignment_is_one_shot() {
    let mut context = AudioContext::new();
    let source = context.create_buffer_source();
    let first = audio_buffer_from_mono(4, 1, [0.25]);
    let second = audio_buffer_from_mono(4, 1, [0.5]);

    assert_eq!(source.try_set_buffer(first.clone()), Ok(()));
    assert_eq!(source.buffer_value(), Some(first));
    assert_eq!(source.try_set_buffer(second), Err(GraphError::InvalidState));

    source.clear_buffer();
    assert_eq!(source.buffer_value(), None);
    assert_eq!(
        source.try_set_buffer(audio_buffer_from_mono(4, 1, [0.75])),
        Err(GraphError::InvalidState)
    );
}

#[test]
fn stop_before_start_marks_source_ended_after_rendering() {
    let mut context = AudioContext::new();
    let source = context.create_oscillator();
    source.try_start(0.5).unwrap();
    source.try_stop(0.25).unwrap();
    context.connect(&source, context.destination()).unwrap();

    process_live_graph(&context, 4, 4);

    assert!(source.ended());
}

#[test]
fn buffer_source_try_loop_range_validates_webaudio_loop_points() {
    let mut context = AudioContext::new();
    let source = context.create_buffer_source();

    assert_eq!(
        source.try_loop_range(0.0, f64::INFINITY),
        Err(GraphError::InvalidLoopRange)
    );

    assert_eq!(source.try_loop_range(-0.1, 1.0), Ok(()));
    assert_eq!(source.loop_range_value(), Some((-0.1, 1.0)));
    assert_eq!(source.try_loop_range(1.0, 0.5), Ok(()));
    assert_eq!(source.loop_range_value(), Some((1.0, 0.5)));
    assert_eq!(source.try_loop_range(0.25, 0.75), Ok(()));
    assert!(!source.looping_value());
    assert_eq!(source.loop_range_value(), Some((0.25, 0.75)));
    source.set_looping(true);
    assert!(source.looping_value());
}

#[test]
fn buffer_source_loop_point_setters_do_not_enable_looping() {
    let mut context = AudioContext::new();

    let range_source = context.create_buffer_source();
    range_source.try_loop_range(0.25, 0.75).unwrap();
    assert!(
        !range_source.looping_value(),
        "setting loopStart/loopEnd must not enable the loop boolean"
    );

    let start_end_source = context.create_buffer_source();
    start_end_source.try_loop_start(0.25).unwrap();
    start_end_source.try_loop_end(0.75).unwrap();
    assert!(
        !start_end_source.looping_value(),
        "setting loopStart and loopEnd separately must not enable the loop boolean"
    );

    start_end_source.set_looping(true);
    assert!(start_end_source.looping_value());
}

#[test]
fn scheduled_sources_are_silent_until_start_is_called() {
    let mut context = AudioContext::new();
    let oscillator = context.create_oscillator();
    context.connect(&oscillator, context.destination()).unwrap();
    let rendered = render_context_offline(&context, 4, 2).unwrap();
    assert_eq!(left_samples(&rendered), &[0.0, 0.0]);

    let mut context = AudioContext::new();
    let constant = context.create_constant_source();
    constant.offset().set_value(1.0).unwrap();
    context.connect(&constant, context.destination()).unwrap();
    let rendered = render_context_offline(&context, 4, 2).unwrap();
    assert_eq!(left_samples(&rendered), &[0.0, 0.0]);

    let mut context = AudioContext::new();
    let source = context.create_buffer_source();
    source
        .try_set_buffer(audio_buffer_from_mono(4, 2, [1.0, 1.0]))
        .unwrap();
    context.connect(&source, context.destination()).unwrap();
    let rendered = render_context_offline(&context, 4, 2).unwrap();
    assert_eq!(left_samples(&rendered), &[0.0, 0.0]);
}

#[test]
fn scheduled_sources_expose_rust_ended_state_after_rendering() {
    let mut context = AudioContext::new();
    let source =
        started_buffer_source_with_buffer(&mut context, audio_buffer_from_mono(3_000, 1, [1.0]));
    context.connect(&source, context.destination()).unwrap();

    assert!(!source.ended());
    process_live_graph(&context, 4, 2);
    assert!(source.ended());

    let mut context = AudioContext::new();
    let oscillator = context.create_oscillator();
    oscillator.try_start(0.0).unwrap();
    oscillator.try_stop(0.25).unwrap();
    context.connect(&oscillator, context.destination()).unwrap();
    assert!(!oscillator.ended());
    process_live_graph(&context, 4, 2);
    assert!(oscillator.ended());
}

#[test]
fn stop_after_source_has_ended_does_not_reschedule_output() {
    let mut context = AudioContext::new();
    let oscillator = context.create_oscillator();
    oscillator.try_start(0.0).unwrap();
    oscillator.try_stop(0.25).unwrap();
    context.connect(&oscillator, context.destination()).unwrap();
    process_live_graph(&context, 4, 2);
    assert!(oscillator.ended());

    oscillator.try_stop(1.0).unwrap();
    let rendered = render_context_offline(&context, 4, 2).unwrap();

    assert_eq!(left_samples(&rendered), &[0.0, 0.0]);

    let mut context = AudioContext::new();
    let constant = context.create_constant_source();
    constant.offset().set_value(1.0).unwrap();
    constant.try_start(0.0).unwrap();
    constant.try_stop(0.25).unwrap();
    context.connect(&constant, context.destination()).unwrap();
    process_live_graph(&context, 4, 2);
    assert!(constant.ended());

    constant.try_stop(1.0).unwrap();
    let rendered = render_context_offline(&context, 4, 2).unwrap();

    assert_eq!(left_samples(&rendered), &[0.0, 0.0]);
}

#[test]
fn dynamics_compressor_exposes_current_reduction() {
    let mut context = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut context);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let compressor = context.create_dynamics_compressor();
    compressor.threshold().set_value(-24.0).unwrap();
    compressor.knee().set_value(0.0).unwrap();
    compressor.ratio().set_value(12.0).unwrap();
    context.connect(&source, &compressor).unwrap();
    context.connect(&compressor, context.destination()).unwrap();

    assert_eq!(compressor.reduction(), 0.0);
    render_context_offline(&context, 4, 1).unwrap();
    assert!(compressor.reduction() < -1.0);
}

#[test]
fn offline_context_exposes_graph_building_entry_point() {
    let mut context = OfflineAudioContext::try_new(1, 4, 3_000).unwrap();
    let source = started_offline_buffer_source(&mut context);
    context.connect(&source, context.destination()).unwrap();
    let rendered = context.start_rendering().unwrap();

    assert_eq!(rendered.number_of_channels(), 1);
    assert_eq!(left_samples(&rendered), &[0.0, 0.0, 0.0, 0.0]);
}

#[test]
fn offline_context_exposes_graph_inspection_and_disconnect_helpers() {
    let mut context = OfflineAudioContext::try_new(1, 1, 3_000).unwrap();
    let source = started_offline_constant_source(&mut context);
    source.offset().set_value(0.5).unwrap();
    let gain = context.create_gain();
    let modulation = started_offline_constant_source(&mut context);
    modulation.offset().set_value(0.25).unwrap();
    gain.gain().set_value(0.0).unwrap();
    context.connect(&source, &gain).unwrap();
    context.connect(&gain, context.destination()).unwrap();
    context.connect_param(&modulation, gain.gain()).unwrap();

    let info = context.node_info(&gain).unwrap();
    assert_eq!(info.number_of_inputs, 1);
    assert_eq!(info.number_of_outputs, 1);

    context.disconnect_param_outputs(&modulation).unwrap();
    context.disconnect_outputs(&source).unwrap();
    let rendered = context.start_rendering().unwrap();

    assert_eq!(left_samples(&rendered), &[0.0]);
}

#[test]
fn offline_context_preserves_requested_multichannel_destination_output() {
    let mut context = OfflineAudioContext::try_new(4, 1, 3_000).unwrap();
    let merger = context.try_create_channel_merger(4).unwrap();
    for (input, value) in [0.125, 0.25, 0.5, 1.0].into_iter().enumerate() {
        let source = started_offline_constant_source(&mut context);
        source.offset().set_value(value).unwrap();
        context
            .connect_with_indices(&source, 0, &merger, input)
            .unwrap();
    }
    context.connect(&merger, context.destination()).unwrap();

    let rendered = context.start_rendering().unwrap();

    assert_eq!(rendered.number_of_channels(), 4);
    assert_eq!(rendered.channel_data(0), Some(&[0.125][..]));
    assert_eq!(rendered.channel_data(1), Some(&[0.25][..]));
    assert_eq!(rendered.channel_data(2), Some(&[0.5][..]));
    assert_eq!(rendered.channel_data(3), Some(&[1.0][..]));
}

#[test]
fn offline_context_uses_its_sample_rate_for_graph_validation() {
    let mut context = OfflineAudioContext::try_new(1, 1, 3_000).unwrap();
    let convolver = context.create_convolver();

    assert_eq!(
        convolver.try_buffer(audio_buffer_from_mono(3_000, 1, [1.0])),
        Ok(())
    );
    assert_eq!(
        convolver.try_buffer(audio_buffer_from_mono(44_100, 1, [1.0])),
        Err(GraphError::InvalidConvolverBuffer)
    );
}

#[test]
fn buffer_source_preserves_more_than_two_channels_for_splitter_outputs() {
    let mut context = OfflineAudioContext::try_new(1, 1, 3_000).unwrap();
    let source = started_offline_buffer_source_with_buffer(
        &mut context,
        audio_buffer_from_channels(4, 1, [[0.125], [0.25], [0.5], [1.0]]),
    );
    let splitter = context.try_create_channel_splitter(4).unwrap();
    context.connect(&source, &splitter).unwrap();
    context
        .connect_with_indices(&splitter, 3, context.destination(), 0)
        .unwrap();

    let rendered = context.start_rendering().unwrap();

    assert_eq!(rendered.number_of_channels(), 1);
    assert_eq!(rendered.channel_data(0), Some(&[1.0][..]));
}

#[test]
fn indexed_audio_connections_route_to_requested_inputs_and_outputs() {
    let mut context = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut context,
        audio_buffer_from_stereo(4, 1, [0.25], [0.75]),
    );
    let splitter = context.try_create_channel_splitter(2).unwrap();
    let merger = context.try_create_channel_merger(2).unwrap();

    context.connect(&source, &splitter).unwrap();
    context
        .connect_with_indices(&splitter, 1, &merger, 0)
        .unwrap();
    context
        .connect_with_indices(&splitter, 0, &merger, 1)
        .unwrap();
    context.connect(&merger, context.destination()).unwrap();

    let rendered = render_context_offline(&context, 4, 1).unwrap();
    assert_eq!(rendered.channel_data(0), Some(&[0.75][..]));
    assert_eq!(rendered.channel_data(1), Some(&[0.25][..]));

    context
        .disconnect_with_indices(&splitter, 1, &merger, 0)
        .unwrap();
    let rendered = render_context_offline(&context, 4, 1).unwrap();
    assert_eq!(rendered.channel_data(0), Some(&[0.0][..]));
    assert_eq!(rendered.channel_data(1), Some(&[0.25][..]));
}

#[test]
fn indexed_splitter_and_merger_do_not_create_proxy_nodes() {
    let mut context = AudioContext::new();
    let splitter = context.try_create_channel_splitter(2).unwrap();
    let merger = context.try_create_channel_merger(2).unwrap();

    context
        .connect(&splitter, &merger)
        .expect("typed splitter handle connects to typed merger handle");
}

#[test]
fn try_create_channel_splitter_and_merger_validate_webaudio_channel_counts() {
    let mut context = AudioContext::new();

    assert_eq!(
        context.try_create_channel_splitter(0).err(),
        Some(GraphError::InvalidChannelCount)
    );
    assert_eq!(
        context.try_create_channel_splitter(33).err(),
        Some(GraphError::InvalidChannelCount)
    );
    assert_eq!(
        context.try_create_channel_merger(0).err(),
        Some(GraphError::InvalidChannelCount)
    );
    assert_eq!(
        context.try_create_channel_merger(33).err(),
        Some(GraphError::InvalidChannelCount)
    );

    let splitter = context.try_create_channel_splitter(32).unwrap();
    let merger = context.try_create_channel_merger(32).unwrap();
    assert_eq!(context.node_info(&splitter).unwrap().number_of_outputs, 32);
    assert_eq!(context.node_info(&merger).unwrap().number_of_inputs, 32);
}

#[test]
fn channel_splitter_and_merger_use_webaudio_channel_config_defaults() {
    let mut context = AudioContext::new();
    let splitter = context.try_create_channel_splitter(4).unwrap();
    let merger = context.try_create_channel_merger(4).unwrap();

    let splitter_info = context.node_info(&splitter).unwrap();
    assert_eq!(splitter_info.channel_count, 4);
    assert_eq!(splitter_info.channel_count_mode, ChannelCountMode::Explicit);
    assert_eq!(
        splitter_info.channel_interpretation,
        ChannelInterpretation::Discrete
    );
    assert_eq!(
        splitter.try_set_channel_config(
            2,
            ChannelCountMode::Explicit,
            ChannelInterpretation::Discrete,
        ),
        Err(GraphError::InvalidChannelCount)
    );

    let merger_info = context.node_info(&merger).unwrap();
    assert_eq!(merger_info.channel_count, 1);
    assert_eq!(merger_info.channel_count_mode, ChannelCountMode::Explicit);
    assert_eq!(
        merger.try_set_channel_config(
            2,
            ChannelCountMode::Explicit,
            ChannelInterpretation::Speakers,
        ),
        Err(GraphError::InvalidChannelCount)
    );
}

#[test]
fn try_create_delay_validates_webaudio_max_delay_time() {
    let mut context = AudioContext::new();

    assert_eq!(
        context.try_create_delay(-0.001).err(),
        Some(GraphError::InvalidDelayTime)
    );
    assert_eq!(
        context.try_create_delay(0.0).err(),
        Some(GraphError::InvalidDelayTime)
    );
    assert_eq!(
        context.try_create_delay(180.0).err(),
        Some(GraphError::InvalidDelayTime)
    );
    assert_eq!(
        context.try_create_delay(180.001).err(),
        Some(GraphError::InvalidDelayTime)
    );

    let delay = context.try_create_delay(2.5).unwrap();
    assert_eq!(delay.delay_time_value(), 0.0);
    assert_eq!(delay.max_delay_time_value(), 2.5);

    let mut offline = OfflineAudioContext::try_new(2, 16, 44_100).unwrap();
    assert_eq!(
        offline.try_create_delay(-1.0).err(),
        Some(GraphError::InvalidDelayTime)
    );
}

#[test]
fn try_set_channel_config_rejects_invalid_channel_counts() {
    let mut context = AudioContext::new();
    let gain = context.create_gain();

    assert_eq!(
        gain.try_set_channel_config(
            0,
            ChannelCountMode::Explicit,
            ChannelInterpretation::Speakers,
        ),
        Err(GraphError::InvalidChannelCount)
    );
    assert_eq!(
        gain.try_set_channel_config(
            33,
            ChannelCountMode::Explicit,
            ChannelInterpretation::Speakers,
        ),
        Err(GraphError::InvalidChannelCount)
    );

    gain.try_set_channel_config(
        4,
        ChannelCountMode::Explicit,
        ChannelInterpretation::Discrete,
    )
    .unwrap();
    let info = context.node_info(&gain).unwrap();
    assert_eq!(info.channel_count, 4);
    assert_eq!(info.channel_count_mode, ChannelCountMode::Explicit);
    assert_eq!(info.channel_interpretation, ChannelInterpretation::Discrete);
}

#[test]
fn try_set_channel_config_enforces_node_specific_webaudio_constraints() {
    let mut context = AudioContext::new();
    let convolver = context.create_convolver();
    let compressor = context.create_dynamics_compressor();
    let panner = context.create_panner();
    let stereo_panner = context.create_stereo_panner();
    let splitter = context.try_create_channel_splitter(2).unwrap();
    let merger = context.try_create_channel_merger(2).unwrap();

    macro_rules! assert_fixed_channel_node_rejects_invalid_config {
        ($node:expr) => {
            assert_eq!(
                $node.try_set_channel_config(
                    3,
                    ChannelCountMode::Explicit,
                    ChannelInterpretation::Speakers,
                ),
                Err(GraphError::InvalidChannelCount)
            );
            assert_eq!(
                $node.try_set_channel_config(
                    2,
                    ChannelCountMode::Max,
                    ChannelInterpretation::Speakers,
                ),
                Err(GraphError::InvalidChannelCount)
            );
        };
    }
    assert_fixed_channel_node_rejects_invalid_config!(convolver);
    assert_fixed_channel_node_rejects_invalid_config!(compressor);
    assert_fixed_channel_node_rejects_invalid_config!(panner);
    assert_fixed_channel_node_rejects_invalid_config!(stereo_panner);

    assert_eq!(
        splitter.try_set_channel_config(
            2,
            ChannelCountMode::Explicit,
            ChannelInterpretation::Speakers,
        ),
        Err(GraphError::InvalidChannelCount)
    );
    assert_eq!(
        merger.try_set_channel_config(2, ChannelCountMode::Max, ChannelInterpretation::Discrete,),
        Err(GraphError::InvalidChannelCount)
    );

    assert_eq!(
        context.destination().try_set_channel_config(
            2,
            ChannelCountMode::Max,
            ChannelInterpretation::Speakers,
        ),
        Err(GraphError::InvalidChannelCount)
    );
    assert_eq!(
        context.destination().try_set_channel_config(
            2,
            ChannelCountMode::Explicit,
            ChannelInterpretation::Discrete,
        ),
        Err(GraphError::InvalidChannelCount)
    );
}

#[test]
fn fixed_channel_nodes_use_webaudio_clamped_max_defaults() {
    let mut context = AudioContext::new();
    let convolver = context.create_convolver();
    let compressor = context.create_dynamics_compressor();
    let panner = context.create_panner();
    let stereo_panner = context.create_stereo_panner();

    macro_rules! assert_clamped_max_defaults {
        ($node:expr) => {
            assert_eq!($node.channel_count(), 2);
            assert_eq!($node.channel_count_mode(), ChannelCountMode::ClampedMax);
            assert_eq!(
                $node.channel_interpretation(),
                ChannelInterpretation::Speakers
            );
        };
    }

    assert_clamped_max_defaults!(convolver);
    assert_clamped_max_defaults!(compressor);
    assert_clamped_max_defaults!(panner);
    assert_clamped_max_defaults!(stereo_panner);
}

#[test]
fn offline_destination_channel_count_is_fixed_by_context_options() {
    let context = OfflineAudioContext::try_new(4, 16, 44_100).unwrap();

    assert_eq!(
        context.destination().try_set_channel_config(
            2,
            ChannelCountMode::Explicit,
            ChannelInterpretation::Speakers,
        ),
        Err(GraphError::InvalidChannelCount)
    );

    context
        .destination()
        .try_set_channel_config(
            4,
            ChannelCountMode::Explicit,
            ChannelInterpretation::Speakers,
        )
        .unwrap();
}

#[test]
fn audio_worklet_node_uses_rust_processor_trait_and_validates_options() {
    struct Doubler;

    impl melody_bay::AudioWorkletProcessor for Doubler {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            let Some(input) = worklet_input_channel(inputs, 0, 0) else {
                return true;
            };
            for port in outputs {
                for output in port {
                    for (sample, input) in output.iter_mut().zip(input.iter().copied()) {
                        *sample = input * 2.0;
                    }
                }
            }
            true
        }
    }

    let mut context = AudioContext::new();
    assert_eq!(
        context
            .try_create_audio_worklet_node(
                Doubler,
                melody_bay::AudioWorkletNodeOptions {
                    number_of_inputs: 0,
                    number_of_outputs: 0,
                    output_channel_count: Some(vec![]),
                    ..Default::default()
                },
            )
            .err(),
        Some(GraphError::InvalidAudioWorkletOptions)
    );
    let input_only = context
        .try_create_audio_worklet_node(
            Doubler,
            melody_bay::AudioWorkletNodeOptions {
                number_of_inputs: 1,
                number_of_outputs: 0,
                output_channel_count: Some(vec![]),
                ..Default::default()
            },
        )
        .expect("input-only worklet options are valid");
    assert_eq!(
        context
            .node_info(&input_only)
            .expect("input-only worklet info")
            .number_of_outputs,
        0
    );
    let input_only_with_default_channel_count = context
        .try_create_audio_worklet_node(
            Doubler,
            melody_bay::AudioWorkletNodeOptions {
                number_of_inputs: 1,
                number_of_outputs: 0,
                ..Default::default()
            },
        )
        .expect("default output channel count is ignored for input-only worklets");
    assert_eq!(
        context
            .node_info(&input_only_with_default_channel_count)
            .expect("input-only worklet info")
            .number_of_outputs,
        0
    );
    assert_eq!(
        context
            .try_create_audio_worklet_node(
                Doubler,
                melody_bay::AudioWorkletNodeOptions {
                    number_of_inputs: 33,
                    number_of_outputs: 1,
                    output_channel_count: Some(vec![2]),
                    ..Default::default()
                },
            )
            .err(),
        Some(GraphError::InvalidAudioWorkletOptions)
    );
    assert_eq!(
        context
            .try_create_audio_worklet_node(
                Doubler,
                melody_bay::AudioWorkletNodeOptions {
                    parameter_descriptors: vec![melody_bay::AudioWorkletParameterDescriptor {
                        name: "depth".to_string(),
                        default_value: 0.5,
                        min_value: 1.0,
                        max_value: 0.0,
                        automation_rate: melody_bay::AutomationRate::ARate,
                    }],
                    ..Default::default()
                },
            )
            .err(),
        Some(GraphError::InvalidAudioWorkletOptions)
    );
    assert_eq!(
        context
            .try_create_audio_worklet_node(
                Doubler,
                melody_bay::AudioWorkletNodeOptions {
                    number_of_inputs: 1,
                    number_of_outputs: 33,
                    output_channel_count: Some(vec![1; 33]),
                    ..Default::default()
                },
            )
            .err(),
        Some(GraphError::InvalidAudioWorkletOptions)
    );
    assert_eq!(
        context
            .try_create_audio_worklet_node(
                Doubler,
                melody_bay::AudioWorkletNodeOptions {
                    parameter_descriptors: vec![melody_bay::AudioWorkletParameterDescriptor {
                        name: "depth".to_string(),
                        default_value: 2.0,
                        min_value: 0.0,
                        max_value: 1.0,
                        automation_rate: melody_bay::AutomationRate::ARate,
                    }],
                    ..Default::default()
                },
            )
            .err(),
        Some(GraphError::InvalidAudioWorkletOptions)
    );
    assert_eq!(
        context
            .try_create_audio_worklet_node(
                Doubler,
                melody_bay::AudioWorkletNodeOptions {
                    parameter_data: [("unknown".to_string(), 0.5)].into_iter().collect(),
                    ..Default::default()
                },
            )
            .err(),
        Some(GraphError::InvalidAudioWorkletOptions)
    );
    assert_eq!(
        context
            .try_create_audio_worklet_node(
                Doubler,
                melody_bay::AudioWorkletNodeOptions {
                    parameter_descriptors: vec![
                        melody_bay::AudioWorkletParameterDescriptor {
                            name: "depth".to_string(),
                            default_value: 0.5,
                            min_value: 0.0,
                            max_value: 1.0,
                            automation_rate: melody_bay::AutomationRate::ARate,
                        },
                        melody_bay::AudioWorkletParameterDescriptor {
                            name: "depth".to_string(),
                            default_value: 0.25,
                            min_value: 0.0,
                            max_value: 1.0,
                            automation_rate: melody_bay::AutomationRate::KRate,
                        },
                    ],
                    ..Default::default()
                },
            )
            .err(),
        Some(GraphError::InvalidAudioWorkletOptions)
    );
    assert_eq!(
        context
            .try_create_audio_worklet_node(
                Doubler,
                melody_bay::AudioWorkletNodeOptions {
                    number_of_outputs: 2,
                    output_channel_count: Some(vec![1]),
                    ..Default::default()
                },
            )
            .err(),
        Some(GraphError::InvalidAudioWorkletOptions)
    );

    let default_channel_counts = context
        .try_create_audio_worklet_node(
            Doubler,
            melody_bay::AudioWorkletNodeOptions {
                number_of_outputs: 2,
                ..Default::default()
            },
        )
        .expect("omitted output channel counts default each output to mono");
    let info = context
        .node_info(&default_channel_counts)
        .expect("default-channel worklet info");
    assert_eq!(info.number_of_outputs, 2);

    let source = {
        let source = started_constant_source(&mut context);
        source.offset().set_value(0.25).unwrap();
        source
    };
    let worklet = context
        .try_create_audio_worklet_node(Doubler, melody_bay::AudioWorkletNodeOptions::default())
        .unwrap();
    context.connect(&source, &worklet).unwrap();
    context.connect(&worklet, context.destination()).unwrap();

    let rendered = render_context_offline(&context, 4, 1).unwrap();

    assert_eq!(rendered.channel_data(0), Some(&[0.5][..]));
}

#[test]
fn audio_worklet_node_exposes_named_parameters_and_processor_options() {
    struct ScaledDepth;

    impl melody_bay::AudioWorkletProcessor for ScaledDepth {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            let depth = context.parameters.get("depth").copied().unwrap_or(0.0);
            let scale = context
                .processor_options
                .get("scale")
                .and_then(|value| value.parse::<f32>().ok())
                .unwrap_or(1.0);
            let Some(input) = worklet_input_channel(inputs, 0, 0) else {
                return true;
            };
            let output = worklet_output_channel_mut(outputs, 0, 0).expect("first output channel");
            for (sample, input) in output.iter_mut().zip(input.iter().copied()) {
                *sample = input * depth * scale;
            }
            true
        }
    }

    let mut context = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut context);
        source.offset().set_value(0.5).unwrap();
        source
    };
    let modulation = {
        let source = started_constant_source(&mut context);
        source.offset().set_value(0.25).unwrap();
        source
    };
    let worklet = context
        .try_create_audio_worklet_node(
            ScaledDepth,
            melody_bay::AudioWorkletNodeOptions {
                parameter_descriptors: vec![melody_bay::AudioWorkletParameterDescriptor {
                    name: "depth".to_string(),
                    default_value: 0.25,
                    min_value: 0.0,
                    max_value: 1.0,
                    automation_rate: melody_bay::AutomationRate::ARate,
                }],
                parameter_data: [("depth".to_string(), 0.5)].into_iter().collect(),
                processor_options: [("scale".to_string(), "2.0".to_string())]
                    .into_iter()
                    .collect(),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(worklet.parameter("depth").unwrap().value(), 0.5);
    worklet.parameter("depth").unwrap().set_value(0.25).unwrap();
    context
        .connect_param(modulation, worklet.param("depth").unwrap())
        .unwrap();
    context.connect(&source, &worklet).unwrap();
    context.connect(&worklet, context.destination()).unwrap();

    let rendered = render_context_offline(&context, 4, 1).unwrap();

    assert_eq!(rendered.channel_data(0), Some(&[0.5][..]));
}

#[test]
fn audio_worklet_quantum_context_exposes_a_rate_parameter_arrays() {
    struct ParameterProbe;

    impl melody_bay::AudioWorkletProcessor for ParameterProbe {
        fn process(
            &mut self,
            _inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            let values = context
                .parameter_values
                .get("depth")
                .expect("depth parameter array exists");
            assert_eq!(values.len(), 128);
            let output = worklet_output_channel_mut(outputs, 0, 0).expect("first output channel");
            for (output, value) in output.iter_mut().zip(values.iter().copied()) {
                *output = value;
            }
            true
        }
    }

    let mut context = AudioContext::new();
    let worklet = context
        .try_create_audio_worklet_node(
            ParameterProbe,
            melody_bay::AudioWorkletNodeOptions {
                parameter_descriptors: vec![melody_bay::AudioWorkletParameterDescriptor {
                    name: "depth".to_string(),
                    default_value: 0.0,
                    min_value: 0.0,
                    max_value: 1.0,
                    automation_rate: melody_bay::AutomationRate::ARate,
                }],
                ..Default::default()
            },
        )
        .unwrap();
    let depth = worklet.parameter("depth").unwrap();
    depth.set_value_at_time(0.0, 0.0).unwrap();
    depth.linear_ramp_to_value_at_time(1.0, 0.75).unwrap();
    context.connect(&worklet, context.destination()).unwrap();

    let rendered = render_context_offline(&context, 4, 4).unwrap();

    assert_eq!(
        rendered.channel_data(0),
        Some(&[0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0][..])
    );
}

#[test]
fn audio_worklet_node_default_factory_uses_default_options() {
    struct ConstantWorklet;

    impl melody_bay::AudioWorkletProcessor for ConstantWorklet {
        fn process(
            &mut self,
            _inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            for port in outputs {
                for output in port {
                    output.fill(0.375);
                }
            }
            true
        }
    }

    let mut context = AudioContext::new();
    let worklet = context.create_audio_worklet_node(ConstantWorklet);
    context.connect(&worklet, context.destination()).unwrap();

    let rendered = render_context_offline(&context, 4, 1).unwrap();

    assert_eq!(rendered.channel_data(0), Some(&[0.375][..]));
    assert_eq!(rendered.channel_data(1), Some(&[0.375][..]));
}

#[test]
fn indexed_param_connections_use_requested_source_output() {
    let mut context = AudioContext::new();
    let carrier = {
        let source = started_constant_source(&mut context);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let modulation = started_buffer_source_with_buffer(
        &mut context,
        audio_buffer_from_stereo(4, 1, [0.25], [0.75]),
    );
    let splitter = context.try_create_channel_splitter(2).unwrap();
    let gain = context.create_gain();
    gain.gain().set_value(0.0).unwrap();

    context.connect(&modulation, &splitter).unwrap();
    context
        .connect_param_from_output(&splitter, 1, gain.gain())
        .unwrap();
    context.connect(&carrier, &gain).unwrap();
    context.connect(&gain, context.destination()).unwrap();

    let rendered = render_context_offline(&context, 4, 1).unwrap();
    assert_eq!(left_samples(&rendered), &[0.75]);

    context
        .disconnect_param_from_output(&splitter, 1, gain.gain())
        .unwrap();
    let rendered = render_context_offline(&context, 4, 1).unwrap();
    assert_eq!(left_samples(&rendered), &[0.0]);
}

#[test]
fn audio_param_exposes_metadata_and_clamps_to_nominal_range() {
    let mut context = AudioContext::new();
    let gain = context.create_gain();
    let param = gain.gain();

    assert_eq!(param.default_value(), 1.0);
    assert_eq!(param.min_value(), f32::NEG_INFINITY);
    assert_eq!(param.max_value(), f32::INFINITY);
    assert_eq!(param.automation_rate(), melody_bay::AutomationRate::ARate);

    param.set_value_at_time(0.5, 0.0).unwrap();
    param.linear_ramp_to_value_at_time(0.0, 1.0).unwrap();

    assert_eq!(param.value(), 0.5);
    assert_eq!(param.value_at(1.0), 0.0);
}

#[test]
fn audio_param_handle_sets_supported_automation_rates_and_rejects_fixed_rates() {
    let mut context = AudioContext::new();
    let gain = context.create_gain();
    let gain_param = gain.gain();

    assert_eq!(
        gain_param.automation_rate(),
        melody_bay::AutomationRate::ARate
    );
    assert_eq!(
        gain_param.try_set_automation_rate(melody_bay::AutomationRate::KRate),
        Ok(())
    );
    assert_eq!(
        gain_param.automation_rate(),
        melody_bay::AutomationRate::KRate
    );
    assert_eq!(
        gain_param.try_set_automation_rate(melody_bay::AutomationRate::ARate),
        Ok(())
    );
    assert_eq!(
        gain_param.automation_rate(),
        melody_bay::AutomationRate::ARate
    );

    let compressor = context.create_dynamics_compressor();
    assert_eq!(
        compressor
            .threshold()
            .try_set_automation_rate(melody_bay::AutomationRate::ARate),
        Err(GraphError::InvalidAutomationRate)
    );

    let source = context.create_buffer_source();
    assert_eq!(
        source
            .playback_rate()
            .try_set_automation_rate(melody_bay::AutomationRate::ARate),
        Err(GraphError::InvalidAutomationRate)
    );
}

#[test]
fn parameterized_nodes_expose_named_audio_param_handles() {
    let mut context = AudioContext::new();
    let oscillator = context.create_oscillator();
    let constant = context.create_constant_source();
    let buffer = context.create_buffer_source();
    let gain = context.create_gain();
    let pan = context.create_stereo_panner();
    let filter = context.create_biquad_filter();
    let delay = context.create_delay();
    let compressor = context.create_dynamics_compressor();
    let panner = context.create_panner();

    oscillator
        .parameter("frequency")
        .unwrap()
        .set_value(220.0)
        .unwrap();
    constant
        .parameter("offset")
        .unwrap()
        .set_value(0.25)
        .unwrap();
    buffer
        .parameter("playbackRate")
        .unwrap()
        .set_value(0.5)
        .unwrap();
    gain.parameter("gain").unwrap().set_value(0.75).unwrap();
    pan.parameter("pan").unwrap().set_value(-0.25).unwrap();
    filter.parameter("Q").unwrap().set_value(0.5).unwrap();
    delay
        .parameter("delayTime")
        .unwrap()
        .set_value(0.125)
        .unwrap();
    compressor
        .parameter("threshold")
        .unwrap()
        .set_value(-18.0)
        .unwrap();
    panner
        .parameter("positionX")
        .unwrap()
        .set_value(1.0)
        .unwrap();

    assert_eq!(oscillator.frequency().value(), 220.0);
    assert_eq!(constant.offset().value(), 0.25);
    assert_eq!(buffer.playback_rate().value(), 0.5);
    assert_eq!(gain.gain().value(), 0.75);
    assert_eq!(pan.pan().value(), -0.25);
    assert_eq!(filter.q().value(), 0.5);
    assert_eq!(delay.delay_time().value(), 0.125);
    assert_eq!(compressor.threshold().value(), -18.0);
    assert_eq!(panner.position_x().value(), 1.0);
    assert!(gain.parameter("missing").is_none());
}

#[test]
fn gain_node_exposes_live_webaudio_audio_param_handle() {
    let mut context = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut context);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let gain = context.create_gain();
    let gain_param = gain.gain();

    assert_eq!(gain_param.default_value(), 1.0);
    assert_eq!(gain_param.value(), 1.0);

    gain_param.set_value(0.25).unwrap();
    context.connect(&source, &gain).unwrap();
    context.connect(&gain, context.destination()).unwrap();

    let rendered = render_context_offline(&context, 4, 1).unwrap();

    assert_eq!(left_samples(&rendered), &[0.25]);
    assert_eq!(gain.gain().value(), 0.25);
}

#[test]
fn gain_node_exposes_node_owned_channel_config() {
    let mut context = AudioContext::new();
    let gain = context.create_gain();

    assert_eq!(gain.channel_count(), 2);
    assert_eq!(gain.channel_count_mode(), ChannelCountMode::Max);
    assert_eq!(
        gain.channel_interpretation(),
        ChannelInterpretation::Speakers
    );

    gain.try_set_channel_config(
        1,
        ChannelCountMode::Explicit,
        ChannelInterpretation::Speakers,
    )
    .unwrap();
    assert_eq!(gain.channel_count(), 1);
    assert_eq!(gain.channel_count_mode(), ChannelCountMode::Explicit);
    assert_eq!(
        gain.channel_interpretation(),
        ChannelInterpretation::Speakers
    );
    assert_eq!(
        gain.try_set_channel_config(
            0,
            ChannelCountMode::Explicit,
            ChannelInterpretation::Speakers,
        ),
        Err(GraphError::InvalidChannelCount)
    );

    let source = started_buffer_source_with_buffer(
        &mut context,
        audio_buffer_from_stereo(4, 1, [0.25], [0.75]),
    );
    context.connect(source, &gain).unwrap();
    context.connect(&gain, context.destination()).unwrap();

    let rendered = render_context_offline(&context, 4, 1).unwrap();
    assert_eq!(left_samples(&rendered), &[0.5]);
}

#[test]
fn processing_nodes_expose_node_owned_channel_config() {
    let mut context = AudioContext::new();
    let filter = context.create_biquad_filter();
    let delay = context.create_delay();
    let stereo = context.create_stereo_panner();
    let panner = context.create_panner();

    filter
        .try_set_channel_config(
            4,
            ChannelCountMode::Explicit,
            ChannelInterpretation::Discrete,
        )
        .unwrap();
    assert_eq!(filter.channel_count(), 4);
    assert_eq!(filter.channel_count_mode(), ChannelCountMode::Explicit);
    assert_eq!(
        filter.channel_interpretation(),
        ChannelInterpretation::Discrete
    );

    delay
        .try_set_channel_config(
            1,
            ChannelCountMode::Explicit,
            ChannelInterpretation::Speakers,
        )
        .unwrap();
    assert_eq!(delay.channel_count(), 1);
    assert_eq!(delay.channel_count_mode(), ChannelCountMode::Explicit);

    stereo
        .try_set_channel_config(
            2,
            ChannelCountMode::ClampedMax,
            ChannelInterpretation::Speakers,
        )
        .unwrap();
    assert_eq!(stereo.channel_count(), 2);
    assert_eq!(stereo.channel_count_mode(), ChannelCountMode::ClampedMax);
    assert_eq!(
        stereo.try_set_channel_config(2, ChannelCountMode::Max, ChannelInterpretation::Speakers,),
        Err(GraphError::InvalidChannelCount)
    );

    assert_eq!(
        panner.try_set_channel_config(
            3,
            ChannelCountMode::ClampedMax,
            ChannelInterpretation::Speakers,
        ),
        Err(GraphError::InvalidChannelCount)
    );
}

#[test]
fn audio_param_set_value_preserves_future_automation_events() {
    let mut context = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut context);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let gain = context.create_gain();
    let gain_param = gain.gain();

    gain_param.set_value_at_time(0.5, 0.0).unwrap();
    gain_param.set_value_at_time(0.75, 0.5).unwrap();
    gain_param.set_value(0.25).unwrap();
    assert_eq!(gain_param.default_value(), 1.0);
    context.connect(&source, &gain).unwrap();
    context.connect(&gain, context.destination()).unwrap();

    let rendered = render_context_offline(&context, 4, 4).unwrap();

    assert_eq!(left_samples(&rendered), &[0.25, 0.25, 0.75, 0.75]);
}

#[test]
fn audio_param_handle_schedules_webaudio_automation_events() {
    let mut context = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut context);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let gain = context.create_gain();
    let gain_param = gain.gain();

    gain_param.set_value_at_time(0.0, 0.0).unwrap();
    gain_param.linear_ramp_to_value_at_time(1.0, 1.0).unwrap();
    gain_param.cancel_and_hold_at_time(0.5).unwrap();
    context.connect(&source, &gain).unwrap();
    context.connect(&gain, context.destination()).unwrap();

    let rendered = render_context_offline(&context, 4, 4).unwrap();

    assert_eq!(left_samples(&rendered), &[0.0, 0.25, 0.5, 0.5]);
    assert_close(gain_param.value_at(1.0), 0.5);
}

#[test]
fn audio_param_preserves_same_time_automation_event_insertion_order() {
    let mut context = AudioContext::new();
    let gain = context.create_gain();
    let param = gain.gain();

    param.set_value_at_time(0.0, 0.0).unwrap();
    param.linear_ramp_to_value_at_time(1.0, 1.0).unwrap();
    param.linear_ramp_to_value_at_time(0.5, 1.0).unwrap();

    assert_close(param.value_at(0.5), 0.5);
}

#[test]
fn audio_param_allows_value_curve_events_at_the_same_start_time() {
    let mut context = AudioContext::new();
    let gain = context.create_gain();
    let param = gain.gain();

    param.set_value_curve_at_time([0.0, 0.5], 1.0, 1.0).unwrap();
    param.set_value_curve_at_time([1.0, 0.0], 1.0, 1.0).unwrap();

    assert_close(param.value_at(1.5), 0.25);
}

#[test]
fn audio_param_value_curve_end_time_allows_following_event() {
    let mut context = AudioContext::new();
    let gain = context.create_gain();
    let param = gain.gain();

    param.set_value_curve_at_time([0.0, 1.0], 0.0, 1.0).unwrap();
    param.set_value_at_time(0.25, 1.0).unwrap();

    assert_close(param.value_at(0.999), 0.999);
    assert_close(param.value_at(1.0), 0.25);
}

#[test]
fn audio_param_handle_rejects_value_curve_automation_overlaps() {
    let mut context = AudioContext::new();
    let gain = context.create_gain();
    let param = gain.gain();

    param.set_value_curve_at_time([0.0, 1.0], 1.0, 1.0).unwrap();
    assert_eq!(
        param.set_value_at_time(0.5, 1.5),
        Err(GraphError::InvalidAutomationValue)
    );

    let gain = context.create_gain();
    let param = gain.gain();
    param.set_value_at_time(0.5, 1.5).unwrap();
    assert_eq!(
        param.set_value_curve_at_time([0.0, 1.0], 1.0, 1.0),
        Err(GraphError::InvalidAutomationValue)
    );
}

#[test]
fn audio_param_handle_cancel_and_hold_stops_active_value_curve() {
    let mut context = AudioContext::new();
    let gain = context.create_gain();
    let param = gain.gain();

    param.set_value_curve_at_time([0.0, 1.0], 0.0, 1.0).unwrap();
    param.cancel_and_hold_at_time(0.5).unwrap();

    assert_close(param.value_at(0.5), 0.5);
    assert_close(param.value_at(0.25), 0.25);
    assert_close(param.value_at(0.75), 0.5);
    assert_close(param.value_at(1.25), 0.5);
}

#[test]
fn audio_param_handle_cancel_scheduled_values_removes_active_value_curve() {
    let mut context = AudioContext::new();
    let gain = context.create_gain();
    let param = gain.gain();

    param.set_value(0.25).unwrap();
    param.set_value_curve_at_time([0.0, 1.0], 0.0, 1.0).unwrap();
    param.cancel_scheduled_values(0.5).unwrap();

    assert_close(param.value_at(0.25), 0.25);
    assert_close(param.value_at(0.75), 0.25);
}

#[test]
fn audio_param_handle_cancel_scheduled_values_keeps_value_curve_ending_at_cancel_time() {
    let mut context = AudioContext::new();
    let gain = context.create_gain();
    let param = gain.gain();

    param.set_value_curve_at_time([0.0, 1.0], 0.0, 1.0).unwrap();
    param.cancel_scheduled_values(1.0).unwrap();

    assert_close(param.value_at(0.5), 0.5);
    assert_close(param.value_at(1.0), 1.0);
    assert_close(param.value_at(1.25), 1.0);
}

#[test]
fn audio_param_handle_rejects_negative_automation_times() {
    let mut context = AudioContext::new();
    let gain = context.create_gain();
    let param = gain.gain();

    assert_eq!(
        param.set_value_at_time(1.0, -0.1),
        Err(GraphError::NegativeTime)
    );
    assert_eq!(
        param.set_value_at_time(1.0, f64::INFINITY),
        Err(GraphError::InvalidAutomationValue)
    );
    assert_eq!(
        param.set_value(f32::NAN),
        Err(GraphError::InvalidAutomationValue)
    );
    assert_eq!(
        param.set_value_at_time(f32::INFINITY, 0.0),
        Err(GraphError::InvalidAutomationValue)
    );
    assert_eq!(
        param.linear_ramp_to_value_at_time(1.0, -0.1),
        Err(GraphError::NegativeTime)
    );
    assert_eq!(
        param.linear_ramp_to_value_at_time(f32::NAN, 1.0),
        Err(GraphError::InvalidAutomationValue)
    );
    assert_eq!(
        param.exponential_ramp_to_value_at_time(1.0, -0.1),
        Err(GraphError::NegativeTime)
    );
    assert_eq!(
        param.exponential_ramp_to_value_at_time(f32::INFINITY, 1.0),
        Err(GraphError::InvalidAutomationValue)
    );
    assert_eq!(
        param.set_target_at_time(1.0, -0.1, 0.1),
        Err(GraphError::NegativeTime)
    );
    assert_eq!(
        param.set_target_at_time(f32::NAN, 0.0, 0.1),
        Err(GraphError::InvalidAutomationValue)
    );
    assert_eq!(
        param.set_target_at_time(1.0, 0.0, -0.1),
        Err(GraphError::InvalidAutomationValue)
    );
    assert_eq!(
        param.set_value_curve_at_time([0.0, 1.0], -0.1, 0.1),
        Err(GraphError::NegativeTime)
    );
    assert_eq!(
        param.set_value_curve_at_time([1.0], 0.0, 0.1),
        Err(GraphError::InvalidAutomationValue)
    );
    assert_eq!(
        param.set_value_curve_at_time([0.0, 1.0], 0.0, 0.0),
        Err(GraphError::InvalidAutomationValue)
    );
    assert_eq!(
        param.set_value_curve_at_time([0.0, 1.0], 0.0, f64::INFINITY),
        Err(GraphError::InvalidAutomationValue)
    );
    assert_eq!(
        param.set_value_curve_at_time([0.0, f32::NAN], 0.0, 0.1),
        Err(GraphError::InvalidAutomationValue)
    );
    assert_eq!(
        param.cancel_scheduled_values(-0.1),
        Err(GraphError::NegativeTime)
    );
    assert_eq!(
        param.cancel_and_hold_at_time(-0.1),
        Err(GraphError::NegativeTime)
    );
}

#[test]
fn audio_param_handle_set_target_with_zero_time_constant_jumps_to_target() {
    let mut context = AudioContext::new();
    let gain = context.create_gain();
    let param = gain.gain();

    param.set_value(1.0).unwrap();
    param.set_target_at_time(0.25, 0.5, 0.0).unwrap();

    assert_close(param.value_at(0.25), 1.0);
    assert_close(param.value_at(0.5), 0.25);
    assert_close(param.value_at(0.75), 0.25);
}

#[test]
fn audio_param_ramp_after_set_target_replaces_the_target_curve() {
    let mut context = AudioContext::new();
    let gain = context.create_gain();
    let param = gain.gain();

    param.set_value(1.0).unwrap();
    param.set_target_at_time(0.0, 0.0, 1.0).unwrap();
    param.linear_ramp_to_value_at_time(0.5, 2.0).unwrap();

    assert_close(param.value_at(1.0), 0.75);
    assert_close(param.value_at(2.0), 0.5);
    assert_close(param.value_at(3.0), 0.5);
}

#[test]
fn audio_param_handle_rejects_invalid_exponential_ramps() {
    let mut context = AudioContext::new();
    let gain = context.create_gain();

    assert_eq!(
        gain.gain().try_exponential_ramp_to_value_at_time(0.0, 1.0),
        Err(GraphError::InvalidAutomationValue)
    );
    assert_eq!(
        gain.gain().exponential_ramp_to_value_at_time(0.0, 1.0),
        Err(GraphError::InvalidAutomationValue)
    );

    let delay = context.create_delay();
    delay.delay_time().set_value_at_time(0.25, 0.5).unwrap();
    assert_eq!(
        delay
            .delay_time()
            .try_exponential_ramp_to_value_at_time(0.5, 1.0),
        Ok(())
    );
    assert!(delay.delay_time().value_at(0.75) > 0.25);
}

#[test]
fn audio_param_exponential_ramp_from_zero_holds_until_end_time() {
    let mut context = AudioContext::new();
    let gain = context.create_gain();

    gain.gain().set_value(0.0).unwrap();
    assert_eq!(
        gain.gain().try_exponential_ramp_to_value_at_time(1.0, 1.0),
        Ok(())
    );
    assert_close(gain.gain().value_at(0.5), 0.0);
    assert_close(gain.gain().value_at(1.0), 1.0);
}

#[test]
fn audio_param_exponential_ramp_accepts_negative_nonzero_values() {
    let mut context = AudioContext::new();
    let gain = context.create_gain();

    gain.gain().set_value(-1.0).unwrap();
    assert_eq!(
        gain.gain().try_exponential_ramp_to_value_at_time(-4.0, 1.0),
        Ok(())
    );
    assert_close(gain.gain().value_at(0.5), -2.0);
    assert_close(gain.gain().value_at(1.0), -4.0);

    let sign_change = context.create_gain();
    sign_change.gain().set_value(-1.0).unwrap();
    assert_eq!(
        sign_change
            .gain()
            .try_exponential_ramp_to_value_at_time(1.0, 1.0),
        Ok(())
    );
    assert_close(sign_change.gain().value_at(0.5), -1.0);
    assert_close(sign_change.gain().value_at(1.0), 1.0);
}

#[test]
fn oscillator_exposes_live_webaudio_audio_param_handles() {
    let mut context = AudioContext::new();
    let oscillator = context.create_oscillator();
    oscillator.try_start(0.0).unwrap();

    assert_eq!(oscillator.frequency().default_value(), 440.0);
    assert_eq!(oscillator.detune().default_value(), 0.0);
    assert_eq!(oscillator.detune().min_value(), -153_600.0);
    assert_eq!(oscillator.detune().max_value(), 153_600.0);
    oscillator.set_type(Waveform::Square);
    assert_eq!(oscillator.type_value(), Waveform::Square);
    oscillator.set_type(Waveform::Sine);
    oscillator.set_periodic_wave(
        PeriodicWave::try_new_with_options(
            [0.0, 0.0],
            [0.0, 0.5],
            melody_bay::PeriodicWaveOptions {
                disable_normalization: true,
            },
        )
        .unwrap(),
    );
    oscillator.frequency().set_value(1.0).unwrap();
    oscillator.detune().set_value(1200.0).unwrap();
    context.connect(&oscillator, context.destination()).unwrap();

    let rendered = render_context_offline(&context, 4, 2).unwrap();

    assert_eq!(oscillator.frequency().value(), 1.0);
    assert_eq!(oscillator.detune().value(), 1200.0);
    assert_close(left_samples(&rendered)[1], 0.0);
    oscillator.detune().set_value(200_000.0).unwrap();
    assert_eq!(oscillator.detune().value(), 153_600.0);
}

#[test]
fn webaudio_node_options_configure_one_shot_node_state() {
    let mut context = AudioContext::try_new_with_sample_rate(48_000).unwrap();
    let buffer = AudioBuffer::try_from_channels(48_000, 4, [vec![0.0, 0.5, 1.0, 0.0]]).unwrap();

    let created_buffer = context
        .try_create_buffer_with_options(melody_bay::AudioBufferOptions {
            number_of_channels: 3,
            length: 8,
            sample_rate: 24_000,
        })
        .unwrap();
    assert_eq!(created_buffer.number_of_channels(), 3);
    assert_eq!(created_buffer.length(), 8);
    assert_eq!(created_buffer.sample_rate(), 24_000);

    let source = context
        .try_create_buffer_source_with_options(melody_bay::AudioBufferSourceOptions {
            buffer: Some(buffer.clone()),
            playback_rate: 2.0,
            detune: 1200.0,
            looping: true,
            loop_start: 0.25,
            loop_end: 0.75,
        })
        .unwrap();
    assert_eq!(source.buffer_value(), Some(buffer));
    assert_eq!(source.playback_rate().value(), 2.0);
    assert_eq!(source.detune().value(), 1200.0);
    assert!(source.looping_value());
    assert_eq!(source.loop_start_value(), 0.25);
    assert_eq!(source.loop_end_value(), 0.75);

    let constant = context
        .try_create_constant_source_with_options(melody_bay::ConstantSourceOptions { offset: 0.25 })
        .unwrap();
    assert_eq!(constant.offset().value(), 0.25);

    let gain = context
        .try_create_gain_with_options(melody_bay::GainOptions { gain: 0.75 })
        .unwrap();
    assert_eq!(gain.gain().value(), 0.75);

    let delay = context
        .try_create_delay_with_options(melody_bay::DelayOptions {
            max_delay_time: 2.5,
            delay_time: 0.5,
        })
        .unwrap();
    assert_eq!(delay.max_delay_time_value(), 2.5);
    assert_eq!(delay.delay_time().value(), 0.5);

    let filter = context
        .try_create_biquad_filter_with_options(melody_bay::BiquadFilterOptions {
            filter_type: BiquadFilterType::Highpass,
            frequency: 1_000.0,
            detune: 12.0,
            q: 0.5,
            gain: 3.0,
        })
        .unwrap();
    assert_eq!(filter.type_value(), BiquadFilterType::Highpass);
    assert_eq!(filter.frequency().value(), 1_000.0);
    assert_eq!(filter.detune().value(), 12.0);
    assert_eq!(filter.q().value(), 0.5);
    assert_eq!(filter.gain().value(), 3.0);

    let iir = context
        .try_create_iir_filter_with_options(melody_bay::IirFilterOptions {
            feedforward: vec![0.5, 0.25],
            feedback: vec![1.0, -0.25],
        })
        .unwrap();
    let mut mag = [0.0];
    let mut phase = [0.0];
    iir.get_frequency_response(&[1_000.0], &mut mag, &mut phase)
        .unwrap();
    assert!(mag[0].is_finite());

    let shaper = context
        .try_create_wave_shaper_with_options(melody_bay::WaveShaperOptions {
            curve: Some(vec![-1.0, 0.0, 1.0]),
            oversample: Oversample::TwoX,
        })
        .unwrap();
    assert_eq!(shaper.curve_value(), Some(vec![-1.0, 0.0, 1.0]));
    assert_eq!(shaper.oversample_value(), Oversample::TwoX);

    let stereo = context
        .try_create_stereo_panner_with_options(melody_bay::StereoPannerOptions { pan: -0.25 })
        .unwrap();
    assert_eq!(stereo.pan().value(), -0.25);

    let panner = context
        .try_create_panner_with_options(melody_bay::PannerOptions {
            panning_model: PanningModel::EqualPower,
            distance_model: DistanceModel::Linear,
            position_x: 1.0,
            position_y: 2.0,
            position_z: 3.0,
            orientation_x: 0.0,
            orientation_y: 1.0,
            orientation_z: 0.0,
            ref_distance: 0.5,
            max_distance: 50.0,
            rolloff_factor: 0.75,
            cone_inner_angle: 90.0,
            cone_outer_angle: 180.0,
            cone_outer_gain: 0.25,
        })
        .unwrap();
    assert_eq!(panner.panning_model_value(), PanningModel::EqualPower);
    assert_eq!(panner.distance_model_value(), DistanceModel::Linear);
    assert_eq!(panner.position_x().value(), 1.0);
    assert_eq!(panner.position_y().value(), 2.0);
    assert_eq!(panner.position_z().value(), 3.0);
    assert_eq!(panner.orientation_x().value(), 0.0);
    assert_eq!(panner.orientation_y().value(), 1.0);
    assert_eq!(panner.orientation_z().value(), 0.0);
    assert_eq!(panner.ref_distance_value(), 0.5);
    assert_eq!(panner.max_distance_value(), 50.0);
    assert_eq!(panner.rolloff_factor_value(), 0.75);
    assert_eq!(panner.cone_inner_angle_value(), 90.0);
    assert_eq!(panner.cone_outer_angle_value(), 180.0);
    assert_eq!(panner.cone_outer_gain_value(), 0.25);

    let compressor = context
        .try_create_dynamics_compressor_with_options(melody_bay::DynamicsCompressorOptions {
            threshold: -12.0,
            knee: 20.0,
            ratio: 4.0,
            attack: 0.01,
            release: 0.5,
        })
        .unwrap();
    assert_eq!(compressor.threshold().value(), -12.0);
    assert_eq!(compressor.knee().value(), 20.0);
    assert_eq!(compressor.ratio().value(), 4.0);
    assert_eq!(compressor.attack().value(), 0.01);
    assert_eq!(compressor.release().value(), 0.5);

    let analyser = context
        .try_create_analyser_with_options(melody_bay::AnalyserOptions {
            fft_size: 1024,
            min_decibels: -90.0,
            max_decibels: -10.0,
            smoothing_time_constant: 0.25,
        })
        .unwrap();
    assert_eq!(analyser.fft_size_value(), 1024);
    assert_eq!(analyser.min_decibels_value(), -90.0);
    assert_eq!(analyser.max_decibels_value(), -10.0);
    assert_eq!(analyser.smoothing_time_constant_value(), 0.25);

    let splitter = context
        .try_create_channel_splitter_with_options(melody_bay::ChannelSplitterOptions {
            number_of_outputs: 4,
        })
        .unwrap();
    assert_eq!(context.node_info(&splitter).unwrap().number_of_outputs, 4);

    let merger = context
        .try_create_channel_merger_with_options(melody_bay::ChannelMergerOptions {
            number_of_inputs: 3,
        })
        .unwrap();
    assert_eq!(context.node_info(&merger).unwrap().number_of_inputs, 3);

    let impulse = AudioBuffer::try_from_channels(48_000, 2, [vec![1.0], vec![0.0]]).unwrap();
    let convolver = context
        .try_create_convolver_with_options(melody_bay::ConvolverOptions {
            buffer: Some(impulse.clone()),
            disable_normalization: true,
        })
        .unwrap();
    assert_eq!(convolver.buffer_value(), Some(impulse));
    assert!(!convolver.normalize_value());

    let wave = PeriodicWave::try_new_with_options(
        [0.0, 0.0],
        [0.0, 1.0],
        melody_bay::PeriodicWaveOptions {
            disable_normalization: true,
        },
    )
    .unwrap();
    let oscillator = context
        .try_create_oscillator_with_options(melody_bay::OscillatorOptions {
            oscillator_type: melody_bay::OscillatorType::Custom(wave),
            frequency: 220.0,
            detune: 100.0,
        })
        .unwrap();
    assert_eq!(
        oscillator.oscillator_type(),
        melody_bay::OscillatorTypeValue::Custom
    );
    assert_eq!(oscillator.frequency().value(), 220.0);
    assert_eq!(oscillator.detune().value(), 100.0);
    oscillator.set_type(Waveform::Sawtooth);
    assert_eq!(
        oscillator.oscillator_type(),
        melody_bay::OscillatorTypeValue::Sawtooth
    );

    let mut offline = OfflineAudioContext::try_new(1, 128, 48_000).unwrap();
    let offline_gain = offline
        .try_create_gain_with_options(melody_bay::GainOptions { gain: 0.5 })
        .unwrap();
    assert_eq!(offline_gain.gain().value(), 0.5);
    let offline_source = offline
        .try_create_buffer_source_with_options(melody_bay::AudioBufferSourceOptions {
            playback_rate: 0.5,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(offline_source.playback_rate().value(), 0.5);
    let offline_convolver = offline
        .try_create_convolver_with_options(melody_bay::ConvolverOptions {
            disable_normalization: true,
            ..Default::default()
        })
        .unwrap();
    assert!(!offline_convolver.normalize_value());
    let offline_oscillator = offline
        .try_create_oscillator_with_options(melody_bay::OscillatorOptions {
            oscillator_type: melody_bay::OscillatorType::Basic(Waveform::Triangle),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        offline_oscillator.oscillator_type(),
        melody_bay::OscillatorTypeValue::Triangle
    );
}

#[test]
fn webaudio_node_options_validate_constructor_values() {
    let mut context = AudioContext::try_new_with_sample_rate(48_000).unwrap();

    assert_eq!(
        context
            .try_create_buffer_with_options(melody_bay::AudioBufferOptions {
                number_of_channels: 0,
                length: 8,
                sample_rate: 48_000,
            })
            .err(),
        Some(GraphError::InvalidAudioBuffer)
    );
    assert_eq!(
        context
            .try_create_delay_with_options(melody_bay::DelayOptions {
                max_delay_time: 0.0,
                ..Default::default()
            })
            .err(),
        Some(GraphError::InvalidDelayTime)
    );
    assert_eq!(
        context
            .try_create_iir_filter_with_options(melody_bay::IirFilterOptions {
                feedforward: Vec::new(),
                feedback: vec![1.0],
            })
            .err(),
        Some(GraphError::InvalidIirFilter)
    );
    assert_eq!(
        context
            .try_create_wave_shaper_with_options(melody_bay::WaveShaperOptions {
                curve: Some(vec![f32::NAN]),
                ..Default::default()
            })
            .err(),
        Some(GraphError::InvalidWaveShaperCurve)
    );
    assert_eq!(
        context
            .try_create_panner_with_options(melody_bay::PannerOptions {
                cone_outer_gain: 1.25,
                ..Default::default()
            })
            .err(),
        Some(GraphError::InvalidPannerConfig)
    );
    assert_eq!(
        context
            .try_create_analyser_with_options(melody_bay::AnalyserOptions {
                fft_size: 3,
                ..Default::default()
            })
            .err(),
        Some(GraphError::InvalidAnalyserConfig)
    );
    assert_eq!(
        context
            .try_create_channel_splitter_with_options(melody_bay::ChannelSplitterOptions {
                number_of_outputs: 0,
            })
            .err(),
        Some(GraphError::InvalidChannelCount)
    );
    assert_eq!(
        context
            .try_create_channel_merger_with_options(melody_bay::ChannelMergerOptions {
                number_of_inputs: 33,
            })
            .err(),
        Some(GraphError::InvalidChannelCount)
    );
}

#[test]
fn biquad_filter_exposes_node_owned_type_setter() {
    let mut context = AudioContext::new();
    let filter = context.create_biquad_filter();

    assert_eq!(filter.type_value(), BiquadFilterType::Lowpass);
    filter.set_type(BiquadFilterType::Highpass);
    assert_eq!(filter.type_value(), BiquadFilterType::Highpass);
}

#[test]
fn constant_source_exposes_live_webaudio_audio_param_handle() {
    let mut context = AudioContext::new();
    let source = started_constant_source(&mut context);

    assert_eq!(source.offset().default_value(), 1.0);
    assert_eq!(source.offset().value(), 1.0);

    source.offset().set_value(0.375).unwrap();
    context.connect(&source, context.destination()).unwrap();

    let rendered = render_context_offline(&context, 4, 1).unwrap();

    assert_eq!(left_samples(&rendered), &[0.375]);
    assert_eq!(source.offset().value(), 0.375);
}

#[test]
fn buffer_source_exposes_live_webaudio_audio_param_handles() {
    let mut context = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut context,
        audio_buffer_from_mono(4, 4, [0.0, 0.25, 0.5, 0.75]),
    );

    assert_eq!(source.playback_rate().default_value(), 1.0);
    assert_eq!(source.playback_rate().min_value(), f32::MIN);
    assert_eq!(source.playback_rate().max_value(), f32::MAX);
    assert_eq!(
        source.playback_rate().automation_rate(),
        melody_bay::AutomationRate::KRate
    );
    assert_eq!(source.detune().default_value(), 0.0);
    assert_eq!(source.detune().min_value(), f32::MIN);
    assert_eq!(source.detune().max_value(), f32::MAX);
    assert_eq!(
        source.detune().automation_rate(),
        melody_bay::AutomationRate::KRate
    );
    assert_eq!(source.loop_start_value(), 0.0);
    assert_eq!(source.loop_end_value(), 0.0);
    source.try_loop_start(-0.1).unwrap();
    source.try_loop_end(-0.1).unwrap();
    assert_eq!(source.loop_start_value(), -0.1);
    assert_eq!(source.loop_end_value(), -0.1);
    source.try_loop_start(0.25).unwrap();
    source.try_loop_end(0.75).unwrap();
    assert_eq!(source.loop_start_value(), 0.25);
    assert_eq!(source.loop_end_value(), 0.75);
    assert!(!source.looping_value());

    source.playback_rate().set_value(1.0).unwrap();
    source.detune().set_value(1200.0).unwrap();
    context.connect(&source, context.destination()).unwrap();

    let rendered = render_context_offline(&context, 4, 2).unwrap();

    assert_eq!(source.playback_rate().value(), 1.0);
    assert_eq!(source.detune().value(), 1200.0);
    assert_eq!(left_samples(&rendered), &[0.0, 0.5]);
}

#[test]
fn buffer_source_exposes_node_owned_buffer_setters() {
    let mut context = AudioContext::new();
    let source = context.create_buffer_source();

    assert_eq!(source.buffer_value(), None);
    source
        .try_set_buffer(audio_buffer_from_mono(3_000, 1, [1.0]))
        .unwrap();
    assert_eq!(
        source.buffer_value().unwrap().channel_data(0),
        Some(&[1.0][..])
    );
    source.clear_buffer();
    assert_eq!(source.buffer_value(), None);
}

#[test]
fn stereo_panner_exposes_live_webaudio_audio_param_handle() {
    let mut context = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut context);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let panner = context.create_stereo_panner();

    assert_eq!(panner.pan().default_value(), 0.0);
    assert_eq!(panner.pan().min_value(), -1.0);
    assert_eq!(panner.pan().max_value(), 1.0);
    assert_eq!(panner.pan().value(), 0.0);

    panner.pan().set_value(2.0).unwrap();
    context.connect(&source, &panner).unwrap();
    context.connect(&panner, context.destination()).unwrap();

    let rendered = render_context_offline(&context, 4, 1).unwrap();

    assert_eq!(rendered.channel_data(0), Some(&[0.0][..]));
    assert_eq!(rendered.channel_data(1), Some(&[1.0][..]));
    assert_eq!(panner.pan().value(), 1.0);
}

#[test]
fn biquad_filter_exposes_live_webaudio_audio_param_handles() {
    let mut context = AudioContext::new();
    let filter = context.create_biquad_filter();

    assert_eq!(filter.frequency().default_value(), 350.0);
    assert_eq!(filter.detune().default_value(), 0.0);
    assert_eq!(filter.detune().min_value(), -153_600.0);
    assert_eq!(filter.detune().max_value(), 153_600.0);
    assert_eq!(filter.q().default_value(), 1.0);
    assert_eq!(filter.gain().default_value(), 0.0);
    assert_eq!(filter.gain().max_value(), 1541.0);

    filter.frequency().set_value(1_000.0).unwrap();
    filter.detune().set_value(-200_000.0).unwrap();
    filter.q().set_value(0.5).unwrap();
    filter.gain().set_value(2_000.0).unwrap();

    assert_eq!(filter.frequency().value(), 1_000.0);
    assert_eq!(filter.detune().value(), -153_600.0);
    assert_eq!(filter.q().value(), 0.5);
    assert_eq!(filter.gain().value(), 1541.0);
}

#[test]
fn delay_node_exposes_live_webaudio_audio_param_handle() {
    let mut context = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut context,
        audio_buffer_from_mono(4, 4, [1.0, 0.0, 0.0, 0.0]),
    );
    let delay = context.create_delay();

    assert_eq!(delay.delay_time().default_value(), 0.0);
    assert_eq!(delay.delay_time().min_value(), 0.0);
    assert_eq!(delay.delay_time().max_value(), 1.0);
    assert_eq!(delay.delay_time().value(), 0.0);

    delay.delay_time().set_value(0.5).unwrap();
    context.connect(&source, &delay).unwrap();
    context.connect(&delay, context.destination()).unwrap();

    let rendered = render_context_offline(&context, 4, 3).unwrap();

    assert_eq!(left_samples(&rendered), &[0.0, 0.0, 1.0]);
    assert_eq!(delay.delay_time().value(), 0.5);
}

#[test]
fn dynamics_compressor_exposes_live_webaudio_audio_param_handles() {
    let mut context = AudioContext::new();
    let compressor = context.create_dynamics_compressor();

    assert_eq!(compressor.threshold().default_value(), -24.0);
    assert_eq!(compressor.threshold().min_value(), -100.0);
    assert_eq!(compressor.threshold().max_value(), 0.0);
    assert_eq!(compressor.knee().default_value(), 30.0);
    assert_eq!(compressor.knee().min_value(), 0.0);
    assert_eq!(compressor.knee().max_value(), 40.0);
    assert_eq!(compressor.ratio().default_value(), 12.0);
    assert_eq!(compressor.ratio().min_value(), 1.0);
    assert_eq!(compressor.ratio().max_value(), 20.0);
    assert_eq!(compressor.attack().default_value(), 0.003);
    assert_eq!(compressor.attack().min_value(), 0.0);
    assert_eq!(compressor.attack().max_value(), 1.0);
    assert_eq!(compressor.release().default_value(), 0.25);
    assert_eq!(compressor.release().min_value(), 0.0);
    assert_eq!(compressor.release().max_value(), 1.0);

    compressor.threshold().set_value(-12.0).unwrap();
    compressor.knee().set_value(0.0).unwrap();
    compressor.ratio().set_value(4.0).unwrap();
    compressor.attack().set_value(0.01).unwrap();
    compressor.release().set_value(0.5).unwrap();

    assert_eq!(compressor.threshold().value(), -12.0);
    assert_eq!(compressor.knee().value(), 0.0);
    assert_eq!(compressor.ratio().value(), 4.0);
    assert_eq!(compressor.attack().value(), 0.01);
    assert_eq!(compressor.release().value(), 0.5);
    compressor.threshold().set_value(24.0).unwrap();
    compressor.knee().set_value(100.0).unwrap();
    compressor.ratio().set_value(0.0).unwrap();
    compressor.attack().set_value(2.0).unwrap();
    compressor.release().set_value(2.0).unwrap();
    assert_eq!(compressor.threshold().value(), 0.0);
    assert_eq!(compressor.knee().value(), 40.0);
    assert_eq!(compressor.ratio().value(), 1.0);
    assert_eq!(compressor.attack().value(), 1.0);
    assert_eq!(compressor.release().value(), 1.0);
    assert_eq!(
        compressor.threshold().automation_rate(),
        melody_bay::AutomationRate::KRate
    );
    assert_eq!(
        compressor.knee().automation_rate(),
        melody_bay::AutomationRate::KRate
    );
    assert_eq!(
        compressor.ratio().automation_rate(),
        melody_bay::AutomationRate::KRate
    );
    assert_eq!(
        compressor.attack().automation_rate(),
        melody_bay::AutomationRate::KRate
    );
    assert_eq!(
        compressor.release().automation_rate(),
        melody_bay::AutomationRate::KRate
    );
}

#[test]
fn panner_node_exposes_live_webaudio_audio_param_handles() {
    let mut context = AudioContext::new();
    let panner = context.create_panner();

    assert_eq!(panner.position_x().default_value(), 0.0);
    assert_eq!(panner.position_y().default_value(), 0.0);
    assert_eq!(panner.position_z().default_value(), 0.0);
    assert_eq!(panner.orientation_x().default_value(), 1.0);
    assert_eq!(panner.orientation_y().default_value(), 0.0);
    assert_eq!(panner.orientation_z().default_value(), 0.0);
    assert_eq!(panner.panning_model_value(), PanningModel::EqualPower);

    panner.position_x().set_value(1.0).unwrap();
    panner.position_y().set_value(2.0).unwrap();
    panner.position_z().set_value(3.0).unwrap();
    panner.orientation_x().set_value(0.0).unwrap();
    panner.orientation_y().set_value(1.0).unwrap();
    panner.orientation_z().set_value(0.5).unwrap();
    panner.set_panning_model(PanningModel::EqualPower).unwrap();
    panner.set_distance_model(DistanceModel::Linear);

    assert_eq!(panner.position_x().value(), 1.0);
    assert_eq!(panner.position_y().value(), 2.0);
    assert_eq!(panner.position_z().value(), 3.0);
    assert_eq!(panner.orientation_x().value(), 0.0);
    assert_eq!(panner.orientation_y().value(), 1.0);
    assert_eq!(panner.orientation_z().value(), 0.5);
    assert_eq!(panner.panning_model_value(), PanningModel::EqualPower);
    assert_eq!(
        panner.set_panning_model(PanningModel::Hrtf),
        Err(GraphError::UnsupportedPanningModel)
    );
    assert_eq!(panner.distance_model_value(), DistanceModel::Linear);
}

#[test]
fn panner_try_setters_validate_webaudio_numeric_attributes() {
    let mut context = AudioContext::new();
    let panner = context.create_panner();

    assert_eq!(
        panner.try_cone_inner_angle(-0.001).err(),
        Some(GraphError::InvalidPannerConfig)
    );
    assert_eq!(
        panner.try_cone_outer_angle(360.001).err(),
        Some(GraphError::InvalidPannerConfig)
    );
    assert_eq!(
        panner.try_cone_outer_gain(1.001).err(),
        Some(GraphError::InvalidPannerConfig)
    );
    assert_eq!(
        panner.try_ref_distance(-0.001).err(),
        Some(GraphError::InvalidPannerConfig)
    );
    assert_eq!(
        panner.try_max_distance(-1.0).err(),
        Some(GraphError::InvalidPannerConfig)
    );
    assert_eq!(
        panner.try_rolloff_factor(-0.001).err(),
        Some(GraphError::InvalidPannerConfig)
    );

    panner.try_cone_inner_angle(30.0).unwrap();
    panner.try_cone_outer_angle(60.0).unwrap();
    panner.try_cone_outer_gain(0.25).unwrap();
    panner.try_ref_distance(0.0).unwrap();
    panner.try_max_distance(10.0).unwrap();
    panner.try_rolloff_factor(0.5).unwrap();

    assert_eq!(panner.cone_inner_angle_value(), 30.0);
    assert_eq!(panner.cone_outer_angle_value(), 60.0);
    assert_eq!(panner.cone_outer_gain_value(), 0.25);
    assert_eq!(panner.ref_distance_value(), 0.0);
    assert_eq!(panner.max_distance_value(), 10.0);
    assert_eq!(panner.rolloff_factor_value(), 0.5);
}

#[test]
fn audio_listener_exposes_live_webaudio_audio_param_handles() {
    let context = AudioContext::new();
    let listener = context.listener();

    assert_eq!(listener.position_x().default_value(), 0.0);
    assert_eq!(listener.position_y().default_value(), 0.0);
    assert_eq!(listener.position_z().default_value(), 0.0);
    assert_eq!(listener.forward_x().default_value(), 0.0);
    assert_eq!(listener.forward_y().default_value(), 0.0);
    assert_eq!(listener.forward_z().default_value(), -1.0);
    assert_eq!(listener.up_x().default_value(), 0.0);
    assert_eq!(listener.up_y().default_value(), 1.0);
    assert_eq!(listener.up_z().default_value(), 0.0);

    listener.position_x().set_value(1.0).unwrap();
    listener.position_y().set_value(2.0).unwrap();
    listener.position_z().set_value(3.0).unwrap();
    listener.forward_x().set_value(0.0).unwrap();
    listener.forward_y().set_value(0.0).unwrap();
    listener.forward_z().set_value(1.0).unwrap();
    listener.up_x().set_value(0.0).unwrap();
    listener.up_y().set_value(1.0).unwrap();
    listener.up_z().set_value(0.5).unwrap();

    assert_eq!(listener.position_value(), [1.0, 2.0, 3.0]);
    assert_eq!(listener.forward_value(), [0.0, 0.0, 1.0]);
    assert_eq!(listener.up_value(), [0.0, 1.0, 0.5]);
}

#[test]
fn analyser_writes_time_and_frequency_data_into_provided_buffers() {
    let mut context = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut context,
        audio_buffer_from_mono(4, 4, [-1.0, 0.0, 1.0, 0.5]),
    );
    let analyser = context.create_analyser();
    analyser.try_fft_size(32).unwrap();
    analyser.try_smoothing_time_constant(0.0).unwrap();
    context.connect(&source, &analyser).unwrap();
    context.connect(&analyser, context.destination()).unwrap();

    render_context_offline(&context, 4, 4).unwrap();

    let mut float_time = [9.0; 4];
    analyser.get_float_time_domain_data(&mut float_time);
    assert_eq!(float_time, [-1.0, 0.0, 1.0, 0.5]);

    let mut byte_time = [0; 4];
    analyser.get_byte_time_domain_data(&mut byte_time);
    assert_eq!(byte_time, [0, 128, 255, 192]);

    let mut float_frequency = [9.0; 2];
    analyser.get_float_frequency_data(&mut float_frequency);
    assert!(float_frequency.iter().all(|value| value.is_finite()));

    let mut byte_frequency = [0; 2];
    analyser.get_byte_frequency_data(&mut byte_frequency);
    assert!(byte_frequency.iter().any(|value| *value > 0));
}

#[test]
fn analyser_ignores_excess_destination_elements() {
    let mut context = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut context,
        audio_buffer_from_mono(4, 4, [-1.0, 0.0, 1.0, 0.5]),
    );
    let analyser = context.create_analyser();
    analyser.try_fft_size(32).unwrap();
    analyser.try_smoothing_time_constant(0.0).unwrap();
    context.connect(&source, &analyser).unwrap();
    context.connect(&analyser, context.destination()).unwrap();

    render_context_offline(&context, 4, 4).unwrap();

    let mut float_time = [9.0; 34];
    analyser.get_float_time_domain_data(&mut float_time);
    assert_eq!(&float_time[..4], &[-1.0, 0.0, 1.0, 0.5]);
    assert_eq!(&float_time[4..32], &[0.0; 28]);
    assert_eq!(&float_time[32..], &[9.0, 9.0]);

    let mut byte_time = [9; 34];
    analyser.get_byte_time_domain_data(&mut byte_time);
    assert_eq!(&byte_time[..4], &[0, 128, 255, 192]);
    assert_eq!(&byte_time[4..32], &[128; 28]);
    assert_eq!(&byte_time[32..], &[9, 9]);

    let mut float_frequency = [9.0; 18];
    analyser.get_float_frequency_data(&mut float_frequency);
    assert_eq!(&float_frequency[16..], &[9.0, 9.0]);

    let mut byte_frequency = [9; 18];
    analyser.get_byte_frequency_data(&mut byte_frequency);
    assert_eq!(&byte_frequency[16..], &[9, 9]);
}

#[test]
fn analyser_try_setters_reject_invalid_webaudio_values() {
    let mut context = AudioContext::new();
    let analyser = context.create_analyser();

    assert_eq!(analyser.fft_size_value(), 2048);
    assert_eq!(
        analyser.try_fft_size(3),
        Err(GraphError::InvalidAnalyserConfig)
    );
    assert_eq!(analyser.fft_size_value(), 2048);
    analyser.try_fft_size(32).unwrap();
    assert_eq!(analyser.fft_size_value(), 32);

    assert_eq!(
        analyser.try_min_decibels(-20.0),
        Err(GraphError::InvalidAnalyserConfig)
    );
    assert_eq!(
        analyser.try_min_decibels(f32::NAN),
        Err(GraphError::InvalidAnalyserConfig)
    );
    assert_eq!(
        analyser.try_min_decibels(f32::NEG_INFINITY),
        Err(GraphError::InvalidAnalyserConfig)
    );
    assert_eq!(
        analyser.try_max_decibels(-120.0),
        Err(GraphError::InvalidAnalyserConfig)
    );
    assert_eq!(
        analyser.try_max_decibels(f32::NAN),
        Err(GraphError::InvalidAnalyserConfig)
    );
    assert_eq!(
        analyser.try_max_decibels(f32::INFINITY),
        Err(GraphError::InvalidAnalyserConfig)
    );
    assert_eq!(
        analyser.try_smoothing_time_constant(1.5),
        Err(GraphError::InvalidAnalyserConfig)
    );
    analyser.try_smoothing_time_constant(0.25).unwrap();
    assert_eq!(analyser.smoothing_time_constant_value(), 0.25);
}

#[test]
fn convolver_try_buffer_validates_webaudio_channel_counts() {
    let mut context = AudioContext::try_new_with_sample_rate(48_000).unwrap();
    let convolver = context.create_convolver();
    let invalid = audio_buffer_from_channels(4, 1, [[1.0], [0.0], [0.0]]);

    assert_eq!(
        convolver.try_buffer(invalid).err(),
        Some(GraphError::InvalidConvolverBuffer)
    );
    assert_eq!(
        AudioBuffer::try_from_mono(48_000, 0, []).err(),
        Some(GraphError::InvalidAudioBuffer)
    );
    assert_eq!(
        convolver
            .try_buffer(audio_buffer_from_mono(44_100, 1, [1.0]))
            .err(),
        Some(GraphError::InvalidConvolverBuffer)
    );
    assert_eq!(convolver.buffer_value(), None);

    convolver
        .try_buffer(audio_buffer_from_channels(
            48_000,
            1,
            [[1.0], [0.0], [0.0], [0.0]],
        ))
        .unwrap();
    assert_eq!(
        convolver
            .buffer_value()
            .map(|buffer| buffer.number_of_channels()),
        Some(4)
    );
}

#[test]
fn biquad_filter_writes_frequency_response_into_provided_buffers() {
    let mut context = AudioContext::try_new_with_sample_rate(48_000).unwrap();
    let filter = context.create_biquad_filter();
    filter.set_type(BiquadFilterType::Allpass);
    filter.frequency().set_value(1_000.0).unwrap();
    filter.q().set_value(0.707).unwrap();
    let frequencies = [100.0, 1_000.0, 10_000.0];
    let mut magnitudes = [0.0; 3];
    let mut phases = [0.0; 3];

    filter
        .get_frequency_response(&frequencies, &mut magnitudes, &mut phases)
        .unwrap();

    for magnitude in magnitudes {
        assert_close(magnitude, 1.0);
    }
    assert!(phases.iter().all(|phase| phase.is_finite()));
}

#[test]
fn biquad_filter_frequency_response_writes_nan_for_invalid_frequencies() {
    let mut context = AudioContext::try_new_with_sample_rate(48_000).unwrap();
    let filter = context.create_biquad_filter();
    let frequencies = [-1.0, 24_001.0, f32::INFINITY, f32::NAN];
    let mut magnitudes = [0.0; 4];
    let mut phases = [0.0; 4];

    filter
        .get_frequency_response(&frequencies, &mut magnitudes, &mut phases)
        .unwrap();

    assert!(magnitudes.iter().all(|value| value.is_nan()));
    assert!(phases.iter().all(|value| value.is_nan()));
}

#[test]
fn iir_filter_writes_frequency_response_into_provided_buffers() {
    let mut context = AudioContext::try_new_with_sample_rate(48_000).unwrap();
    let filter = context.try_create_iir_filter([1.0], [1.0]).unwrap();
    let frequencies = [100.0, 1_000.0, 10_000.0];
    let mut magnitudes = [0.0; 3];
    let mut phases = [0.0; 3];

    filter
        .get_frequency_response(&frequencies, &mut magnitudes, &mut phases)
        .unwrap();

    for magnitude in magnitudes {
        assert_close(magnitude, 1.0);
    }
    for phase in phases {
        assert_close(phase, 0.0);
    }
}

#[test]
fn iir_filter_frequency_response_writes_nan_for_invalid_frequencies() {
    let mut context = AudioContext::try_new_with_sample_rate(48_000).unwrap();
    let filter = context.try_create_iir_filter([1.0], [1.0]).unwrap();
    let frequencies = [-1.0, 24_001.0, f32::INFINITY, f32::NAN];
    let mut magnitudes = [0.0; 4];
    let mut phases = [0.0; 4];

    filter
        .get_frequency_response(&frequencies, &mut magnitudes, &mut phases)
        .unwrap();

    assert!(magnitudes.iter().all(|value| value.is_nan()));
    assert!(phases.iter().all(|value| value.is_nan()));
}

#[test]
fn try_create_iir_filter_rejects_invalid_coefficients() {
    let mut context = AudioContext::new();

    assert_eq!(
        context.try_create_iir_filter([], [1.0]).err(),
        Some(GraphError::InvalidIirFilter)
    );
    assert_eq!(
        context.try_create_iir_filter([0.0], [1.0]).err(),
        Some(GraphError::InvalidIirFilter)
    );
    assert_eq!(
        context.try_create_iir_filter([1.0], []).err(),
        Some(GraphError::InvalidIirFilter)
    );
    assert_eq!(
        context.try_create_iir_filter([1.0], [0.0]).err(),
        Some(GraphError::InvalidIirFilter)
    );
    assert_eq!(
        context.try_create_iir_filter([1.0; 21], [1.0]).err(),
        Some(GraphError::InvalidIirFilter)
    );
    assert_eq!(
        context.try_create_iir_filter([1.0], [1.0; 21]).err(),
        Some(GraphError::InvalidIirFilter)
    );
    assert_eq!(
        context.try_create_iir_filter([f32::NAN], [1.0]).err(),
        Some(GraphError::InvalidIirFilter)
    );
    assert_eq!(
        context.try_create_iir_filter([1.0], [f32::INFINITY]).err(),
        Some(GraphError::InvalidIirFilter)
    );
    assert_eq!(
        context.try_create_iir_filter([1.0], [1.0, -1.1]).err(),
        Some(GraphError::InvalidIirFilter)
    );

    let filter = context.try_create_iir_filter([1.0], [1.0]).unwrap();
    let mut magnitudes = [0.0];
    let mut phases = [0.0];
    filter
        .get_frequency_response(&[1_000.0], &mut magnitudes, &mut phases)
        .unwrap();
    assert_close(magnitudes[0], 1.0);
    assert_close(phases[0], 0.0);

    let filter = context.create_iir_filter([1.0], [1.0]).unwrap();
    filter
        .get_frequency_response(&[1_000.0], &mut magnitudes, &mut phases)
        .unwrap();
    assert_close(magnitudes[0], 1.0);
    assert_close(phases[0], 0.0);

    let mut offline = OfflineAudioContext::try_new(1, 1, 44_100).unwrap();
    let filter = offline.create_iir_filter([1.0], [1.0]).unwrap();
    filter
        .get_frequency_response(&[1_000.0], &mut magnitudes, &mut phases)
        .unwrap();
    assert_close(magnitudes[0], 1.0);
    assert_close(phases[0], 0.0);
}

#[test]
fn iir_filter_coefficient_replacement_validates_like_factory_creation() {
    let mut context = AudioContext::new();
    let filter = context.try_create_iir_filter([1.0], [1.0]).unwrap();

    assert_eq!(
        filter.coefficients([1.0], [1.0, -1.1]).err(),
        Some(GraphError::InvalidIirFilter)
    );

    let mut magnitudes = [0.0];
    let mut phases = [0.0];
    filter
        .get_frequency_response(&[1_000.0], &mut magnitudes, &mut phases)
        .unwrap();
    assert_close(magnitudes[0], 1.0);
    assert_close(phases[0], 0.0);
}

#[test]
fn try_create_periodic_wave_rejects_invalid_coefficient_arrays() {
    let context = AudioContext::new();

    assert_eq!(
        context.try_create_periodic_wave([0.0], [0.0]).err(),
        Some(GraphError::InvalidPeriodicWave)
    );
    assert_eq!(
        context.try_create_periodic_wave([0.0, 1.0], [0.0]).err(),
        Some(GraphError::InvalidPeriodicWave)
    );
    assert_eq!(
        context
            .try_create_periodic_wave([0.0, f32::NAN], [0.0, 1.0])
            .err(),
        Some(GraphError::InvalidPeriodicWave)
    );
    assert_eq!(
        context
            .try_create_periodic_wave([0.0, 1.0], [0.0, f32::INFINITY])
            .err(),
        Some(GraphError::InvalidPeriodicWave)
    );

    let wave = context
        .try_create_periodic_wave_with_options(
            [1.0, 0.25],
            [1.0, 0.75],
            melody_bay::PeriodicWaveOptions {
                disable_normalization: true,
            },
        )
        .unwrap();
    assert_eq!(
        wave,
        melody_bay::PeriodicWave::try_new_with_options(
            [0.0, 0.25],
            [0.0, 0.75],
            melody_bay::PeriodicWaveOptions {
                disable_normalization: true,
            },
        )
        .unwrap()
    );

    let offline = OfflineAudioContext::try_new(2, 16, 44_100).unwrap();
    assert_eq!(
        offline.try_create_periodic_wave([], []).err(),
        Some(GraphError::InvalidPeriodicWave)
    );
}

#[test]
fn periodic_wave_try_constructors_validate_like_context_factories() {
    assert_eq!(
        melody_bay::PeriodicWave::try_new([0.0], [0.0]).err(),
        Some(GraphError::InvalidPeriodicWave)
    );
    assert_eq!(
        melody_bay::PeriodicWave::try_new([0.0, 1.0], [0.0]).err(),
        Some(GraphError::InvalidPeriodicWave)
    );
    assert_eq!(
        melody_bay::PeriodicWave::try_new([0.0, f32::NAN], [0.0, 1.0]).err(),
        Some(GraphError::InvalidPeriodicWave)
    );

    assert_eq!(
        melody_bay::PeriodicWave::try_new_with_options(
            [0.0, 0.25],
            [0.0, 0.75],
            melody_bay::PeriodicWaveOptions {
                disable_normalization: true,
            },
        )
        .unwrap(),
        melody_bay::PeriodicWave::try_new_with_options(
            [0.0, 0.25],
            [0.0, 0.75],
            melody_bay::PeriodicWaveOptions {
                disable_normalization: true,
            },
        )
        .unwrap()
    );

    let normalized = melody_bay::PeriodicWave::try_new([0.0, 0.0], [0.0, 2.0]).unwrap();
    assert_eq!(
        normalized,
        melody_bay::PeriodicWave::try_new_with_options(
            [0.0, 0.0],
            [0.0, 1.0],
            melody_bay::PeriodicWaveOptions {
                disable_normalization: true,
            },
        )
        .unwrap()
    );
}

#[test]
fn periodic_wave_factory_normalizes_unless_disabled() {
    let mut normalized = AudioContext::new();
    let normalized_wave = normalized
        .try_create_periodic_wave([0.0, 0.0], [0.0, 2.0])
        .unwrap();
    let normalized_osc = normalized.create_oscillator();
    normalized_osc.set_periodic_wave(normalized_wave);
    normalized_osc.try_start(0.0).unwrap();
    normalized_osc.frequency().set_value(1.0).unwrap();
    normalized
        .connect(&normalized_osc, normalized.destination())
        .unwrap();

    let mut raw = AudioContext::new();
    let raw_wave = raw
        .try_create_periodic_wave_with_options(
            [0.0, 0.0],
            [0.0, 2.0],
            melody_bay::PeriodicWaveOptions {
                disable_normalization: true,
            },
        )
        .unwrap();
    let raw_osc = raw.create_oscillator();
    raw_osc.set_periodic_wave(raw_wave);
    raw_osc.try_start(0.0).unwrap();
    raw_osc.frequency().set_value(1.0).unwrap();
    raw.connect(&raw_osc, raw.destination()).unwrap();

    let normalized_rendered = render_context_offline(&normalized, 4, 2).unwrap();
    let raw_rendered = render_context_offline(&raw, 4, 2).unwrap();

    assert_close(left_samples(&normalized_rendered)[1], 1.0);
    assert_close(left_samples(&raw_rendered)[1], 2.0);
}
