use kira::backend::mock::{MockBackend, MockBackendSettings};
use kira::backend::{Backend, Renderer};
use kira::info::MockInfoBuilder;
use kira::sound::static_sound::{StaticSoundData, StaticSoundSettings};
use kira::sound::{Sound, SoundData};
use kira::{AudioManager, AudioManagerSettings, Frame, info::Info};
use melody_bay::{
    AudioBuffer, AudioContext, BiquadFilterType, ChannelCountMode, ChannelInterpretation,
    DistanceModel, GraphError, OfflineAudioContext, OfflineAudioContextState, Oversample,
    ParamTimeline, PeriodicWave, Waveform,
};
use std::sync::{Arc, Mutex};

fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= 0.0001,
        "expected {actual} to be close to {expected}"
    );
}

fn assert_close64(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 0.0001,
        "expected {actual} to be close to {expected}"
    );
}

fn rms(frames: &[Frame]) -> f32 {
    let sum = frames
        .iter()
        .map(|frame| {
            let sample = (frame.left + frame.right) * 0.5;
            sample * sample
        })
        .sum::<f32>();
    (sum / frames.len().max(1) as f32).sqrt()
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
    if context.state() == OfflineAudioContextState::Closed {
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

fn started_constant_source(graph: &mut AudioContext) -> melody_bay::ConstantSourceNode {
    let source = graph.create_constant_source();
    source.try_start(0.0).unwrap();
    source
}

fn started_offline_constant_source(
    graph: &mut OfflineAudioContext,
) -> melody_bay::ConstantSourceNode {
    let source = graph.create_constant_source();
    source.try_start(0.0).unwrap();
    source
}

fn started_buffer_source_with_buffer(
    graph: &mut AudioContext,
    buffer: AudioBuffer,
) -> melody_bay::AudioBufferSourceNode {
    let source = graph.create_buffer_source();
    source.try_set_buffer(buffer).unwrap();
    source.try_start(0.0).unwrap();
    source
}

fn started_offline_buffer_source_with_buffer(
    graph: &mut OfflineAudioContext,
    buffer: AudioBuffer,
) -> melody_bay::AudioBufferSourceNode {
    let source = graph.create_buffer_source();
    source.try_set_buffer(buffer).unwrap();
    source.try_start(0.0).unwrap();
    source
}

#[derive(Clone)]
struct CapturingBackendSettings {
    sample_rate: u32,
    captured: Arc<Mutex<Vec<f32>>>,
}

impl Default for CapturingBackendSettings {
    fn default() -> Self {
        Self {
            sample_rate: 1,
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

fn render_graph(graph: AudioContext, sample_rate: u32, frames: usize) -> Vec<Frame> {
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(sample_rate)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = vec![Frame::ZERO; frames];
    sound.process(&mut out, 1.0 / sample_rate as f64, &info);
    out
}

fn render_graph_with_default_sound_rate(graph: AudioContext, frames: usize) -> Vec<Frame> {
    let (mut sound, _) = graph.sound_data().into_sound().expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = vec![Frame::ZERO; frames];
    sound.process(&mut out, 0.0, &info);
    out
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

struct FailingSoundData;

impl SoundData for FailingSoundData {
    type Error = &'static str;
    type Handle = ();

    fn into_sound(self) -> Result<(Box<dyn Sound>, Self::Handle), Self::Error> {
        Err("source failed")
    }
}

impl Sound for TestSound {
    fn process(&mut self, out: &mut [Frame], _dt: f64, _info: &Info) {
        for frame in out {
            *frame = self.frames.get(self.cursor).copied().unwrap_or(Frame::ZERO);
            self.cursor += 1;
        }
    }

    fn finished(&self) -> bool {
        self.cursor >= self.frames.len()
    }
}

#[test]
fn audio_buffer_exposes_webaudio_style_metadata_and_channel_data() {
    let mut buffer = audio_buffer_from_channels(3_000, 3, [vec![0.0, 0.25], vec![0.5, 0.75, 1.0]]);

    assert_eq!(buffer.sample_rate(), 3_000);
    assert_eq!(buffer.length(), 3);
    assert_close(buffer.duration(), 0.001);
    assert_eq!(buffer.number_of_channels(), 2);
    assert_eq!(buffer.channel_data(0), Some(&[0.0, 0.25, 0.0][..]));
    assert_eq!(buffer.channel_data(1), Some(&[0.5, 0.75, 1.0][..]));
    assert_eq!(buffer.channel_data(2), None);

    buffer.channel_data_mut(0).expect("channel exists")[2] = 0.5;

    assert_eq!(buffer.channel_data(0), Some(&[0.0, 0.25, 0.5][..]));
}

#[test]
fn audio_buffer_copies_to_and_from_channels_with_offsets() {
    let mut buffer =
        audio_buffer_from_stereo(3_000, 4, [0.0, 0.25, 0.5, 0.75], [1.0, 0.5, 0.0, -0.5]);
    let mut copied = [9.0, 9.0, 9.0];

    buffer
        .copy_from_channel(&mut copied, 0, 1)
        .expect("copy from channel succeeds");
    assert_eq!(copied, [0.25, 0.5, 0.75]);

    buffer
        .copy_to_channel([0.125, 0.375], 1, 2)
        .expect("copy to channel succeeds");
    assert_eq!(buffer.channel_data(1), Some(&[1.0, 0.5, 0.125, 0.375][..]));

    assert_eq!(
        buffer.copy_from_channel(&mut copied, 9, 0),
        Err(melody_bay::GraphError::InvalidChannel)
    );
    assert_eq!(
        buffer.copy_to_channel([0.0], 9, 0),
        Err(melody_bay::GraphError::InvalidChannel)
    );
}

#[test]
fn audio_buffer_copy_methods_treat_offsets_at_or_beyond_length_as_noops() {
    let mut buffer = audio_buffer_from_mono(3_000, 2, [0.25, 0.5]);
    let mut copied = [9.0, 9.0, 9.0];

    buffer
        .copy_from_channel(&mut copied, 0, 1)
        .expect("partial copy succeeds");
    assert_eq!(copied, [0.5, 9.0, 9.0]);

    buffer
        .copy_from_channel(&mut copied, 0, 2)
        .expect("offset at length is a no-op");
    assert_eq!(copied, [0.5, 9.0, 9.0]);
    buffer
        .copy_from_channel(&mut copied, 0, 3)
        .expect("offset beyond length is a no-op");
    assert_eq!(copied, [0.5, 9.0, 9.0]);

    buffer
        .copy_to_channel([0.75, 1.0], 0, 1)
        .expect("partial copy to channel succeeds");
    assert_eq!(buffer.channel_data(0), Some(&[0.25, 0.75][..]));

    buffer
        .copy_to_channel([0.0], 0, 2)
        .expect("offset at length is a no-op");
    buffer
        .copy_to_channel([0.0], 0, 3)
        .expect("offset beyond length is a no-op");
    assert_eq!(buffer.channel_data(0), Some(&[0.25, 0.75][..]));
}

#[test]
fn graph_render_offline_produces_audio_buffer() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(0.5).unwrap();
        source
    };
    graph
        .connect(source, graph.destination())
        .expect("source connects");

    let buffer = render_context_offline(&graph, 4, 3).expect("graph renders offline");

    assert_eq!(buffer.sample_rate(), 3_000);
    assert_eq!(buffer.length(), 3);
    assert_eq!(buffer.number_of_channels(), 2);
    assert_eq!(buffer.channel_data(0), Some(&[0.5, 0.5, 0.5][..]));
    assert_eq!(buffer.channel_data(1), Some(&[0.5, 0.5, 0.5][..]));
}

#[test]
fn graph_exposes_context_style_timing_metadata() {
    let default_graph = AudioContext::new();
    assert_eq!(default_graph.sample_rate(), 44_100);

    let mut graph = AudioContext::try_new_with_sample_rate(48_000).unwrap();

    assert_eq!(graph.sample_rate(), 48_000);
    assert_close64(graph.current_time(), 0.0);
    assert_close64(graph.base_latency(), 0.0);
    assert_close64(graph.output_latency(), 0.0);
    assert_eq!(graph.state(), OfflineAudioContextState::Suspended);

    graph.resume().expect("graph resumes");
    assert_eq!(graph.state(), OfflineAudioContextState::Running);
    graph.suspend().expect("graph suspends");
    assert_eq!(graph.state(), OfflineAudioContextState::Suspended);
    assert_close64(graph.current_time(), 0.0);
    graph.close().expect("graph closes");
    assert_eq!(graph.state(), OfflineAudioContextState::Closed);
    assert!(render_context_offline(&graph, 4, 1).is_err());
}

#[test]
fn graph_sound_data_defaults_to_context_sample_rate() {
    let mut graph = AudioContext::try_new_with_sample_rate(48_000).unwrap();
    let osc = graph.create_oscillator();
    osc.set_type(Waveform::Sine);
    osc.try_start(0.0).unwrap();
    osc.frequency().set_value(12_000.0).unwrap();
    graph
        .connect(osc, graph.destination())
        .expect("oscillator connects to destination");

    let out = render_graph_with_default_sound_rate(graph, 2);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 1.0);
}

#[test]
fn graph_exposes_and_updates_audio_node_metadata() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let gain = graph.create_gain();
    let splitter = graph.try_create_channel_splitter(2).unwrap();
    let merger = graph.try_create_channel_merger(2).unwrap();

    let source_info = graph.node_info(&source).expect("source info");
    assert_eq!(source_info.number_of_inputs, 0);
    assert_eq!(source_info.number_of_outputs, 1);

    let gain_info = graph.node_info(&gain).expect("gain info");
    assert_eq!(gain_info.number_of_inputs, 1);
    assert_eq!(gain_info.number_of_outputs, 1);
    assert_eq!(gain_info.channel_count, 2);
    assert_eq!(gain_info.channel_count_mode, ChannelCountMode::Max);
    assert_eq!(
        gain_info.channel_interpretation,
        ChannelInterpretation::Speakers
    );

    assert_eq!(
        graph
            .node_info(&splitter)
            .expect("splitter info")
            .number_of_outputs,
        2
    );
    assert_eq!(
        graph
            .node_info(&merger)
            .expect("merger info")
            .number_of_inputs,
        2
    );

    gain.try_set_channel_config(
        1,
        ChannelCountMode::Explicit,
        ChannelInterpretation::Discrete,
    )
    .expect("channel config updates");
    let updated = graph.node_info(&gain).expect("updated gain info");
    assert_eq!(updated.channel_count, 1);
    assert_eq!(updated.channel_count_mode, ChannelCountMode::Explicit);
    assert_eq!(
        updated.channel_interpretation,
        ChannelInterpretation::Discrete
    );
}

#[test]
fn custom_processor_node_transforms_input_frames() {
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

    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(0.5).unwrap();
        source
    };
    let processor = graph.create_audio_worklet_node(Doubler);
    graph.connect(source, &processor).expect("source connects");
    graph
        .connect(&processor, graph.destination())
        .expect("processor connects");

    let buffer = render_context_offline(&graph, 4, 2).expect("graph renders offline");

    assert_eq!(buffer.channel_data(0), Some(&[1.0, 1.0][..]));
    assert_eq!(buffer.channel_data(1), Some(&[1.0, 1.0][..]));
}

#[test]
fn custom_processor_node_can_generate_frames_from_time() {
    struct TimeGenerator;
    impl melody_bay::AudioWorkletProcessor for TimeGenerator {
        fn process(
            &mut self,
            _inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            for port in outputs {
                for output in port {
                    for (frame, sample) in output.iter_mut().enumerate() {
                        let time = context.current_time + frame as f64 * context.sample_dt;
                        *sample = if time >= 0.25 { 0.75 } else { 0.25 };
                    }
                }
            }
            true
        }
    }

    let mut graph = AudioContext::new();
    let processor = graph
        .try_create_audio_worklet_node(
            TimeGenerator,
            melody_bay::AudioWorkletNodeOptions {
                number_of_inputs: 0,
                number_of_outputs: 1,
                output_channel_count: Some(vec![1]),
                ..Default::default()
            },
        )
        .expect("worklet options are valid");
    graph
        .connect(&processor, graph.destination())
        .expect("processor connects");

    let buffer = render_context_offline(&graph, 4, 2).expect("graph renders offline");

    assert_eq!(buffer.channel_data(0), Some(&[0.25, 0.75][..]));
}

#[test]
fn audio_worklet_processor_false_result_stops_future_processing() {
    struct OneShot;
    impl melody_bay::AudioWorkletProcessor for OneShot {
        fn process(
            &mut self,
            _inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            for port in outputs {
                for output in port {
                    output.fill(0.5);
                }
            }
            false
        }
    }

    let mut graph = AudioContext::new();
    let processor = graph
        .try_create_audio_worklet_node(
            OneShot,
            melody_bay::AudioWorkletNodeOptions {
                number_of_inputs: 0,
                number_of_outputs: 1,
                output_channel_count: Some(vec![1]),
                ..Default::default()
            },
        )
        .expect("worklet options are valid");
    graph
        .connect(&processor, graph.destination())
        .expect("processor connects");

    let buffer = render_context_offline(&graph, 4, 129).expect("graph renders offline");

    let samples = buffer.channel_data(0).expect("left channel");
    assert!(samples[..128].iter().all(|sample| *sample == 0.5));
    assert_eq!(samples[128], 0.0);
}

#[test]
fn audio_worklet_processor_can_render_one_quantum_per_callback() {
    #[derive(Clone)]
    struct QuantumCounter {
        calls: Arc<Mutex<Vec<usize>>>,
    }

    impl melody_bay::AudioWorkletProcessor for QuantumCounter {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            let output = worklet_output_channel_mut(outputs, 0, 0).expect("first output channel");
            self.calls
                .lock()
                .expect("calls mutex poisoned")
                .push(output.len());
            for (frame, sample) in output.iter_mut().enumerate() {
                *sample = inputs.len() as f32 + frame as f32;
            }
            true
        }
    }

    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut graph = AudioContext::new();
    let worklet = graph
        .try_create_audio_worklet_node(
            QuantumCounter {
                calls: calls.clone(),
            },
            melody_bay::AudioWorkletNodeOptions {
                number_of_inputs: 0,
                number_of_outputs: 1,
                output_channel_count: Some(vec![1]),
                ..Default::default()
            },
        )
        .expect("worklet options are valid");
    graph
        .connect(&worklet, graph.destination())
        .expect("worklet connects");

    let rendered = render_context_offline(&graph, 4, 4).expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[0.0, 1.0, 2.0, 3.0][..]));
    assert_eq!(
        &calls.lock().expect("calls mutex poisoned")[..],
        &[128],
        "worklet should process one full render quantum"
    );
}

#[test]
fn audio_worklet_quantum_inputs_include_future_upstream_samples() {
    struct QuantumInputCopier;

    impl melody_bay::AudioWorkletProcessor for QuantumInputCopier {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            let input = worklet_input_channel(inputs, 0, 0).expect("first input channel");
            let output = worklet_output_channel_mut(outputs, 0, 0).expect("first output channel");
            let frames = output.len();
            output.copy_from_slice(&input[..frames]);
            true
        }
    }

    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [1.0, 2.0, 3.0, 4.0]),
    );
    let worklet = graph.create_audio_worklet_node(QuantumInputCopier);
    graph.connect(source, &worklet).expect("source connects");
    graph
        .connect(&worklet, graph.destination())
        .expect("worklet connects");

    let rendered = render_context_offline(&graph, 4, 4).expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[1.0, 2.0, 3.0, 4.0][..]));
}

#[test]
fn audio_worklet_a_rate_parameter_arrays_include_future_param_input_samples() {
    struct ParameterInputCopier;

    impl melody_bay::AudioWorkletProcessor for ParameterInputCopier {
        fn process(
            &mut self,
            _inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            let values = context
                .parameter_values
                .get("depth")
                .expect("depth values are present");
            let output = worklet_output_channel_mut(outputs, 0, 0).expect("first output channel");
            let frames = output.len();
            output.copy_from_slice(&values[..frames]);
            true
        }
    }

    let mut graph = AudioContext::new();
    let modulation = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [0.0, 0.25, 0.5, 0.75]),
    );
    let worklet = graph
        .try_create_audio_worklet_node(
            ParameterInputCopier,
            melody_bay::AudioWorkletNodeOptions {
                number_of_inputs: 0,
                number_of_outputs: 1,
                output_channel_count: Some(vec![1]),
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
        .expect("worklet options are valid");
    graph
        .connect_param(modulation, worklet.param("depth").unwrap())
        .expect("parameter input connects");
    graph
        .connect(&worklet, graph.destination())
        .expect("worklet connects");

    let rendered = render_context_offline(&graph, 4, 4).expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[0.0, 0.25, 0.5, 0.75][..]));
}

#[test]
fn audio_worklet_quantum_inputs_include_buffer_source_playback_rate_modulation() {
    struct QuantumInputCopier;

    impl melody_bay::AudioWorkletProcessor for QuantumInputCopier {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            let input = worklet_input_channel(inputs, 0, 0).expect("first input channel");
            let output = worklet_output_channel_mut(outputs, 0, 0).expect("first output channel");
            let frames = output.len();
            output.copy_from_slice(&input[..frames]);
            true
        }
    }

    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [1.0, 2.0, 3.0, 4.0]),
    );
    source.playback_rate().set_value(0.0).unwrap();
    let playback_rate_modulator = started_constant_source(&mut graph);
    playback_rate_modulator.offset().set_value(1.0).unwrap();
    graph
        .connect_param(&playback_rate_modulator, source.playback_rate())
        .expect("playback rate modulator connects");
    let worklet = graph.create_audio_worklet_node(QuantumInputCopier);
    graph.connect(source, &worklet).expect("source connects");
    graph
        .connect(&worklet, graph.destination())
        .expect("worklet connects");

    let rendered = render_context_offline(&graph, 4, 4).expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[1.0, 2.0, 3.0, 4.0][..]));
}

#[test]
fn audio_worklet_quantum_inputs_integrate_buffer_source_playback_rate_changes() {
    struct QuantumInputCopier;

    impl melody_bay::AudioWorkletProcessor for QuantumInputCopier {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            let input = worklet_input_channel(inputs, 0, 0).expect("first input channel");
            let output = worklet_output_channel_mut(outputs, 0, 0).expect("first output channel");
            let frames = output.len();
            output.copy_from_slice(&input[..frames]);
            true
        }
    }

    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(256, 512, (0..512).map(|index| index as f32)),
    );
    source.playback_rate().set_value_at_time(1.0, 0.0).unwrap();
    source.playback_rate().set_value_at_time(2.0, 0.5).unwrap();
    let worklet = graph.create_audio_worklet_node(QuantumInputCopier);
    graph.connect(source, &worklet).expect("source connects");
    graph
        .connect(&worklet, graph.destination())
        .expect("worklet connects");

    let rendered = render_context_offline(&graph, 256, 193).expect("graph renders");

    assert_eq!(rendered.channel_data(0).unwrap()[192], 256.0);
}

#[test]
fn audio_worklet_input_port_keeps_slow_buffer_source_active_past_wall_duration() {
    struct QuantumInputCopier;

    impl melody_bay::AudioWorkletProcessor for QuantumInputCopier {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            let input = worklet_input_channel(inputs, 0, 0).expect("first input channel");
            let output = worklet_output_channel_mut(outputs, 0, 0).expect("first output channel");
            output[..6].copy_from_slice(&input[..6]);
            true
        }
    }

    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [0.0, 0.25, 0.5, 0.75]),
    );
    source.playback_rate().set_value(0.5).unwrap();
    let worklet = graph.create_audio_worklet_node(QuantumInputCopier);
    graph.connect(source, &worklet).expect("source connects");
    graph
        .connect(&worklet, graph.destination())
        .expect("worklet connects");

    let rendered = render_context_offline(&graph, 4, 6).expect("graph renders");

    assert_eq!(
        rendered.channel_data(0),
        Some(&[0.0, 0.0, 0.25, 0.25, 0.5, 0.5][..])
    );
}

#[test]
fn audio_worklet_quantum_inputs_include_simple_upstream_processing() {
    struct QuantumInputCopier;

    impl melody_bay::AudioWorkletProcessor for QuantumInputCopier {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            let input = worklet_input_channel(inputs, 0, 0).expect("first input channel");
            let output = worklet_output_channel_mut(outputs, 0, 0).expect("first output channel");
            output[..4].copy_from_slice(&input[..4]);
            true
        }
    }

    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [1.0, 2.0, 3.0, 4.0]),
    );
    let gain = graph.create_gain();
    gain.gain().set_value(0.5).unwrap();
    let worklet = graph.create_audio_worklet_node(QuantumInputCopier);
    graph.connect(source, &gain).expect("source connects");
    graph.connect(&gain, &worklet).expect("gain connects");
    graph
        .connect(&worklet, graph.destination())
        .expect("worklet connects");

    let rendered = render_context_offline(&graph, 4, 4).expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[0.5, 1.0, 1.5, 2.0][..]));
}

#[test]
fn audio_worklet_quantum_inputs_include_future_channel_merger_outputs() {
    struct QuantumInputSummer;

    impl melody_bay::AudioWorkletProcessor for QuantumInputSummer {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            let left = worklet_input_channel(inputs, 0, 0).expect("left input channel");
            let right = worklet_input_channel(inputs, 0, 1).expect("right input channel");
            let output = worklet_output_channel_mut(outputs, 0, 0).expect("first output channel");
            for frame in 0..4 {
                output[frame] = left[frame] + right[frame];
            }
            true
        }
    }

    let mut graph = AudioContext::new();
    let left = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [1.0, 2.0, 3.0, 4.0]),
    );
    let right = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [10.0, 20.0, 30.0, 40.0]),
    );
    let merger = graph.try_create_channel_merger(2).unwrap();
    let worklet = graph.create_audio_worklet_node(QuantumInputSummer);
    graph
        .connect_with_indices(left, 0, &merger, 0)
        .expect("left connects");
    graph
        .connect_with_indices(right, 0, &merger, 1)
        .expect("right connects");
    graph.connect(&merger, &worklet).expect("merger connects");
    graph
        .connect(&worklet, graph.destination())
        .expect("worklet connects");

    let rendered = render_context_offline(&graph, 4, 4).expect("graph renders");

    assert_eq!(
        rendered.channel_data(0),
        Some(&[11.0, 22.0, 33.0, 44.0][..])
    );
}

#[test]
fn audio_worklet_quantum_inputs_include_future_waveshaper_samples() {
    struct QuantumInputCopier;

    impl melody_bay::AudioWorkletProcessor for QuantumInputCopier {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            let input = worklet_input_channel(inputs, 0, 0).expect("first input channel");
            let output = worklet_output_channel_mut(outputs, 0, 0).expect("first output channel");
            output[..4].copy_from_slice(&input[..4]);
            true
        }
    }

    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [-1.0, -0.5, 0.0, 1.0]),
    );
    let shaper = graph.create_wave_shaper();
    shaper.try_curve([10.0, 20.0, 30.0]).unwrap();
    let worklet = graph.create_audio_worklet_node(QuantumInputCopier);
    graph.connect(source, &shaper).expect("source connects");
    graph.connect(&shaper, &worklet).expect("shaper connects");
    graph
        .connect(&worklet, graph.destination())
        .expect("worklet connects");

    let rendered = render_context_offline(&graph, 4, 4).expect("graph renders");

    assert_eq!(
        rendered.channel_data(0),
        Some(&[10.0, 15.0, 20.0, 30.0][..])
    );
}

#[test]
fn audio_worklet_quantum_inputs_include_future_biquad_samples() {
    struct QuantumInputCopier;

    impl melody_bay::AudioWorkletProcessor for QuantumInputCopier {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            let input = worklet_input_channel(inputs, 0, 0).expect("first input channel");
            let output = worklet_output_channel_mut(outputs, 0, 0).expect("first output channel");
            output[..4].copy_from_slice(&input[..4]);
            true
        }
    }

    fn filtered_graph(use_worklet: bool) -> AudioBuffer {
        let mut graph = AudioContext::new();
        let source = started_buffer_source_with_buffer(
            &mut graph,
            audio_buffer_from_mono(4, 4, [0.0, 1.0, 1.0, 1.0]),
        );
        let filter = graph.create_biquad_filter();
        filter.set_type(BiquadFilterType::Lowpass);
        filter.frequency().set_value(1.0).unwrap();
        graph.connect(source, &filter).expect("source connects");
        if use_worklet {
            let worklet = graph.create_audio_worklet_node(QuantumInputCopier);
            graph.connect(&filter, &worklet).expect("filter connects");
            graph
                .connect(&worklet, graph.destination())
                .expect("worklet connects");
        } else {
            graph
                .connect(&filter, graph.destination())
                .expect("filter connects");
        }
        render_context_offline(&graph, 4, 4).expect("graph renders")
    }

    let direct = filtered_graph(false);
    let through_worklet = filtered_graph(true);

    assert_eq!(through_worklet.channel_data(0), direct.channel_data(0));
}

#[test]
fn audio_worklet_quantum_inputs_include_future_iir_samples() {
    struct QuantumInputCopier;

    impl melody_bay::AudioWorkletProcessor for QuantumInputCopier {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            let input = worklet_input_channel(inputs, 0, 0).expect("first input channel");
            let output = worklet_output_channel_mut(outputs, 0, 0).expect("first output channel");
            output[..4].copy_from_slice(&input[..4]);
            true
        }
    }

    fn filtered_graph(use_worklet: bool) -> AudioBuffer {
        let mut graph = AudioContext::new();
        let source = started_buffer_source_with_buffer(
            &mut graph,
            audio_buffer_from_mono(4, 4, [1.0, 0.0, 0.0, 0.0]),
        );
        let filter = graph.try_create_iir_filter([0.5, 0.5], [1.0]).unwrap();
        graph.connect(source, &filter).expect("source connects");
        if use_worklet {
            let worklet = graph.create_audio_worklet_node(QuantumInputCopier);
            graph.connect(&filter, &worklet).expect("filter connects");
            graph
                .connect(&worklet, graph.destination())
                .expect("worklet connects");
        } else {
            graph
                .connect(&filter, graph.destination())
                .expect("filter connects");
        }
        render_context_offline(&graph, 4, 4).expect("graph renders")
    }

    let direct = filtered_graph(false);
    let through_worklet = filtered_graph(true);

    assert_eq!(through_worklet.channel_data(0), direct.channel_data(0));
}

#[test]
fn audio_worklet_quantum_inputs_include_future_delay_samples() {
    struct QuantumInputCopier;

    impl melody_bay::AudioWorkletProcessor for QuantumInputCopier {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            let input = worklet_input_channel(inputs, 0, 0).expect("first input channel");
            let output = worklet_output_channel_mut(outputs, 0, 0).expect("first output channel");
            output[..4].copy_from_slice(&input[..4]);
            true
        }
    }

    fn delayed_graph(use_worklet: bool) -> AudioBuffer {
        let mut graph = AudioContext::new();
        let source = started_buffer_source_with_buffer(
            &mut graph,
            audio_buffer_from_mono(4, 4, [1.0, 0.0, 0.0, 0.0]),
        );
        let delay = graph.try_create_delay(1.0).unwrap();
        delay.delay_time().set_value(0.5).unwrap();
        graph.connect(source, &delay).expect("source connects");
        if use_worklet {
            let worklet = graph.create_audio_worklet_node(QuantumInputCopier);
            graph.connect(&delay, &worklet).expect("delay connects");
            graph
                .connect(&worklet, graph.destination())
                .expect("worklet connects");
        } else {
            graph
                .connect(&delay, graph.destination())
                .expect("delay connects");
        }
        render_context_offline(&graph, 4, 4).expect("graph renders")
    }

    let direct = delayed_graph(false);
    let through_worklet = delayed_graph(true);

    assert_eq!(through_worklet.channel_data(0), direct.channel_data(0));
}

#[test]
fn audio_worklet_input_port_includes_delay_tail_after_source_stops() {
    struct QuantumInputProbe {
        input_lengths: Arc<Mutex<Vec<usize>>>,
    }

    impl melody_bay::AudioWorkletProcessor for QuantumInputProbe {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            self.input_lengths
                .lock()
                .expect("input length mutex poisoned")
                .push(inputs.first().map_or(usize::MAX, Vec::len));
            let input = worklet_input_channel(inputs, 0, 0).expect("delay tail input channel");
            let output = worklet_output_channel_mut(outputs, 0, 0).expect("first output channel");
            output[..2].copy_from_slice(&input[..2]);
            true
        }
    }

    let input_lengths = Arc::new(Mutex::new(Vec::new()));
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(&mut graph, audio_buffer_from_mono(4, 1, [1.0]));
    source.try_stop(0.25).unwrap();
    let delay = graph.try_create_delay(1.0).unwrap();
    delay.delay_time().set_value(0.25).unwrap();
    let worklet = graph.create_audio_worklet_node(QuantumInputProbe {
        input_lengths: input_lengths.clone(),
    });
    graph.connect(source, &delay).expect("source connects");
    graph.connect(&delay, &worklet).expect("delay connects");
    graph
        .connect(&worklet, graph.destination())
        .expect("worklet connects");

    let rendered = render_context_offline(&graph, 4, 2).expect("graph renders");

    assert_eq!(
        &input_lengths.lock().expect("input length mutex poisoned")[..],
        &[1],
        "delay tail should keep the worklet input channel array active"
    );
    assert_eq!(rendered.channel_data(0), Some(&[0.0, 1.0][..]));
}

#[test]
fn audio_worklet_input_port_includes_convolver_tail_after_source_stops() {
    struct QuantumInputProbe {
        input_lengths: Arc<Mutex<Vec<usize>>>,
    }

    impl melody_bay::AudioWorkletProcessor for QuantumInputProbe {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            self.input_lengths
                .lock()
                .expect("input length mutex poisoned")
                .push(inputs.first().map_or(usize::MAX, Vec::len));
            let input = worklet_input_channel(inputs, 0, 0).expect("convolver tail input channel");
            let output = worklet_output_channel_mut(outputs, 0, 0).expect("first output channel");
            output[..2].copy_from_slice(&input[..2]);
            true
        }
    }

    let input_lengths = Arc::new(Mutex::new(Vec::new()));
    let mut graph = AudioContext::try_new_with_sample_rate(3_000).unwrap();
    let source =
        started_buffer_source_with_buffer(&mut graph, audio_buffer_from_mono(3_000, 1, [1.0]));
    source.try_stop(1.0 / 3_000.0).unwrap();
    let convolver = graph.create_convolver();
    convolver.set_normalize(false);
    convolver
        .try_buffer(audio_buffer_from_mono(3_000, 3, [0.0, 1.0, 0.0]))
        .expect("valid impulse response");
    let worklet = graph.create_audio_worklet_node(QuantumInputProbe {
        input_lengths: input_lengths.clone(),
    });
    graph.connect(source, &convolver).expect("source connects");
    graph
        .connect(&convolver, &worklet)
        .expect("convolver connects");
    graph
        .connect(&worklet, graph.destination())
        .expect("worklet connects");

    let rendered = render_context_offline(&graph, 3_000, 2).expect("graph renders");

    assert_eq!(
        &input_lengths.lock().expect("input length mutex poisoned")[..],
        &[1],
        "convolver tail should keep the worklet input channel array active"
    );
    assert_eq!(rendered.channel_data(0), Some(&[0.0, 1.0][..]));
}

#[test]
fn audio_worklet_input_port_includes_dynamics_compressor_tail_after_source_stops() {
    const SAMPLE_RATE: u32 = 3_000;
    const LOOKAHEAD_FRAMES: usize = 18;

    struct QuantumInputProbe {
        input_lengths: Arc<Mutex<Vec<usize>>>,
    }

    impl melody_bay::AudioWorkletProcessor for QuantumInputProbe {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            self.input_lengths
                .lock()
                .expect("input length mutex poisoned")
                .push(inputs.first().map_or(usize::MAX, Vec::len));
            let input = worklet_input_channel(inputs, 0, 0).expect("compressor tail input channel");
            let output = worklet_output_channel_mut(outputs, 0, 0).expect("first output channel");
            output[..=LOOKAHEAD_FRAMES].copy_from_slice(&input[..=LOOKAHEAD_FRAMES]);
            true
        }
    }

    let input_lengths = Arc::new(Mutex::new(Vec::new()));
    let mut graph = AudioContext::try_new_with_sample_rate(SAMPLE_RATE).unwrap();
    let source = started_constant_source(&mut graph);
    source.offset().set_value(1.0).unwrap();
    source
        .try_stop(1.0 / SAMPLE_RATE as f64)
        .expect("source stops after one frame");
    let compressor = graph.create_dynamics_compressor();
    compressor.threshold().set_value(0.0).unwrap();
    compressor.ratio().set_value(1.0).unwrap();
    compressor.knee().set_value(0.0).unwrap();
    compressor.attack().set_value(0.0).unwrap();
    compressor.release().set_value(0.0).unwrap();
    let worklet = graph.create_audio_worklet_node(QuantumInputProbe {
        input_lengths: input_lengths.clone(),
    });
    graph
        .connect(&source, &compressor)
        .expect("source connects");
    graph
        .connect(&compressor, &worklet)
        .expect("compressor connects");
    graph
        .connect(&worklet, graph.destination())
        .expect("worklet connects");

    let rendered =
        render_context_offline(&graph, SAMPLE_RATE, LOOKAHEAD_FRAMES + 1).expect("graph renders");
    let samples = rendered.channel_data(0).expect("left channel");

    assert_eq!(
        &input_lengths.lock().expect("input length mutex poisoned")[..],
        &[1],
        "compressor lookahead tail should keep the worklet input channel array active"
    );
    assert!(
        samples[..LOOKAHEAD_FRAMES]
            .iter()
            .all(|sample| sample.abs() <= f32::EPSILON)
    );
    assert_close(samples[LOOKAHEAD_FRAMES], 1.0);
}

#[test]
fn audio_worklet_quantum_inputs_include_future_stereo_panner_samples() {
    struct QuantumInputCopier;

    impl melody_bay::AudioWorkletProcessor for QuantumInputCopier {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            for channel in 0..2 {
                let input =
                    worklet_input_channel(inputs, 0, channel).expect("stereo input channel");
                let output =
                    worklet_output_channel_mut(outputs, 0, channel).expect("stereo output channel");
                output[..4].copy_from_slice(&input[..4]);
            }
            true
        }
    }

    fn panned_graph(use_worklet: bool) -> AudioBuffer {
        let mut graph = AudioContext::new();
        let source = started_buffer_source_with_buffer(
            &mut graph,
            audio_buffer_from_mono(4, 4, [0.25, 0.5, 0.75, 1.0]),
        );
        let panner = graph.create_stereo_panner();
        panner.pan().set_value(1.0).unwrap();
        graph.connect(source, &panner).expect("source connects");
        if use_worklet {
            let worklet = graph
                .try_create_audio_worklet_node(
                    QuantumInputCopier,
                    melody_bay::AudioWorkletNodeOptions {
                        number_of_inputs: 1,
                        number_of_outputs: 1,
                        output_channel_count: Some(vec![2]),
                        ..Default::default()
                    },
                )
                .unwrap();
            graph.connect(&panner, &worklet).expect("panner connects");
            graph
                .connect(&worklet, graph.destination())
                .expect("worklet connects");
        } else {
            graph
                .connect(&panner, graph.destination())
                .expect("panner connects");
        }
        render_context_offline(&graph, 4, 4).expect("graph renders")
    }

    let direct = panned_graph(false);
    let through_worklet = panned_graph(true);

    assert_eq!(through_worklet.channel_data(0), direct.channel_data(0));
    assert_eq!(through_worklet.channel_data(1), direct.channel_data(1));
}

#[test]
fn audio_worklet_quantum_inputs_include_future_panner_samples() {
    struct QuantumInputCopier;

    impl melody_bay::AudioWorkletProcessor for QuantumInputCopier {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            for channel in 0..2 {
                let input =
                    worklet_input_channel(inputs, 0, channel).expect("stereo input channel");
                let output =
                    worklet_output_channel_mut(outputs, 0, channel).expect("stereo output channel");
                output[..4].copy_from_slice(&input[..4]);
            }
            true
        }
    }

    fn panned_graph(use_worklet: bool) -> AudioBuffer {
        let mut graph = AudioContext::new();
        let source = started_buffer_source_with_buffer(
            &mut graph,
            audio_buffer_from_mono(4, 4, [0.25, 0.5, 0.75, 1.0]),
        );
        let panner = graph.create_panner();
        panner.set_distance_model(DistanceModel::Inverse);
        panner.position_x().set_value(1.0).unwrap();
        panner.position_y().set_value(0.0).unwrap();
        panner.position_z().set_value(0.0).unwrap();
        graph.connect(source, &panner).expect("source connects");
        if use_worklet {
            let worklet = graph
                .try_create_audio_worklet_node(
                    QuantumInputCopier,
                    melody_bay::AudioWorkletNodeOptions {
                        number_of_inputs: 1,
                        number_of_outputs: 1,
                        output_channel_count: Some(vec![2]),
                        ..Default::default()
                    },
                )
                .unwrap();
            graph.connect(&panner, &worklet).expect("panner connects");
            graph
                .connect(&worklet, graph.destination())
                .expect("worklet connects");
        } else {
            graph
                .connect(&panner, graph.destination())
                .expect("panner connects");
        }
        render_context_offline(&graph, 4, 4).expect("graph renders")
    }

    let direct = panned_graph(false);
    let through_worklet = panned_graph(true);

    assert_eq!(through_worklet.channel_data(0), direct.channel_data(0));
    assert_eq!(through_worklet.channel_data(1), direct.channel_data(1));
}

#[test]
fn audio_worklet_quantum_inputs_include_future_convolver_samples() {
    struct QuantumInputCopier;

    impl melody_bay::AudioWorkletProcessor for QuantumInputCopier {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            let input = worklet_input_channel(inputs, 0, 0).expect("first input channel");
            let output = worklet_output_channel_mut(outputs, 0, 0).expect("first output channel");
            output[..4].copy_from_slice(&input[..4]);
            true
        }
    }

    fn convolved_graph(use_worklet: bool) -> AudioBuffer {
        let sample_rate = 3_000;
        let mut graph = AudioContext::try_new_with_sample_rate(sample_rate).unwrap();
        let source = started_buffer_source_with_buffer(
            &mut graph,
            audio_buffer_from_mono(sample_rate, 4, [1.0, 0.0, 0.0, 0.0]),
        );
        let convolver = graph.create_convolver();
        convolver.set_normalize(false);
        convolver
            .try_buffer(audio_buffer_from_mono(sample_rate, 2, [0.5, 1.0]))
            .unwrap();
        graph.connect(source, &convolver).expect("source connects");
        if use_worklet {
            let worklet = graph.create_audio_worklet_node(QuantumInputCopier);
            graph
                .connect(&convolver, &worklet)
                .expect("convolver connects");
            graph
                .connect(&worklet, graph.destination())
                .expect("worklet connects");
        } else {
            graph
                .connect(&convolver, graph.destination())
                .expect("convolver connects");
        }
        render_context_offline(&graph, sample_rate, 4).expect("graph renders")
    }

    let direct = convolved_graph(false);
    let through_worklet = convolved_graph(true);

    assert_eq!(through_worklet.channel_data(0), direct.channel_data(0));
}

#[test]
fn audio_worklet_quantum_inputs_include_future_dynamics_compressor_samples() {
    struct QuantumInputCopier;

    impl melody_bay::AudioWorkletProcessor for QuantumInputCopier {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            let input = worklet_input_channel(inputs, 0, 0).expect("first input channel");
            let output = worklet_output_channel_mut(outputs, 0, 0).expect("first output channel");
            output[..4].copy_from_slice(&input[..4]);
            true
        }
    }

    fn compressed_graph(use_worklet: bool) -> AudioBuffer {
        let mut graph = AudioContext::new();
        let source = started_buffer_source_with_buffer(
            &mut graph,
            audio_buffer_from_mono(4, 4, [0.25, 1.0, 0.5, 1.0]),
        );
        let compressor = graph.create_dynamics_compressor();
        compressor.threshold().set_value(-12.0).unwrap();
        compressor.ratio().set_value(4.0).unwrap();
        compressor.knee().set_value(0.0).unwrap();
        compressor.attack().set_value(0.0).unwrap();
        compressor.release().set_value(0.0).unwrap();
        graph.connect(source, &compressor).expect("source connects");
        if use_worklet {
            let worklet = graph.create_audio_worklet_node(QuantumInputCopier);
            graph
                .connect(&compressor, &worklet)
                .expect("compressor connects");
            graph
                .connect(&worklet, graph.destination())
                .expect("worklet connects");
        } else {
            graph
                .connect(&compressor, graph.destination())
                .expect("compressor connects");
        }
        render_context_offline(&graph, 4, 4).expect("graph renders")
    }

    let direct = compressed_graph(false);
    let through_worklet = compressed_graph(true);

    assert_eq!(through_worklet.channel_data(0), direct.channel_data(0));
}

#[test]
fn audio_worklet_quantum_inputs_include_future_analyser_samples() {
    struct QuantumInputCopier;

    impl melody_bay::AudioWorkletProcessor for QuantumInputCopier {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            let input = worklet_input_channel(inputs, 0, 0).expect("first input channel");
            let output = worklet_output_channel_mut(outputs, 0, 0).expect("first output channel");
            output[..4].copy_from_slice(&input[..4]);
            true
        }
    }

    fn analysed_graph(use_worklet: bool) -> AudioBuffer {
        let mut graph = AudioContext::new();
        let source = started_buffer_source_with_buffer(
            &mut graph,
            audio_buffer_from_mono(4, 4, [0.25, 0.5, 0.75, 1.0]),
        );
        let analyser = graph.create_analyser();
        graph.connect(source, &analyser).expect("source connects");
        if use_worklet {
            let worklet = graph.create_audio_worklet_node(QuantumInputCopier);
            graph
                .connect(&analyser, &worklet)
                .expect("analyser connects");
            graph
                .connect(&worklet, graph.destination())
                .expect("worklet connects");
        } else {
            graph
                .connect(&analyser, graph.destination())
                .expect("analyser connects");
        }
        render_context_offline(&graph, 4, 4).expect("graph renders")
    }

    let direct = analysed_graph(false);
    let through_worklet = analysed_graph(true);

    assert_eq!(through_worklet.channel_data(0), direct.channel_data(0));
}

#[test]
fn audio_worklet_quantum_inputs_include_future_oscillator_samples() {
    struct QuantumInputCopier;

    impl melody_bay::AudioWorkletProcessor for QuantumInputCopier {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            let input = worklet_input_channel(inputs, 0, 0).expect("first input channel");
            let output = worklet_output_channel_mut(outputs, 0, 0).expect("first output channel");
            output[..4].copy_from_slice(&input[..4]);
            true
        }
    }

    let mut graph = AudioContext::new();
    let oscillator = graph.create_oscillator();
    oscillator.set_type(Waveform::Sine);
    oscillator.frequency().set_value(1.0).unwrap();
    oscillator.try_start(0.0).unwrap();
    oscillator.try_stop(1.0).unwrap();
    let worklet = graph.create_audio_worklet_node(QuantumInputCopier);
    graph
        .connect(oscillator, &worklet)
        .expect("oscillator connects");
    graph
        .connect(&worklet, graph.destination())
        .expect("worklet connects");

    let rendered = render_context_offline(&graph, 4, 4).expect("graph renders");
    let samples = rendered.channel_data(0).expect("left channel");

    assert_close(samples[0], 0.0);
    assert_close(samples[1], 1.0);
    assert_close(samples[2], 0.0);
    assert_close(samples[3], -1.0);
}

#[test]
fn audio_worklet_quantum_inputs_include_oscillator_param_modulation() {
    struct QuantumInputCopier;

    impl melody_bay::AudioWorkletProcessor for QuantumInputCopier {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            let input = worklet_input_channel(inputs, 0, 0).expect("first input channel");
            let output = worklet_output_channel_mut(outputs, 0, 0).expect("first output channel");
            output[..4].copy_from_slice(&input[..4]);
            true
        }
    }

    let mut graph = AudioContext::new();
    let oscillator = graph.create_oscillator();
    oscillator.set_type(Waveform::Sine);
    oscillator.frequency().set_value(0.0).unwrap();
    oscillator.try_start(0.0).unwrap();
    oscillator.try_stop(1.0).unwrap();
    let frequency_modulator = started_constant_source(&mut graph);
    frequency_modulator.offset().set_value(1.0).unwrap();
    graph
        .connect_param(&frequency_modulator, oscillator.frequency())
        .expect("frequency modulator connects");
    let worklet = graph.create_audio_worklet_node(QuantumInputCopier);
    graph
        .connect(oscillator, &worklet)
        .expect("oscillator connects");
    graph
        .connect(&worklet, graph.destination())
        .expect("worklet connects");

    let rendered = render_context_offline(&graph, 4, 4).expect("graph renders");
    let samples = rendered.channel_data(0).expect("left channel");

    assert_close(samples[0], 0.0);
    assert_close(samples[1], 1.0);
    assert_close(samples[2], 0.0);
    assert_close(samples[3], -1.0);
}

#[test]
fn audio_worklet_default_output_channel_count_is_mono() {
    struct ChannelCountProbe;
    impl melody_bay::AudioWorkletProcessor for ChannelCountProbe {
        fn process(
            &mut self,
            _inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            if let Some(output) = worklet_output_channel_mut(outputs, 0, 0) {
                output.fill(0.25);
            }
            if let Some(output) = worklet_output_channel_mut(outputs, 0, 1) {
                output.fill(0.75);
            }
            true
        }
    }

    let mut graph = AudioContext::new();
    let processor = graph.create_audio_worklet_node(ChannelCountProbe);
    graph
        .connect(&processor, graph.destination())
        .expect("processor connects");

    let buffer = render_context_offline(&graph, 4, 1).expect("graph renders offline");

    assert_eq!(buffer.channel_data(0), Some(&[0.25][..]));
    assert_eq!(buffer.channel_data(1), Some(&[0.25][..]));
}

#[test]
fn audio_worklet_omitted_output_channel_count_follows_single_input() {
    struct Passthrough;
    impl melody_bay::AudioWorkletProcessor for Passthrough {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            let output_port = outputs.first_mut().expect("first output port");
            let input_port = inputs.first().expect("first input port");
            assert_eq!(
                output_port.len(),
                2,
                "omitted outputChannelCount should dynamically match stereo input"
            );
            for (output, input) in output_port.iter_mut().zip(input_port.iter()) {
                output[..2].copy_from_slice(&input[..2]);
            }
            true
        }
    }

    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_stereo(4, 2, [0.25, 0.5], [0.75, 1.0]),
    );
    let worklet = graph.create_audio_worklet_node(Passthrough);
    graph.connect(source, &worklet).expect("source connects");
    graph
        .connect(&worklet, graph.destination())
        .expect("worklet connects");

    let buffer = render_context_offline(&graph, 4, 2).expect("graph renders offline");

    assert_eq!(buffer.channel_data(0), Some(&[0.25, 0.5][..]));
    assert_eq!(buffer.channel_data(1), Some(&[0.75, 1.0][..]));
}

#[test]
fn custom_processor_node_reports_configured_input_output_counts() {
    struct Passthrough;
    impl melody_bay::AudioWorkletProcessor for Passthrough {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            for (port_index, port) in outputs.iter_mut().enumerate() {
                let Some(input_port) = inputs.get(port_index) else {
                    continue;
                };
                for (channel_index, output) in port.iter_mut().enumerate() {
                    if let Some(input) = input_port.get(channel_index) {
                        let frames = output.len();
                        output.copy_from_slice(&input[..frames]);
                    }
                }
            }
            true
        }
    }

    let mut graph = AudioContext::new();
    let processor = graph
        .try_create_audio_worklet_node(
            Passthrough,
            melody_bay::AudioWorkletNodeOptions {
                number_of_inputs: 2,
                number_of_outputs: 3,
                output_channel_count: Some(vec![1, 1, 1]),
                ..Default::default()
            },
        )
        .expect("worklet options are valid");

    let info = graph.node_info(&processor).expect("processor info");

    assert_eq!(info.number_of_inputs, 2);
    assert_eq!(info.number_of_outputs, 3);
}

#[test]
fn audio_worklet_quantum_input_preserves_indexed_input_ports() {
    struct SecondInputCopier;

    impl melody_bay::AudioWorkletProcessor for SecondInputCopier {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            let second_input =
                worklet_input_channel(inputs, 1, 0).expect("second indexed input port");
            let output = worklet_output_channel_mut(outputs, 0, 0).expect("first output channel");
            output[..2].copy_from_slice(&second_input[..2]);
            true
        }
    }

    let mut graph = AudioContext::new();
    let first =
        started_buffer_source_with_buffer(&mut graph, audio_buffer_from_mono(4, 2, [0.25, 0.5]));
    let second =
        started_buffer_source_with_buffer(&mut graph, audio_buffer_from_mono(4, 2, [0.75, 1.0]));
    let worklet = graph
        .try_create_audio_worklet_node(
            SecondInputCopier,
            melody_bay::AudioWorkletNodeOptions {
                number_of_inputs: 2,
                number_of_outputs: 1,
                output_channel_count: Some(vec![1]),
                ..Default::default()
            },
        )
        .expect("worklet options are valid");
    graph
        .connect_with_indices(first, 0, &worklet, 0)
        .expect("first source connects to input 0");
    graph
        .connect_with_indices(second, 0, &worklet, 1)
        .expect("second source connects to input 1");
    graph
        .connect(&worklet, graph.destination())
        .expect("worklet connects");

    let rendered = render_context_offline(&graph, 4, 2).expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[0.75, 1.0][..]));
}

#[test]
fn audio_worklet_nested_quantum_api_preserves_input_and_output_ports() {
    struct PortRouter;

    impl melody_bay::AudioWorkletProcessor for PortRouter {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            let first_input = inputs
                .first()
                .and_then(|port| port.first())
                .expect("first input channel");
            let second_input = inputs
                .get(1)
                .and_then(|port| port.first())
                .expect("second input channel");
            outputs[0][0][..2].copy_from_slice(&first_input[..2]);
            outputs[1][0][..2].copy_from_slice(&second_input[..2]);
            true
        }
    }

    let mut graph = AudioContext::new();
    let first = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(0.25).unwrap();
        source
    };
    let second = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(0.75).unwrap();
        source
    };
    let worklet = graph
        .try_create_audio_worklet_node(
            PortRouter,
            melody_bay::AudioWorkletNodeOptions {
                number_of_inputs: 2,
                number_of_outputs: 2,
                output_channel_count: Some(vec![1, 1]),
                ..Default::default()
            },
        )
        .expect("worklet options are valid");
    let merger = graph.try_create_channel_merger(2).unwrap();
    graph
        .connect_with_indices(first, 0, &worklet, 0)
        .expect("first input connects");
    graph
        .connect_with_indices(second, 0, &worklet, 1)
        .expect("second input connects");
    graph
        .connect_with_indices(&worklet, 0, &merger, 0)
        .expect("first worklet output connects");
    graph
        .connect_with_indices(&worklet, 1, &merger, 1)
        .expect("second worklet output connects");
    graph
        .connect(&merger, graph.destination())
        .expect("merger connects");

    let rendered = render_context_offline(&graph, 4, 2).expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[0.25, 0.25][..]));
    assert_eq!(rendered.channel_data(1), Some(&[0.75, 0.75][..]));
}

#[test]
fn audio_worklet_processor_can_implement_only_nested_quantum_callback() {
    struct NestedOnly;

    impl melody_bay::AudioWorkletProcessor for NestedOnly {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            let input = inputs
                .first()
                .and_then(|port| port.first())
                .expect("first input channel");
            outputs[0][0][..2].copy_from_slice(&input[..2]);
            true
        }
    }

    let mut graph = AudioContext::new();
    let source =
        started_buffer_source_with_buffer(&mut graph, audio_buffer_from_mono(4, 2, [0.25, 0.5]));
    let worklet = graph.create_audio_worklet_node(NestedOnly);
    graph.connect(source, &worklet).expect("source connects");
    graph
        .connect(&worklet, graph.destination())
        .expect("worklet connects");

    let rendered = render_context_offline(&graph, 4, 2).expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[0.25, 0.5][..]));
}

#[test]
fn audio_worklet_input_only_processor_receives_no_output_channels() {
    struct InputOnlyProbe {
        output_lengths: Arc<Mutex<Vec<usize>>>,
    }

    impl melody_bay::AudioWorkletProcessor for InputOnlyProbe {
        fn process(
            &mut self,
            _inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            self.output_lengths
                .lock()
                .expect("output length mutex poisoned")
                .push(outputs.len());
            true
        }
    }

    let output_lengths = Arc::new(Mutex::new(Vec::new()));
    let mut graph = AudioContext::new();
    let source = started_constant_source(&mut graph);
    let worklet = graph
        .try_create_audio_worklet_node(
            InputOnlyProbe {
                output_lengths: output_lengths.clone(),
            },
            melody_bay::AudioWorkletNodeOptions {
                number_of_inputs: 1,
                number_of_outputs: 0,
                output_channel_count: Some(vec![]),
                ..Default::default()
            },
        )
        .expect("input-only worklet options are valid");
    graph.connect(source, &worklet).expect("source connects");

    render_context_offline(&graph, 4, 1).expect("graph renders");

    assert_eq!(
        &output_lengths.lock().expect("output length mutex poisoned")[..],
        &[0],
        "input-only worklets should receive an empty output channel slice"
    );
}

#[test]
fn audio_worklet_unconnected_input_port_has_zero_channels() {
    struct InputChannelProbe {
        input_lengths: Arc<Mutex<Vec<usize>>>,
    }

    impl melody_bay::AudioWorkletProcessor for InputChannelProbe {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            _outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            self.input_lengths
                .lock()
                .expect("input length mutex poisoned")
                .push(inputs.first().map_or(usize::MAX, Vec::len));
            true
        }
    }

    let input_lengths = Arc::new(Mutex::new(Vec::new()));
    let mut graph = AudioContext::new();
    let worklet = graph.create_audio_worklet_node(InputChannelProbe {
        input_lengths: input_lengths.clone(),
    });
    graph
        .connect(&worklet, graph.destination())
        .expect("worklet connects");

    render_context_offline(&graph, 4, 1).expect("graph renders");

    assert_eq!(
        &input_lengths.lock().expect("input length mutex poisoned")[..],
        &[0],
        "unconnected AudioWorklet inputs should be empty channel arrays"
    );
}

#[test]
fn audio_worklet_input_port_ignores_connected_inactive_source() {
    struct InputChannelProbe {
        input_lengths: Arc<Mutex<Vec<usize>>>,
    }

    impl melody_bay::AudioWorkletProcessor for InputChannelProbe {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            _outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            self.input_lengths
                .lock()
                .expect("input length mutex poisoned")
                .push(inputs.first().map_or(usize::MAX, Vec::len));
            true
        }
    }

    let input_lengths = Arc::new(Mutex::new(Vec::new()));
    let mut graph = AudioContext::new();
    let inactive = graph.create_constant_source();
    inactive.offset().set_value(0.0).unwrap();
    let worklet = graph.create_audio_worklet_node(InputChannelProbe {
        input_lengths: input_lengths.clone(),
    });
    graph.connect(inactive, &worklet).expect("source connects");
    graph
        .connect(&worklet, graph.destination())
        .expect("worklet connects");

    render_context_offline(&graph, 4, 1).expect("graph renders");

    assert_eq!(
        &input_lengths.lock().expect("input length mutex poisoned")[..],
        &[0],
        "connected but inactive AudioWorklet inputs should be empty channel arrays"
    );

    let input_lengths = Arc::new(Mutex::new(Vec::new()));
    let mut graph = AudioContext::new();
    let active_silent = graph.create_constant_source();
    active_silent.offset().set_value(0.0).unwrap();
    active_silent.try_start(0.0).unwrap();
    let worklet = graph.create_audio_worklet_node(InputChannelProbe {
        input_lengths: input_lengths.clone(),
    });
    graph
        .connect(active_silent, &worklet)
        .expect("source connects");
    graph
        .connect(&worklet, graph.destination())
        .expect("worklet connects");

    render_context_offline(&graph, 4, 1).expect("graph renders");

    assert_eq!(
        &input_lengths.lock().expect("input length mutex poisoned")[..],
        &[1],
        "actively processing silent sources should still provide their channel array"
    );
}

#[test]
fn audio_worklet_input_port_ignores_inactive_source_through_processing_node() {
    struct InputChannelProbe {
        input_lengths: Arc<Mutex<Vec<usize>>>,
    }

    impl melody_bay::AudioWorkletProcessor for InputChannelProbe {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            _outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            self.input_lengths
                .lock()
                .expect("input length mutex poisoned")
                .push(inputs.first().map_or(usize::MAX, Vec::len));
            true
        }
    }

    let input_lengths = Arc::new(Mutex::new(Vec::new()));
    let mut graph = AudioContext::new();
    let inactive = graph.create_constant_source();
    inactive.offset().set_value(0.0).unwrap();
    let gain = graph.create_gain();
    let worklet = graph.create_audio_worklet_node(InputChannelProbe {
        input_lengths: input_lengths.clone(),
    });
    graph.connect(inactive, &gain).expect("source connects");
    graph.connect(&gain, &worklet).expect("gain connects");
    graph
        .connect(&worklet, graph.destination())
        .expect("worklet connects");

    render_context_offline(&graph, 4, 1).expect("graph renders");

    assert_eq!(
        &input_lengths.lock().expect("input length mutex poisoned")[..],
        &[0],
        "inactive upstream sources should not create worklet input channels through processing nodes"
    );

    let input_lengths = Arc::new(Mutex::new(Vec::new()));
    let mut graph = AudioContext::new();
    let active_silent = graph.create_constant_source();
    active_silent.offset().set_value(0.0).unwrap();
    active_silent.try_start(0.0).unwrap();
    let gain = graph.create_gain();
    let worklet = graph.create_audio_worklet_node(InputChannelProbe {
        input_lengths: input_lengths.clone(),
    });
    graph
        .connect(active_silent, &gain)
        .expect("source connects");
    graph.connect(&gain, &worklet).expect("gain connects");
    graph
        .connect(&worklet, graph.destination())
        .expect("worklet connects");

    render_context_offline(&graph, 4, 1).expect("graph renders");

    assert_eq!(
        &input_lengths.lock().expect("input length mutex poisoned")[..],
        &[1],
        "active silent upstream sources should still create worklet input channels through processing nodes"
    );
}

#[test]
fn audio_worklet_routes_requested_output_indices() {
    struct MultiOutput;
    impl melody_bay::AudioWorkletProcessor for MultiOutput {
        fn process(
            &mut self,
            _inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            worklet_output_channel_mut(outputs, 0, 0)
                .expect("first output channel")
                .fill(0.25);
            worklet_output_channel_mut(outputs, 1, 0)
                .expect("second output channel")
                .fill(0.5);
            worklet_output_channel_mut(outputs, 2, 0)
                .expect("third output channel")
                .fill(0.75);
            true
        }
    }

    let mut graph = AudioContext::new();
    let worklet = graph
        .try_create_audio_worklet_node(
            MultiOutput,
            melody_bay::AudioWorkletNodeOptions {
                number_of_inputs: 0,
                number_of_outputs: 3,
                output_channel_count: Some(vec![1, 1, 1]),
                ..Default::default()
            },
        )
        .expect("worklet options are valid");
    let merger = graph.try_create_channel_merger(2).unwrap();
    graph
        .connect_with_indices(&worklet, 2, &merger, 0)
        .expect("third output connects to left");
    graph
        .connect_with_indices(&worklet, 0, &merger, 1)
        .expect("first output connects to right");
    graph
        .connect(&merger, graph.destination())
        .expect("merger connects");

    let buffer = render_context_offline(&graph, 4, 1).expect("graph renders");

    assert_eq!(buffer.channel_data(0), Some(&[0.75][..]));
    assert_eq!(buffer.channel_data(1), Some(&[0.25][..]));
}

#[test]
fn offline_audio_context_renders_graph_and_tracks_state() {
    let mut context = OfflineAudioContext::try_new(2, 3, 3_000).unwrap();
    let source = {
        let source = started_offline_constant_source(&mut context);
        source.offset().set_value(0.5).unwrap();
        source
    };
    context
        .connect(source, context.destination())
        .expect("source connects");

    assert_eq!(context.number_of_channels(), 2);
    assert_eq!(context.length(), 3);
    assert_eq!(context.sample_rate(), 3_000);
    assert_close64(context.current_time(), 0.0);
    assert_eq!(context.state(), OfflineAudioContextState::Suspended);

    let buffer = context.start_rendering().expect("graph renders");

    assert_eq!(context.state(), OfflineAudioContextState::Closed);
    assert_close64(context.current_time(), 3.0 / 3_000.0);
    assert_eq!(buffer.channel_data(0), Some(&[0.5, 0.5, 0.5][..]));
}

#[test]
fn offline_audio_context_supports_webaudio_state_transitions() {
    let mut context = OfflineAudioContext::try_new(2, 2, 3_000).unwrap();
    let source = {
        let source = started_offline_constant_source(&mut context);
        source.offset().set_value(0.25).unwrap();
        source
    };
    context
        .connect(source, context.destination())
        .expect("source connects");

    context.suspend(0.00025).expect("context suspends");
    assert_eq!(context.state(), OfflineAudioContextState::Suspended);
    assert_close64(context.current_time(), 0.0);
    assert_eq!(context.resume(), Err(GraphError::InvalidState));
    assert_eq!(context.state(), OfflineAudioContextState::Suspended);

    let rendered = context.start_rendering().expect("graph renders");

    assert_eq!(context.state(), OfflineAudioContextState::Closed);
    assert_close64(context.current_time(), 2.0 / 3_000.0);
    assert_eq!(rendered.channel_data(0), Some(&[0.25, 0.25][..]));
    assert!(context.start_rendering().is_err());
}

#[test]
fn source_nodes_reject_repeated_start_and_stop_scheduling() {
    let mut graph = AudioContext::new();
    let oscillator = graph.create_oscillator();
    oscillator.set_type(Waveform::Sine);
    let constant = graph.create_constant_source();
    let buffer = graph.create_buffer_source();
    buffer
        .try_set_buffer(audio_buffer_from_mono(4, 1, [1.0]))
        .unwrap();

    oscillator.try_start(0.0).expect("oscillator starts");
    constant.try_start(0.0).expect("constant starts");
    buffer
        .try_start_with_offset_and_duration(0.0, 0.0, 0.25)
        .expect("buffer starts");

    assert_eq!(
        oscillator.try_start(0.25),
        Err(melody_bay::GraphError::SourceAlreadyStarted)
    );
    assert_eq!(
        constant.try_start(0.25),
        Err(melody_bay::GraphError::SourceAlreadyStarted)
    );
    assert_eq!(
        buffer.try_start_with_offset(0.25, 0.0),
        Err(melody_bay::GraphError::SourceAlreadyStarted)
    );

    oscillator.try_stop(0.5).expect("oscillator stops");
    oscillator
        .try_stop(0.75)
        .expect("later stop replaces earlier stop time");
}

#[test]
fn source_stop_before_scheduled_start_renders_silence() {
    let mut graph = AudioContext::new();
    let oscillator = graph.create_oscillator();
    oscillator.try_start(0.5).unwrap();
    oscillator.try_stop(0.25).unwrap();

    let constant = graph.create_constant_source();
    constant.offset().set_value(1.0).unwrap();
    constant.try_start(0.5).unwrap();
    constant.try_stop(0.25).unwrap();

    let buffer = graph.create_buffer_source();
    buffer
        .try_set_buffer(audio_buffer_from_mono(4, 4, [1.0, 1.0, 1.0, 1.0]))
        .unwrap();
    buffer.try_start_with_offset(0.5, 0.0).unwrap();
    buffer.try_stop(0.25).unwrap();

    graph.connect(&oscillator, graph.destination()).unwrap();
    graph.connect(&constant, graph.destination()).unwrap();
    graph.connect(&buffer, graph.destination()).unwrap();

    let rendered = render_context_offline(&graph, 4, 4).expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[0.0, 0.0, 0.0, 0.0][..]));
}

#[test]
fn repeated_stop_before_end_replaces_previous_stop_time() {
    let mut graph = AudioContext::new();
    let source = started_constant_source(&mut graph);
    source.offset().set_value(1.0).unwrap();
    source.try_stop(0.25).unwrap();
    source.try_stop(0.75).unwrap();
    graph
        .connect(&source, graph.destination())
        .expect("source connects");

    let rendered = render_context_offline(&graph, 4, 4).expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[1.0, 1.0, 1.0, 0.0][..]));
}

#[test]
fn graph_renders_connected_oscillator_gain_destination() {
    let mut graph = AudioContext::new();
    let osc = graph.create_oscillator();
    osc.set_type(Waveform::Sine);
    osc.try_start(0.0).unwrap();
    osc.frequency().set_value(1.0).unwrap();
    let gain = graph.create_gain();
    gain.gain().set_value(0.5).unwrap();
    graph
        .connect(osc, &gain)
        .expect("oscillator connects to gain");
    graph
        .connect(&gain, graph.destination())
        .expect("gain connects to destination");

    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [kira::Frame::ZERO; 2];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 0.5);
}

#[test]
fn simple_oscillator_graph_sound_outputs_audible_signal() {
    let mut graph = AudioContext::new();
    let osc = graph.create_oscillator();
    osc.set_type(Waveform::Sine);
    osc.try_start(0.0).unwrap();
    osc.try_stop(1.5).unwrap();
    osc.frequency().set_value(440.0).unwrap();
    let gain = graph.create_gain();
    gain.gain().set_value(0.25).unwrap();
    graph.connect(osc, &gain).expect("oscillator connects");
    graph
        .connect(&gain, graph.destination())
        .expect("gain connects to destination");

    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(48_000)
        .into_sound()
        .expect("graph should build");
    assert!(!sound.finished());

    let info = MockInfoBuilder::new().build();
    let mut out = vec![Frame::ZERO; 4_800];
    sound.process(&mut out, 1.0 / 48_000.0, &info);

    assert!(rms(&out) > 0.1, "simple oscillator graph rendered silence");
}

#[test]
fn tone_graph_survives_kira_start_processing_before_first_audio_callback() {
    let mut graph = AudioContext::new();
    let osc = graph.create_oscillator();
    osc.set_type(Waveform::Sine);
    osc.try_start(0.0).unwrap();
    osc.try_stop(1.5).unwrap();
    osc.frequency().set_value(440.0).unwrap();
    let gain = graph.create_gain();
    gain.gain().set_value(0.25).unwrap();
    graph.connect(osc, &gain).expect("oscillator connects");
    graph
        .connect(&gain, graph.destination())
        .expect("gain connects to destination");

    let mut manager = AudioManager::<MockBackend>::new(AudioManagerSettings {
        backend_settings: MockBackendSettings {
            sample_rate: 48_000,
        },
        ..Default::default()
    })
    .expect("mock backend starts");

    manager
        .play(graph.sound_data())
        .expect("graph sound starts");
    assert_eq!(manager.main_track().num_sounds(), 1);

    manager.backend_mut().on_start_processing();

    assert_eq!(manager.main_track().num_sounds(), 1);
}

#[test]
fn tone_graph_outputs_audio_through_kira_manager_renderer() {
    let mut graph = AudioContext::new();
    let osc = graph.create_oscillator();
    osc.set_type(Waveform::Sine);
    osc.try_start(0.0).unwrap();
    osc.try_stop(1.5).unwrap();
    osc.frequency().set_value(440.0).unwrap();
    let gain = graph.create_gain();
    gain.gain().set_value(0.25).unwrap();
    graph.connect(osc, &gain).expect("oscillator connects");
    graph
        .connect(&gain, graph.destination())
        .expect("gain connects to destination");

    let captured = Arc::new(Mutex::new(Vec::new()));
    let mut manager = AudioManager::<CapturingBackend>::new(AudioManagerSettings {
        backend_settings: CapturingBackendSettings {
            sample_rate: 48_000,
            captured: captured.clone(),
        },
        ..Default::default()
    })
    .expect("capture backend starts");

    manager
        .play(graph.sound_data())
        .expect("graph sound starts");
    manager.backend_mut().process(4_800);

    let captured = captured.lock().expect("captured mutex poisoned");
    let frames = captured
        .chunks_exact(2)
        .map(|channels| Frame::new(channels[0], channels[1]))
        .collect::<Vec<_>>();
    assert!(
        rms(&frames) > 0.1,
        "simple oscillator graph rendered silence through Kira manager"
    );
}

#[test]
fn graph_sound_handle_stops_live_kira_sound() {
    let mut graph = AudioContext::new();
    let osc = graph.create_oscillator();
    osc.set_type(Waveform::Sine);
    osc.try_start(0.0).unwrap();
    osc.frequency().set_value(440.0).unwrap();
    graph
        .connect(osc, graph.destination())
        .expect("oscillator connects");

    let captured = Arc::new(Mutex::new(Vec::new()));
    let mut manager = AudioManager::<CapturingBackend>::new(AudioManagerSettings {
        backend_settings: CapturingBackendSettings {
            sample_rate: 48_000,
            captured: captured.clone(),
        },
        ..Default::default()
    })
    .expect("capture backend starts");

    let handle = manager
        .play(graph.sound_data())
        .expect("graph sound starts");
    manager.backend_mut().process(2_400);
    assert_eq!(manager.main_track().num_sounds(), 1);

    handle.stop();
    manager.backend_mut().process(2_400);

    assert!(handle.stopped());
    assert_eq!(manager.main_track().num_sounds(), 0);
}

#[test]
fn tone_graph_outputs_audio_through_kira_after_offline_preflight_render() {
    let mut graph = AudioContext::new();
    let osc = graph.create_oscillator();
    osc.set_type(Waveform::Sine);
    osc.try_start(0.0).unwrap();
    osc.try_stop(3.0).unwrap();
    osc.frequency().set_value(440.0).unwrap();
    let gain = graph.create_gain();
    gain.gain().set_value(0.25).unwrap();
    graph.connect(osc, &gain).expect("oscillator connects");
    graph
        .connect(&gain, graph.destination())
        .expect("gain connects to destination");

    let preflight =
        render_context_offline(&graph, 48_000, 4_800).expect("preflight render succeeds");
    let preflight_frames = preflight
        .channel_data(0)
        .expect("preflight has left channel")
        .iter()
        .copied()
        .map(Frame::from_mono)
        .collect::<Vec<_>>();
    assert!(rms(&preflight_frames) > 0.1, "preflight rendered silence");

    let captured = Arc::new(Mutex::new(Vec::new()));
    let mut manager = AudioManager::<CapturingBackend>::new(AudioManagerSettings {
        backend_settings: CapturingBackendSettings {
            sample_rate: 48_000,
            captured: captured.clone(),
        },
        ..Default::default()
    })
    .expect("capture backend starts");

    manager
        .play(graph.sound_data())
        .expect("graph sound starts");
    manager.backend_mut().process(4_800);

    let captured = captured.lock().expect("captured mutex poisoned");
    let frames = captured
        .chunks_exact(2)
        .map(|channels| Frame::new(channels[0], channels[1]))
        .collect::<Vec<_>>();
    assert!(
        rms(&frames) > 0.1,
        "simple oscillator graph rendered silence through Kira manager after preflight"
    );
}

#[test]
fn live_graph_outputs_audio_after_preflight() {
    let mut graph = AudioContext::new();
    let osc = graph.create_oscillator();
    osc.set_type(Waveform::Sine);
    osc.try_start(0.0).unwrap();
    osc.try_stop(10.0).unwrap();
    osc.frequency().set_value(440.0).unwrap();
    let gain = graph.create_gain();
    gain.gain().set_value(1.0).unwrap();
    graph.connect(osc, &gain).expect("oscillator connects");
    graph
        .connect(&gain, graph.destination())
        .expect("gain connects to destination");

    let sample_rate = 48_000;
    let preflight = render_context_offline(&graph, sample_rate, sample_rate as usize / 10)
        .expect("preflight render succeeds");
    let left = preflight
        .channel_data(0)
        .expect("preflight has left channel");
    let preflight_rms =
        (left.iter().map(|sample| sample * sample).sum::<f32>() / left.len().max(1) as f32).sqrt();
    assert!(preflight_rms > 0.1, "tone preflight rendered silence");

    let captured = Arc::new(Mutex::new(Vec::new()));
    let mut manager = AudioManager::<CapturingBackend>::new(AudioManagerSettings {
        backend_settings: CapturingBackendSettings {
            sample_rate,
            captured: captured.clone(),
        },
        ..Default::default()
    })
    .expect("capture backend starts");

    let handle = manager
        .play(graph.sound_data().sample_rate(sample_rate))
        .expect("graph sound starts");
    manager.backend_mut().process(sample_rate as usize);

    let captured = captured.lock().expect("captured mutex poisoned");
    let frames = captured
        .chunks_exact(2)
        .map(|channels| Frame::new(channels[0], channels[1]))
        .collect::<Vec<_>>();
    assert!(
        rms(&frames) > 0.1,
        "live graph rendered silence after preflight"
    );
    assert!(!handle.stopped());
}

#[test]
fn live_graph_outputs_audio_with_explicit_sample_rate_and_full_preflight() {
    let sample_rate = 48_000;
    let mut graph = AudioContext::try_new_with_sample_rate(sample_rate).unwrap();
    let osc = graph.create_oscillator();
    osc.set_type(Waveform::Sine);
    osc.try_start(0.0).unwrap();
    osc.try_stop(10.0).unwrap();
    osc.frequency().set_value(440.0).unwrap();
    let gain = graph.create_gain();
    gain.gain().set_value(1.0).unwrap();
    graph.connect(osc, &gain).expect("oscillator connects");
    graph
        .connect(&gain, graph.destination())
        .expect("gain connects to destination");

    let preflight = render_context_offline(&graph, sample_rate, sample_rate as usize * 10)
        .expect("preflight render succeeds");
    let left = preflight
        .channel_data(0)
        .expect("preflight has left channel");
    let preflight_rms =
        (left.iter().map(|sample| sample * sample).sum::<f32>() / left.len().max(1) as f32).sqrt();
    assert!(preflight_rms > 0.1, "tone preflight rendered silence");

    let captured = Arc::new(Mutex::new(Vec::new()));
    let mut manager = AudioManager::<CapturingBackend>::new(AudioManagerSettings {
        backend_settings: CapturingBackendSettings {
            sample_rate,
            captured: captured.clone(),
        },
        ..Default::default()
    })
    .expect("capture backend starts");

    let handle = manager
        .play(graph.sound_data().sample_rate(sample_rate))
        .expect("graph sound starts");
    manager.backend_mut().process(sample_rate as usize);

    let captured = captured.lock().expect("captured mutex poisoned");
    let frames = captured
        .chunks_exact(2)
        .map(|channels| Frame::new(channels[0], channels[1]))
        .collect::<Vec<_>>();
    assert!(
        rms(&frames) > 0.1,
        "live graph rendered silence after full preflight"
    );
    assert!(!handle.stopped());
}

#[test]
fn rendered_tone_buffer_outputs_audio_through_kira_static_sound() {
    let mut graph = AudioContext::new();
    let osc = graph.create_oscillator();
    osc.set_type(Waveform::Sine);
    osc.try_start(0.0).unwrap();
    osc.try_stop(1.0).unwrap();
    osc.frequency().set_value(440.0).unwrap();
    let gain = graph.create_gain();
    gain.gain().set_value(1.0).unwrap();
    graph.connect(osc, &gain).expect("oscillator connects");
    graph
        .connect(&gain, graph.destination())
        .expect("gain connects to destination");

    let rendered = render_context_offline(&graph, 48_000, 48_000).expect("tone render succeeds");
    let left = rendered.channel_data(0).expect("left channel exists");
    let right = rendered.channel_data(1).expect("right channel exists");
    let frames = left
        .iter()
        .copied()
        .zip(right.iter().copied())
        .map(|(left, right)| Frame::new(left, right))
        .collect::<Vec<_>>();

    let captured = Arc::new(Mutex::new(Vec::new()));
    let mut manager = AudioManager::<CapturingBackend>::new(AudioManagerSettings {
        backend_settings: CapturingBackendSettings {
            sample_rate: 48_000,
            captured: captured.clone(),
        },
        ..Default::default()
    })
    .expect("capture backend starts");

    manager
        .play(StaticSoundData {
            sample_rate: 48_000,
            frames: frames.into(),
            settings: StaticSoundSettings::new(),
            slice: None,
        })
        .expect("static tone starts");
    manager.backend_mut().process(4_800);

    let captured = captured.lock().expect("captured mutex poisoned");
    let frames = captured
        .chunks_exact(2)
        .map(|channels| Frame::new(channels[0], channels[1]))
        .collect::<Vec<_>>();
    assert!(
        rms(&frames) > 0.1,
        "rendered buffer played silence through Kira static sound"
    );
}

#[test]
fn finite_graph_still_plays_live_after_complete_offline_preflight() {
    let mut offline = OfflineAudioContext::try_new(2, 3_001, 3_000).unwrap();
    let source = started_offline_constant_source(&mut offline);
    source.try_stop(1.0).unwrap();
    source.offset().set_value(0.5).unwrap();
    offline
        .connect(&source, offline.destination())
        .expect("source connects");

    let rendered = offline.start_rendering().expect("offline render succeeds");
    let rendered = rendered.channel_data(0).expect("left channel exists");
    assert_eq!(rendered.first().copied(), Some(0.5));
    assert_eq!(rendered.get(2_999).copied(), Some(0.5));
    assert_eq!(rendered.get(3_000).copied(), Some(0.0));

    let mut graph = AudioContext::new();
    let source = started_constant_source(&mut graph);
    source.try_stop(1.0).unwrap();
    source.offset().set_value(0.5).unwrap();
    graph
        .connect(&source, graph.destination())
        .expect("source connects");

    let captured = Arc::new(Mutex::new(Vec::new()));
    let mut manager = AudioManager::<CapturingBackend>::new(AudioManagerSettings {
        backend_settings: CapturingBackendSettings {
            sample_rate: 4,
            captured: captured.clone(),
        },
        ..Default::default()
    })
    .expect("capture backend starts");

    manager
        .play(graph.sound_data().sample_rate(4))
        .expect("graph sound starts");
    manager.backend_mut().process(2);

    let captured = captured.lock().expect("captured mutex poisoned");
    assert_eq!(&captured[..], &[0.5, 0.5, 0.5, 0.5]);
}

#[test]
fn graph_sound_finishes_after_finite_sources_end() {
    let mut graph = AudioContext::new();
    let source = started_constant_source(&mut graph);
    source.try_stop(0.25).unwrap();
    source.offset().set_value(1.0).unwrap();
    graph
        .connect(&source, graph.destination())
        .expect("source connects");

    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 3];

    assert!(!sound.finished());
    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 1.0);
    assert_close(out[1].left, 0.0);
    assert!(sound.finished());
}

#[test]
fn graph_sound_keeps_convolver_tail_alive_after_source_end() {
    let sample_rate = 3_000;
    let mut graph = AudioContext::try_new_with_sample_rate(sample_rate).unwrap();
    let source = started_constant_source(&mut graph);
    source.offset().set_value(1.0).unwrap();
    source.try_stop(1.0 / sample_rate as f64).unwrap();
    let convolver = graph.create_convolver();
    convolver.set_normalize(false);
    convolver
        .try_buffer(audio_buffer_from_mono(sample_rate, 2, [0.5, 0.25]))
        .unwrap();
    graph.connect(source, &convolver).expect("source connects");
    graph
        .connect(&convolver, graph.destination())
        .expect("convolver connects");

    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(sample_rate)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 1];

    sound.process(&mut out, 1.0 / sample_rate as f64, &info);
    assert_close(out[0].left, 0.5);
    assert!(
        !sound.finished(),
        "convolver tail should keep finite graph alive after dry source ends"
    );

    sound.process(&mut out, 1.0 / sample_rate as f64, &info);
    assert_close(out[0].left, 0.25);
    assert!(sound.finished());
}

#[test]
fn graph_sound_keeps_delay_tail_alive_after_source_end() {
    let sample_rate = 3_000;
    let mut graph = AudioContext::try_new_with_sample_rate(sample_rate).unwrap();
    let source = started_constant_source(&mut graph);
    source.offset().set_value(1.0).unwrap();
    source.try_stop(1.0 / sample_rate as f64).unwrap();
    let delay = graph.try_create_delay(1.0).unwrap();
    delay
        .delay_time()
        .set_value(1.0 / sample_rate as f32)
        .unwrap();
    graph.connect(source, &delay).expect("source connects");
    graph
        .connect(&delay, graph.destination())
        .expect("delay connects");

    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(sample_rate)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 1];

    sound.process(&mut out, 1.0 / sample_rate as f64, &info);
    assert_close(out[0].left, 0.0);
    assert!(
        !sound.finished(),
        "delay tail should keep finite graph alive after dry source ends"
    );

    sound.process(&mut out, 1.0 / sample_rate as f64, &info);
    assert_close(out[0].left, 1.0);
    assert!(sound.finished());
}

#[test]
fn graph_sound_keeps_dynamics_compressor_lookahead_tail_alive_after_source_end() {
    let sample_rate = 3_000;
    let mut graph = AudioContext::try_new_with_sample_rate(sample_rate).unwrap();
    let source = started_constant_source(&mut graph);
    source.offset().set_value(1.0).unwrap();
    source.try_stop(1.0 / sample_rate as f64).unwrap();
    let compressor = graph.create_dynamics_compressor();
    compressor.threshold().set_value(0.0).unwrap();
    compressor.ratio().set_value(1.0).unwrap();
    compressor.knee().set_value(0.0).unwrap();
    compressor.attack().set_value(0.0).unwrap();
    compressor.release().set_value(0.0).unwrap();
    graph
        .connect(&source, &compressor)
        .expect("source connects");
    graph
        .connect(&compressor, graph.destination())
        .expect("compressor connects");

    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(sample_rate)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 18];

    sound.process(&mut out, 1.0 / sample_rate as f64, &info);
    assert!(out.iter().all(|frame| frame.left.abs() <= f32::EPSILON));
    assert!(
        !sound.finished(),
        "compressor lookahead should keep finite graph alive after dry source ends"
    );

    let mut tail = [Frame::ZERO; 1];
    sound.process(&mut tail, 1.0 / sample_rate as f64, &info);
    assert_close(tail[0].left, 1.0);
    assert!(sound.finished());
}

#[test]
fn graph_can_disconnect_audio_connections() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    graph
        .connect(&source, graph.destination())
        .expect("source connects");
    graph
        .disconnect(&source, graph.destination())
        .expect("source disconnects");

    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[0].right, 0.0);
}

#[test]
fn graph_can_disconnect_audio_param_connections() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let modulation = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let gain = graph.create_gain();
    gain.gain().set_value(0.0).unwrap();
    let gain_param = gain.param("gain").expect("gain param exists");
    graph
        .connect_param(&modulation, gain_param.clone())
        .expect("modulator connects");
    graph
        .disconnect_param(&modulation, gain_param)
        .expect("modulator disconnects");
    graph.connect(source, &gain).expect("source connects");
    graph
        .connect(&gain, graph.destination())
        .expect("gain connects");

    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[0].right, 0.0);
}

#[test]
fn graph_can_disconnect_all_audio_outputs_from_a_node() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let gain_a = graph.create_gain();
    let gain_b = graph.create_gain();
    graph.connect(&source, &gain_a).expect("source connects");
    graph.connect(&source, &gain_b).expect("source connects");
    graph
        .connect(&gain_a, graph.destination())
        .expect("gain connects");
    graph
        .connect(&gain_b, graph.destination())
        .expect("gain connects");
    graph
        .disconnect_outputs(&source)
        .expect("outputs disconnect");

    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
}

#[test]
fn graph_can_disconnect_all_param_outputs_from_a_node() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let modulation = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let gain_a = graph.create_gain();
    gain_a.gain().set_value(0.0).unwrap();
    let gain_b = graph.create_gain();
    gain_b.gain().set_value(0.0).unwrap();
    graph
        .connect_param(
            &modulation,
            gain_a.param("gain").expect("gain param exists"),
        )
        .expect("modulator connects");
    graph
        .connect_param(
            &modulation,
            gain_b.param("gain").expect("gain param exists"),
        )
        .expect("modulator connects");
    graph
        .disconnect_param_outputs(&modulation)
        .expect("param outputs disconnect");
    graph.connect(&source, &gain_a).expect("source connects");
    graph.connect(&source, &gain_b).expect("source connects");
    graph
        .connect(&gain_a, graph.destination())
        .expect("gain connects");
    graph
        .connect(&gain_b, graph.destination())
        .expect("gain connects");

    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
}

#[test]
fn graph_audio_connections_are_idempotent() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    graph
        .connect(&source, graph.destination())
        .expect("source connects");
    graph
        .connect(&source, graph.destination())
        .expect("duplicate source connects");

    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 1.0);
}

#[test]
fn graph_param_connections_are_idempotent() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let modulation = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(0.5).unwrap();
        source
    };
    let gain = graph.create_gain();
    gain.gain().set_value(0.0).unwrap();
    let gain_param = gain.param("gain").expect("gain param exists");
    graph
        .connect_param(&modulation, gain_param.clone())
        .expect("modulator connects");
    graph
        .connect_param(&modulation, gain_param)
        .expect("duplicate modulator connects");
    graph.connect(&source, &gain).expect("source connects");
    graph
        .connect(&gain, graph.destination())
        .expect("gain connects");

    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.5);
}

#[test]
fn audio_buffer_source_preserves_stereo_channels() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_stereo(4, 1, [0.25], [0.75]),
    );
    graph
        .connect(source, graph.destination())
        .expect("buffer source connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [kira::Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.25);
    assert_close(out[0].right, 0.75);
}

#[test]
fn constant_source_start_and_stop_gate_output() {
    let mut graph = AudioContext::new();
    let source = graph.create_constant_source();
    source.try_start(0.25).unwrap();
    source.try_stop(0.75).unwrap();
    source.offset().set_value(1.0).unwrap();
    graph
        .connect(&source, graph.destination())
        .expect("constant source connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 4];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 1.0);
    assert_close(out[2].left, 1.0);
    assert_close(out[3].left, 0.0);
}

#[test]
fn audio_buffer_source_start_time_offsets_playback_clock() {
    let mut graph = AudioContext::new();
    let source = graph.create_buffer_source();
    source
        .try_set_buffer(audio_buffer_from_mono(4, 2, [0.25, 0.5]))
        .unwrap();
    source.try_start(0.25).unwrap();
    source.try_stop(0.75).unwrap();
    graph
        .connect(&source, graph.destination())
        .expect("buffer source connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 4];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 0.25);
    assert_close(out[2].left, 0.5);
    assert_close(out[3].left, 0.0);
}

#[test]
fn audio_buffer_source_start_offset_selects_buffer_position() {
    let mut graph = AudioContext::new();
    let source = graph.create_buffer_source();
    source
        .try_set_buffer(audio_buffer_from_mono(4, 4, [0.0, 0.25, 0.5, 0.75]))
        .unwrap();
    source.try_start_with_offset(0.25, 0.5).unwrap();
    graph
        .connect(&source, graph.destination())
        .expect("buffer source connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 3];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 0.5);
    assert_close(out[2].left, 0.75);
}

#[test]
fn audio_buffer_source_start_duration_stops_after_play_window() {
    let mut graph = AudioContext::new();
    let source = graph.create_buffer_source();
    source
        .try_set_buffer(audio_buffer_from_mono(4, 4, [0.0, 0.25, 0.5, 0.75]))
        .unwrap();
    source
        .try_start_with_offset_and_duration(0.25, 0.25, 0.5)
        .unwrap();
    graph
        .connect(&source, graph.destination())
        .expect("buffer source connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 4];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 0.25);
    assert_close(out[2].left, 0.5);
    assert_close(out[3].left, 0.0);
}

#[test]
fn audio_buffer_source_duration_uses_buffer_time_not_wall_time() {
    let mut graph = AudioContext::new();
    let source = graph.create_buffer_source();
    source
        .try_set_buffer(audio_buffer_from_mono(4, 4, [0.0, 0.25, 0.5, 0.75]))
        .unwrap();
    source
        .try_start_with_offset_and_duration(0.0, 0.0, 0.5)
        .unwrap();
    source.playback_rate().set_value(0.5).unwrap();
    graph
        .connect(&source, graph.destination())
        .expect("buffer source connects");
    let rendered = render_context_offline(&graph, 4, 4).expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[0.0, 0.0, 0.25, 0.25][..]));
}

#[test]
fn audio_buffer_source_negative_playback_duration_uses_buffer_time() {
    let mut graph = AudioContext::new();
    let source = graph.create_buffer_source();
    source
        .try_set_buffer(audio_buffer_from_mono(4, 4, [0.0, 0.25, 0.5, 0.75]))
        .unwrap();
    source
        .try_start_with_offset_and_duration(0.0, 0.75, 0.5)
        .unwrap();
    source.playback_rate().set_value(-1.0).unwrap();
    graph
        .connect(&source, graph.destination())
        .expect("buffer source connects");

    let rendered = render_context_offline(&graph, 4, 4).expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[0.75, 0.5, 0.0, 0.0][..]));
}

#[test]
fn audio_buffer_source_applies_playback_rate() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [0.0, 0.25, 0.5, 0.75]),
    );
    source.playback_rate().set_value(2.0).unwrap();
    graph
        .connect(source, graph.destination())
        .expect("buffer source connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 2];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 0.5);
}

#[test]
fn audio_buffer_source_negative_playback_rate_renders_backwards() {
    let mut graph = AudioContext::new();
    let source = graph.create_buffer_source();
    source
        .try_set_buffer(audio_buffer_from_mono(4, 4, [0.0, 0.25, 0.5, 0.75]))
        .unwrap();
    source.try_start_with_offset(0.0, 0.75).unwrap();
    source.playback_rate().set_value(-1.0).unwrap();
    graph
        .connect(&source, graph.destination())
        .expect("buffer source connects");

    let rendered = render_context_offline(&graph, 4, 4).expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[0.75, 0.5, 0.25, 0.0][..]));
}

#[test]
fn audio_buffer_source_integrates_playback_rate_changes_over_time() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(256, 512, (0..512).map(|index| index as f32)),
    );
    source.playback_rate().set_value_at_time(1.0, 0.0).unwrap();
    source.playback_rate().set_value_at_time(2.0, 0.5).unwrap();
    graph
        .connect(source, graph.destination())
        .expect("buffer source connects");

    let rendered = render_context_offline(&graph, 256, 193).expect("graph renders");

    assert_eq!(rendered.channel_data(0).unwrap()[192], 256.0);
}

#[test]
fn audio_buffer_source_natural_end_uses_integrated_playback_position() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [0.0, 0.25, 0.5, 0.75]),
    );
    source.playback_rate().set_value(2.0).unwrap();
    graph
        .connect(&source, graph.destination())
        .expect("buffer source connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 2];

    sound.process(&mut out, 0.25, &info);

    assert_eq!(out, [Frame::new(0.0, 0.0), Frame::new(0.5, 0.5)]);
    assert!(sound.finished());
}

#[test]
fn audio_buffer_source_slow_playback_does_not_end_at_wall_clock_duration() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [0.0, 0.25, 0.5, 0.75]),
    );
    source.playback_rate().set_value(0.5).unwrap();
    graph
        .connect(&source, graph.destination())
        .expect("buffer source connects");
    let rendered = render_context_offline(&graph, 4, 6).expect("graph renders");

    assert_eq!(
        rendered.channel_data(0),
        Some(&[0.0, 0.0, 0.25, 0.25, 0.5, 0.5][..])
    );
}

#[test]
fn audio_buffer_source_slow_playback_live_lifetime_uses_integrated_position() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [0.0, 0.25, 0.5, 0.75]),
    );
    source.playback_rate().set_value(0.5).unwrap();
    graph
        .connect(&source, graph.destination())
        .expect("buffer source connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut first_second = [Frame::ZERO; 4];
    let mut second_second = [Frame::ZERO; 4];

    sound.process(&mut first_second, 0.25, &info);
    assert!(
        !sound.finished(),
        "slow playback should still be alive after one wall-clock buffer duration"
    );

    sound.process(&mut second_second, 0.25, &info);
    assert!(sound.finished());
}

#[test]
fn audio_buffer_source_applies_detune_in_cents() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [0.0, 0.25, 0.5, 0.75]),
    );
    source.detune().set_value(1200.0).unwrap();
    graph
        .connect(source, graph.destination())
        .expect("buffer source connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 2];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 0.5);
}

#[test]
fn audio_buffer_source_detune_accepts_audio_rate_modulation() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [0.0, 0.25, 0.5, 0.75]),
    );
    let detune = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1200.0).unwrap();
        source
    };
    graph
        .connect_param(detune, source.detune())
        .expect("detune modulator connects");
    graph
        .connect(&source, graph.destination())
        .expect("buffer source connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 2];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 0.5);
}

#[test]
fn audio_buffer_source_playback_rate_param_input_is_sampled_at_k_rate() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(3_000, 256, (0..256).map(|index| index as f32)),
    );
    let modulator = graph.create_constant_source();
    modulator.try_start(0.0).unwrap();
    modulator.offset().set_value_at_time(0.0, 0.0).unwrap();
    modulator.offset().set_value_at_time(1.0, 0.5).unwrap();

    graph
        .connect_param(&modulator, source.playback_rate())
        .expect("modulator connects to playback rate");
    graph
        .connect(&source, graph.destination())
        .expect("buffer source connects");

    let rendered = render_context_offline(&graph, 3_000, 96).expect("graph renders");

    assert_eq!(rendered.channel_data(0).unwrap()[95], 95.0);
}

#[test]
fn k_rate_param_input_from_upstream_nodes_is_sampled_at_quantum_start() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(3_000, 256, (0..256).map(|index| index as f32)),
    );
    let modulator = graph.create_constant_source();
    modulator.try_start(0.0).unwrap();
    modulator.offset().set_value_at_time(0.0, 0.0).unwrap();
    modulator.offset().set_value_at_time(1.0, 0.5).unwrap();
    let pass_through = graph.create_gain();

    graph
        .connect(&modulator, &pass_through)
        .expect("modulator connects to gain");
    graph
        .connect_param(&pass_through, source.playback_rate())
        .expect("gain connects to playback rate");
    graph
        .connect(&source, graph.destination())
        .expect("buffer source connects");

    let rendered = render_context_offline(&graph, 3_000, 96).expect("graph renders");

    assert_eq!(rendered.channel_data(0).unwrap()[95], 95.0);
}

#[test]
fn audio_buffer_source_rejects_second_non_null_buffer_assignment() {
    let mut graph = AudioContext::new();
    let source = graph.create_buffer_source();
    source
        .try_set_buffer(audio_buffer_from_mono(4, 1, [0.625]))
        .unwrap();
    assert_eq!(
        source.try_set_buffer(audio_buffer_from_mono(4, 1, [1.0])),
        Err(GraphError::InvalidState)
    );
    source.try_start(0.0).unwrap();
    graph
        .connect(&source, graph.destination())
        .expect("buffer source connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.625);
}

#[test]
fn audio_buffer_source_acquires_buffer_contents_when_started() {
    let mut graph = AudioContext::new();
    let source = graph.create_buffer_source();
    source
        .try_set_buffer(audio_buffer_from_mono(4, 2, [0.25, 0.5]))
        .unwrap();
    source.try_start(0.0).unwrap();
    source.clear_buffer();
    graph
        .connect(&source, graph.destination())
        .expect("buffer source connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 2];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.25);
    assert_close(out[1].left, 0.5);
}

#[test]
fn audio_buffer_source_acquires_first_buffer_assigned_after_start() {
    let mut graph = AudioContext::new();
    let source = graph.create_buffer_source();
    source.try_start(0.0).unwrap();
    source
        .try_set_buffer(audio_buffer_from_mono(4, 2, [0.75, 1.0]))
        .unwrap();
    graph
        .connect(&source, graph.destination())
        .expect("buffer source connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 2];

    sound.process(&mut out, 0.25, &info);

    assert_eq!(out, [Frame::new(0.75, 0.75), Frame::new(1.0, 1.0)]);
}

#[test]
fn audio_buffer_source_lifetime_uses_buffer_assigned_after_start() {
    let mut graph = AudioContext::new();
    let source = graph.create_buffer_source();
    source.try_start(0.0).unwrap();
    source.try_stop(1.0).unwrap();
    source
        .try_set_buffer(audio_buffer_from_mono(4, 1, [1.0]))
        .unwrap();
    graph
        .connect(&source, graph.destination())
        .expect("buffer source connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 2];

    sound.process(&mut out, 0.25, &info);

    assert_eq!(out, [Frame::new(1.0, 1.0), Frame::ZERO]);
    assert!(sound.finished(), "acquired buffer should naturally end");
}

#[test]
fn audio_buffer_source_loops_configured_range() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [0.0, 0.25, 0.5, 0.75]),
    );
    source.try_loop_range(0.25, 0.75).unwrap();
    source.set_looping(true);
    graph
        .connect(source, graph.destination())
        .expect("buffer source connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 5];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 0.25);
    assert_close(out[2].left, 0.5);
    assert_close(out[3].left, 0.25);
    assert_close(out[4].left, 0.5);
}

#[test]
fn audio_buffer_source_interpolates_across_loop_boundary() {
    let mut graph = AudioContext::new();
    let sample_rate = 3_000;
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(sample_rate, 4, [0.0, 1.0, 0.0, 0.0]),
    );
    source
        .try_loop_range(1.0 / f64::from(sample_rate), 3.0 / f64::from(sample_rate))
        .unwrap();
    source.set_looping(true);
    source.playback_rate().set_value(2.5).unwrap();
    graph
        .connect(source, graph.destination())
        .expect("buffer source connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(sample_rate)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 2];

    sound.process(&mut out, 1.0 / f64::from(sample_rate), &info);

    assert_close(out[1].left, 0.5);
}

#[test]
fn audio_buffer_source_averages_fast_sample_spans_to_reduce_aliasing() {
    let mut graph = AudioContext::new();
    let sample_rate = 3_000;
    let samples = (0..128)
        .map(|index| if index % 2 == 0 { 1.0 } else { -1.0 })
        .collect::<Vec<_>>();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(sample_rate, 128, samples),
    );
    source.playback_rate().set_value(8.0).unwrap();
    graph
        .connect(source, graph.destination())
        .expect("buffer source connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(sample_rate)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 8];

    sound.process(&mut out, 1.0 / f64::from(sample_rate), &info);

    let peak = out
        .iter()
        .map(|frame| frame.left.abs())
        .fold(0.0f32, f32::max);
    assert!(
        peak < 0.2,
        "expected anti-aliased fast sample replay, got peak {peak}"
    );
}

#[test]
fn audio_buffer_source_looping_still_obeys_start_duration() {
    let mut graph = AudioContext::new();
    let source = graph.create_buffer_source();
    source
        .try_set_buffer(audio_buffer_from_mono(4, 4, [0.0, 0.25, 0.5, 0.75]))
        .unwrap();
    source.try_loop_range(0.25, 0.75).unwrap();
    source.set_looping(true);
    source
        .try_start_with_offset_and_duration(0.0, 0.0, 0.75)
        .unwrap();
    graph
        .connect(&source, graph.destination())
        .expect("buffer source connects");

    let rendered = render_context_offline(&graph, 4, 5).expect("graph renders");

    assert_eq!(
        rendered.channel_data(0),
        Some(&[0.0, 0.25, 0.5, 0.0, 0.0][..])
    );
    assert!(source.ended());
}

#[test]
fn audio_buffer_source_loop_end_zero_uses_buffer_duration() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [0.0, 0.25, 0.5, 0.75]),
    );
    source.try_loop_start(0.25).unwrap();
    source.set_looping(true);
    graph
        .connect(source, graph.destination())
        .expect("buffer source connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 6];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 0.25);
    assert_close(out[2].left, 0.5);
    assert_close(out[3].left, 0.75);
    assert_close(out[4].left, 0.25);
    assert_close(out[5].left, 0.5);
}

#[test]
fn audio_buffer_source_negative_loop_start_effectively_clamps_to_buffer_start() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [0.0, 0.25, 0.5, 0.75]),
    );
    source.try_loop_start(-0.5).unwrap();
    source.try_loop_end(0.5).unwrap();
    source.set_looping(true);
    graph
        .connect(source, graph.destination())
        .expect("buffer source connects");
    let rendered = render_context_offline(&graph, 4, 5).expect("graph renders");

    assert_eq!(
        rendered.channel_data(0),
        Some(&[0.0, 0.25, 0.0, 0.25, 0.0][..])
    );
}

#[test]
fn audio_buffer_source_negative_playback_rate_wraps_within_loop_range() {
    let mut graph = AudioContext::new();
    let source = graph.create_buffer_source();
    source
        .try_set_buffer(audio_buffer_from_mono(4, 4, [0.0, 0.25, 0.5, 0.75]))
        .unwrap();
    source.try_start_with_offset(0.0, 0.5).unwrap();
    source.try_loop_range(0.25, 0.75).unwrap();
    source.set_looping(true);
    source.playback_rate().set_value(-1.0).unwrap();
    graph
        .connect(&source, graph.destination())
        .expect("buffer source connects");

    let rendered = render_context_offline(&graph, 4, 4).expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[0.5, 0.25, 0.5, 0.25][..]));
}

#[test]
fn audio_buffer_source_can_disable_configured_looping() {
    let mut graph = AudioContext::new();
    let source =
        started_buffer_source_with_buffer(&mut graph, audio_buffer_from_mono(4, 2, [0.0, 0.25]));
    source.try_loop_range(0.0, 0.5).unwrap();
    source.set_looping(false);
    graph
        .connect(source, graph.destination())
        .expect("buffer source connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 4];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 0.25);
    assert_close(out[2].left, 0.0);
    assert_close(out[3].left, 0.0);
}

#[test]
fn audio_buffer_source_can_loop_the_full_buffer() {
    let mut graph = AudioContext::new();
    let source =
        started_buffer_source_with_buffer(&mut graph, audio_buffer_from_mono(4, 2, [0.0, 0.25]));
    source.set_looping(true);
    graph
        .connect(source, graph.destination())
        .expect("buffer source connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 4];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 0.25);
    assert_close(out[2].left, 0.0);
    assert_close(out[3].left, 0.25);
}

#[test]
fn oscillator_periodic_wave_uses_custom_harmonics() {
    let mut graph = AudioContext::new();
    let osc = graph.create_oscillator();
    osc.set_type(Waveform::Sine);
    osc.set_periodic_wave(
        PeriodicWave::try_new_with_options(
            [0.0, 0.0],
            [0.0, 0.5],
            melody_bay::PeriodicWaveOptions {
                disable_normalization: true,
            },
        )
        .unwrap(),
    );
    osc.try_start(0.0).unwrap();
    osc.frequency().set_value(1.0).unwrap();
    graph
        .connect(osc, graph.destination())
        .expect("oscillator connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [kira::Frame::ZERO; 2];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 0.5);
}

#[test]
fn oscillator_waveform_can_be_changed_after_creation() {
    let mut graph = AudioContext::new();
    let osc = graph.create_oscillator();
    osc.set_type(Waveform::Sine);
    osc.set_type(Waveform::Square);
    osc.try_start(0.0).unwrap();
    osc.frequency().set_value(1.0).unwrap();
    graph
        .connect(osc, graph.destination())
        .expect("oscillator connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 2];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 1.0);
    assert_close(out[1].left, 1.0);
}

#[test]
fn oscillator_waveform_replaces_previously_set_periodic_wave() {
    let mut graph = AudioContext::new();
    let osc = graph.create_oscillator();
    osc.set_periodic_wave(
        PeriodicWave::try_new_with_options(
            [0.0, 0.0],
            [0.0, 0.5],
            melody_bay::PeriodicWaveOptions {
                disable_normalization: true,
            },
        )
        .unwrap(),
    );
    osc.set_type(Waveform::Square);
    osc.try_start(0.0).unwrap();
    osc.frequency().set_value(1.0).unwrap();
    graph
        .connect(osc, graph.destination())
        .expect("oscillator connects");
    let rendered = render_context_offline(&graph, 4, 2).expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[1.0, 1.0][..]));
}

#[test]
fn oscillator_detune_shifts_frequency_in_cents() {
    let mut graph = AudioContext::new();
    let osc = graph.create_oscillator();
    osc.set_type(Waveform::Sine);
    osc.try_start(0.0).unwrap();
    osc.frequency().set_value(1.0).unwrap();
    osc.detune().set_value(1200.0).unwrap();
    graph
        .connect(osc, graph.destination())
        .expect("oscillator connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(8)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 2];

    sound.process(&mut out, 0.125, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 1.0);
}

#[test]
fn oscillator_integrates_phase_across_frequency_changes() {
    let mut graph = AudioContext::new();
    let osc = graph.create_oscillator();
    osc.set_type(Waveform::Sine);
    osc.try_start(0.0).unwrap();
    osc.frequency().set_value_at_time(1.0, 0.0).unwrap();
    osc.frequency().set_value_at_time(2.0, 0.25).unwrap();
    graph
        .connect(&osc, graph.destination())
        .expect("oscillator connects");

    let rendered = render_context_offline(&graph, 4, 2).expect("graph renders");

    assert_close(rendered.channel_data(0).unwrap()[0], 0.0);
    assert_close(rendered.channel_data(0).unwrap()[1], 1.0);
}

#[test]
fn constant_source_offset_is_automatable() {
    let mut graph = AudioContext::new();
    let source = started_constant_source(&mut graph);
    source.offset().set_value(0.0).unwrap();
    source.offset().set_value(0.25).unwrap();
    let modulator = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(0.5).unwrap();
        source
    };
    graph
        .connect_param(modulator, source.offset())
        .expect("modulator connects to offset");
    graph
        .connect(&source, graph.destination())
        .expect("constant source connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.75);
}

#[test]
fn audio_param_input_uses_speaker_downmix_for_multichannel_sources() {
    let mut graph = AudioContext::new();
    let carrier = started_constant_source(&mut graph);
    carrier.offset().set_value(1.0).unwrap();
    let modulator = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_channels(
            4,
            1,
            [
                vec![0.2],
                vec![0.4],
                vec![0.6],
                vec![0.8],
                vec![1.0],
                vec![1.2],
            ],
        ),
    );
    let gain = graph.create_gain();
    gain.gain().set_value(0.0).unwrap();
    graph.connect(carrier, &gain).expect("carrier connects");
    graph
        .connect_param(modulator, gain.gain())
        .expect("modulator connects to gain AudioParam");
    graph
        .connect(&gain, graph.destination())
        .expect("gain connects");

    let out = render_graph(graph, 4, 1);
    let center_gain = 0.5_f32.sqrt();

    assert_close(
        out[0].left,
        center_gain * (0.2 + 0.4) + 0.6 + 0.5 * (1.0 + 1.2),
    );
}

#[test]
fn graph_can_render_kira_sound_data_sources() {
    let mut graph = AudioContext::new();
    let source = graph.create_sound_data_source(TestSoundData {
        frames: vec![Frame::new(0.25, 0.75), Frame::new(0.5, 0.125)],
    });
    source.try_start(0.0).unwrap();
    graph
        .connect(source, graph.destination())
        .expect("external source connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 2];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.25);
    assert_close(out[0].right, 0.75);
    assert_close(out[1].left, 0.5);
    assert_close(out[1].right, 0.125);
}

#[test]
fn sound_data_source_start_and_stop_gate_output_and_ended_state() {
    let mut graph = AudioContext::new();
    let source = graph.create_sound_data_source(TestSoundData {
        frames: vec![
            Frame::from_mono(0.25),
            Frame::from_mono(0.5),
            Frame::from_mono(0.75),
        ],
    });
    assert_eq!(source.try_stop(0.0), Err(GraphError::SourceNotStarted));
    source.try_start(0.25).unwrap();
    assert_eq!(source.try_start(0.5), Err(GraphError::SourceAlreadyStarted));
    source.try_stop(0.75).unwrap();
    graph
        .connect(&source, graph.destination())
        .expect("external source connects");

    let rendered = render_context_offline(&graph, 4, 4).expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[0.0, 0.25, 0.5, 0.0][..]));
    assert!(source.ended());
}

#[test]
fn sound_data_source_natural_end_marks_ended_and_finishes_graph() {
    let mut graph = AudioContext::new();
    let source = graph.create_sound_data_source(TestSoundData {
        frames: vec![Frame::from_mono(0.25), Frame::from_mono(0.5)],
    });
    source.try_start(0.0).unwrap();
    graph
        .connect(&source, graph.destination())
        .expect("external source connects");

    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 3];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.25);
    assert_close(out[1].left, 0.5);
    assert_close(out[2].left, 0.0);
    assert!(source.ended());
    assert!(sound.finished());
}

#[test]
fn graph_sound_data_reports_external_source_errors() {
    let mut graph = AudioContext::new();
    let source = graph.create_sound_data_source(FailingSoundData);
    graph
        .connect(source, graph.destination())
        .expect("external source connects");

    let error = match graph.sound_data().into_sound() {
        Ok(_) => panic!("external source failure should be reported"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("source failed"));
}

#[test]
fn graph_rejects_cycles() {
    let mut graph = AudioContext::new();
    let gain_a = graph.create_gain();
    let gain_b = graph.create_gain();
    graph
        .connect(&gain_a, &gain_b)
        .expect("first edge is valid");

    assert!(graph.connect(&gain_b, &gain_a).is_err());
}

#[test]
fn graph_rejects_typed_nodes_from_other_contexts() {
    let mut graph = AudioContext::new();
    let mut other_graph = AudioContext::new();
    let local_gain = graph.create_gain();
    let foreign_source = other_graph.create_constant_source();
    let foreign_gain = other_graph.create_gain();

    assert_eq!(
        graph.connect(&foreign_source, &local_gain),
        Err(GraphError::WrongContext)
    );
    assert_eq!(
        graph.connect(&local_gain, &foreign_gain),
        Err(GraphError::WrongContext)
    );
}

#[test]
fn graph_rejects_typed_audio_params_from_other_contexts() {
    let mut graph = AudioContext::new();
    let mut other_graph = AudioContext::new();
    let local_modulator = started_constant_source(&mut graph);
    let local_gain = graph.create_gain();
    let foreign_gain = other_graph.create_gain();

    assert_eq!(
        graph.connect_param(&local_modulator, foreign_gain.gain()),
        Err(GraphError::WrongContext)
    );
    assert_eq!(
        graph.disconnect_param(&local_modulator, foreign_gain.gain()),
        Err(GraphError::WrongContext)
    );
    assert_eq!(
        graph.connect_param(&local_modulator, local_gain.gain()),
        Ok(())
    );
}

#[test]
fn graph_rejects_typed_node_info_from_other_contexts() {
    let graph = AudioContext::new();
    let mut other_graph = AudioContext::new();
    let foreign_gain = other_graph.create_gain();

    assert_eq!(
        graph.node_info(&foreign_gain),
        Err(GraphError::WrongContext)
    );
}

#[test]
fn graph_allows_cycles_that_include_delay_nodes() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(0.25).unwrap();
        source
    };
    let delay = graph.create_delay();
    delay.delay_time().set_value(0.25).unwrap();
    let feedback = graph.create_gain();
    feedback.gain().set_value(0.5).unwrap();

    graph.connect(&source, &delay).expect("source connects");
    graph.connect(&delay, &feedback).expect("delay feeds gain");
    graph
        .connect(&feedback, &delay)
        .expect("delay-containing feedback cycle is allowed");
    graph
        .connect(&feedback, graph.destination())
        .expect("feedback output connects");

    let (_sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("delay cycle compiles");

    let mut no_delay_graph = AudioContext::new();
    let gain_a = no_delay_graph.create_gain();
    let gain_b = no_delay_graph.create_gain();
    no_delay_graph
        .connect(&gain_a, &gain_b)
        .expect("first non-delay edge is valid");
    assert!(no_delay_graph.connect(&gain_b, &gain_a).is_err());
}

#[test]
fn delay_cycles_feed_back_after_one_render_quantum() {
    let sample_rate = 3_000;
    let mut graph = AudioContext::try_new_with_sample_rate(sample_rate).unwrap();
    let impulse = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(
            sample_rate,
            132,
            std::iter::once(1.0).chain(std::iter::repeat_n(0.0, 131)),
        ),
    );
    let delay = graph.create_delay();
    delay
        .delay_time()
        .set_value(1.0 / sample_rate as f32)
        .unwrap();
    let feedback = graph.create_gain();
    feedback.gain().set_value(0.5).unwrap();

    graph.connect(&impulse, &delay).expect("impulse connects");
    graph.connect(&delay, &feedback).expect("delay feeds gain");
    graph
        .connect(&feedback, &delay)
        .expect("delay-containing feedback cycle is allowed");
    graph
        .connect(&feedback, graph.destination())
        .expect("feedback connects to destination");

    let rendered = render_context_offline(&graph, sample_rate, 132).expect("graph renders");
    let left = rendered.channel_data(0).expect("left channel exists");

    assert!(
        left[..128].iter().all(|sample| sample.abs() <= 0.0001),
        "feedback cycle should not re-enter within the same render quantum"
    );
    assert_close(left[128], 0.5);
}

#[test]
fn zero_delay_cycles_are_clamped_to_one_render_quantum() {
    let sample_rate = 3_000;
    let mut graph = AudioContext::try_new_with_sample_rate(sample_rate).unwrap();
    let impulse = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(
            sample_rate,
            132,
            std::iter::once(1.0).chain(std::iter::repeat_n(0.0, 131)),
        ),
    );
    let delay = graph.create_delay();
    delay.delay_time().set_value(0.0).unwrap();
    let feedback = graph.create_gain();
    feedback.gain().set_value(0.5).unwrap();

    graph.connect(&impulse, &delay).expect("impulse connects");
    graph.connect(&delay, &feedback).expect("delay feeds gain");
    graph
        .connect(&feedback, &delay)
        .expect("zero-delay feedback cycle is allowed through DelayNode");
    graph
        .connect(&feedback, graph.destination())
        .expect("feedback connects to destination");

    let rendered = render_context_offline(&graph, sample_rate, 132).expect("graph renders");
    let left = rendered.channel_data(0).expect("left channel exists");

    assert!(
        left[..128].iter().all(|sample| sample.abs() <= 0.0001),
        "zero-delay feedback cycle should be silent before one render quantum"
    );
    assert_close(left[128], 0.5);
}

#[test]
fn audio_rate_param_modulation_controls_gain() {
    let mut graph = AudioContext::new();
    let carrier = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let modulator = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(0.25).unwrap();
        source
    };
    let gain = graph.create_gain();
    gain.gain().set_value(0.0).unwrap();
    graph
        .connect_param(modulator, gain.param("gain").expect("gain param exists"))
        .expect("modulator connects to gain param");
    graph
        .connect(carrier, &gain)
        .expect("carrier connects to gain");
    graph
        .connect(&gain, graph.destination())
        .expect("gain connects to destination");

    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [kira::Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.25);
}

#[test]
fn param_modulators_render_before_targets_even_if_created_later() {
    let mut graph = AudioContext::new();
    let carrier = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let gain = graph.create_gain();
    gain.gain().set_value(0.0).unwrap();
    let modulator = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(0.25).unwrap();
        source
    };
    graph
        .connect_param(modulator, gain.param("gain").expect("gain param exists"))
        .expect("modulator connects to gain param");
    graph
        .connect(carrier, &gain)
        .expect("carrier connects to gain");
    graph
        .connect(&gain, graph.destination())
        .expect("gain connects to destination");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.25);
}

#[test]
fn set_target_and_value_curve_are_available_on_audio_param() {
    let param = ParamTimeline::new(0.0)
        .set_value_at_time(1.0, 0.0)
        .set_target_at_time(0.0, 0.0, 0.5)
        .set_value_curve_at_time([0.0, 0.5, 1.0], 1.0, 1.0);

    assert!(param.value_at(0.5) < 1.0);
    assert_close(param.value_at(1.5), 0.5);
}

#[test]
fn audio_param_can_cancel_scheduled_values() {
    let param = ParamTimeline::new(0.0)
        .linear_ramp_to_value_at_time(1.0, 1.0)
        .set_value_at_time(0.25, 1.5)
        .cancel_scheduled_values(1.25);

    assert_close(param.value_at(0.5), 0.5);
    assert_close(param.value_at(1.25), 1.0);
    assert_close(param.value_at(2.0), 1.0);
}

#[test]
fn audio_param_exposes_default_and_initial_values() {
    let param = ParamTimeline::new(0.25).set_value_at_time(0.75, 0.0);

    assert_close(param.default_value(), 0.25);
    assert_close(param.value(), 0.75);
}

#[test]
fn audio_param_can_cancel_and_hold_at_time() {
    let param = ParamTimeline::new(0.0)
        .linear_ramp_to_value_at_time(1.0, 1.0)
        .set_value_at_time(0.25, 1.5)
        .cancel_and_hold_at_time(0.5);

    assert_close(param.value_at(0.25), 0.25);
    assert_close(param.value_at(0.5), 0.5);
    assert_close(param.value_at(1.25), 0.5);
    assert_close(param.value_at(2.0), 0.5);
}

#[test]
fn audio_param_rejects_invalid_exponential_ramp_values() {
    assert_eq!(
        ParamTimeline::new(1.0).try_exponential_ramp_to_value_at_time(0.0, 1.0),
        Err(melody_bay::GraphError::InvalidAutomationValue)
    );
    let zero_start = ParamTimeline::new(0.0)
        .try_exponential_ramp_to_value_at_time(1.0, 1.0)
        .expect("zero-start exponential ramp is valid");
    assert_close(zero_start.value_at(0.5), 0.0);
    assert_close(zero_start.value_at(1.0), 1.0);

    let param = ParamTimeline::new(1.0)
        .try_exponential_ramp_to_value_at_time(4.0, 1.0)
        .expect("positive exponential ramp is valid");

    assert_close(param.value_at(0.5), 2.0);
}

#[test]
fn graph_exposes_webaudio_node_surface() {
    let sample_rate = 3_000;
    let mut graph = AudioContext::try_new_with_sample_rate(sample_rate).unwrap();
    let source = graph.create_buffer_source();
    source
        .try_set_buffer(audio_buffer_from_mono(sample_rate, 4, [1.0, 0.0, 0.0, 0.0]))
        .unwrap();
    source.try_start(0.0).unwrap();
    source.playback_rate().set_value(1.0).unwrap();
    source.detune().set_value(0.0).unwrap();
    let oscillator = graph.create_oscillator();
    oscillator.set_type(Waveform::Sine);
    oscillator.set_periodic_wave(graph.create_periodic_wave([0.0, 1.0], [0.0, 0.0]).unwrap());
    oscillator.try_start(0.0).unwrap();
    let constant = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(0.5).unwrap();
        source
    };
    let source_detune_mod = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(0.0).unwrap();
        source
    };
    let gain = graph.create_gain();
    let pan = graph.create_stereo_panner();
    let biquad = graph.create_biquad_filter();
    biquad.set_type(BiquadFilterType::Lowpass);
    biquad.frequency().set_value(440.0).unwrap();
    biquad.detune().set_value(0.0).unwrap();
    biquad.q().set_value(1.0).unwrap();
    biquad.gain().set_value(0.0).unwrap();
    let iir = graph.try_create_iir_filter([1.0], [1.0]).unwrap();
    iir.coefficients([1.0], [1.0]).unwrap();
    let delay = graph.create_delay();
    delay.delay_time().set_value(0.25).unwrap();
    let shaper = graph.create_wave_shaper();
    shaper.set_oversample(Oversample::None);
    shaper.try_curve([-1.0, 0.0, 1.0]).unwrap();
    shaper.try_curve([-1.0, 0.0, 1.0]).unwrap();
    let compressor = graph.create_dynamics_compressor();
    let convolver = graph.create_convolver();
    convolver.set_normalize(false);
    convolver
        .try_buffer(audio_buffer_from_mono(sample_rate, 1, [1.0]))
        .unwrap();
    convolver
        .try_buffer(audio_buffer_from_mono(sample_rate, 1, [1.0]))
        .unwrap();
    let analyser = graph.create_analyser();
    let splitter = graph.try_create_channel_splitter(2).unwrap();
    let merger = graph.try_create_channel_merger(2).unwrap();
    let panner = graph.create_panner();
    panner.set_distance_model(DistanceModel::Inverse);
    panner.try_ref_distance(1.0).unwrap();
    panner.try_max_distance(10.0).unwrap();
    panner.try_rolloff_factor(1.0).unwrap();
    panner.try_cone_inner_angle(180.0).unwrap();
    panner.try_cone_outer_angle(360.0).unwrap();
    panner.try_cone_outer_gain(0.25).unwrap();
    panner.orientation_x().set_value(1.0).unwrap();
    panner.orientation_y().set_value(0.0).unwrap();
    panner.orientation_z().set_value(0.0).unwrap();

    graph
        .connect(&source, &gain)
        .expect("buffer source connects");
    graph
        .connect_param(source_detune_mod, source.detune())
        .expect("buffer detune connects");
    graph
        .connect(oscillator, &gain)
        .expect("oscillator connects");
    graph.connect(constant, &gain).expect("constant connects");
    graph.connect(&gain, &pan).expect("gain connects");
    graph.connect(&pan, &biquad).expect("pan connects");
    graph.connect(&biquad, &iir).expect("filter connects");
    graph.connect(&iir, &delay).expect("iir connects");
    graph.connect(&delay, &shaper).expect("delay connects");
    graph
        .connect(&shaper, &compressor)
        .expect("shaper connects");
    graph
        .connect(&compressor, &convolver)
        .expect("compressor connects");
    graph
        .connect(&convolver, &analyser)
        .expect("convolver connects");
    graph
        .connect(&analyser, &splitter)
        .expect("analyser connects");
    graph
        .connect(&splitter, &merger)
        .expect("splitter connects");
    graph.connect(&merger, &panner).expect("merger connects");
    graph
        .connect(&panner, graph.destination())
        .expect("panner connects to destination");
}

#[test]
fn graph_exposes_webaudio_create_node_aliases() {
    struct Passthrough;
    impl melody_bay::AudioWorkletProcessor for Passthrough {
        fn process(
            &mut self,
            inputs: &[Vec<Vec<f32>>],
            outputs: &mut [Vec<Vec<f32>>],
            _context: melody_bay::AudioWorkletProcessContext,
        ) -> bool {
            for (port_index, port) in outputs.iter_mut().enumerate() {
                let Some(input_port) = inputs.get(port_index) else {
                    continue;
                };
                for (channel_index, output) in port.iter_mut().enumerate() {
                    if let Some(input) = input_port.get(channel_index) {
                        let frames = output.len();
                        output.copy_from_slice(&input[..frames]);
                    }
                }
            }
            true
        }
    }

    let sample_rate = 3_000;
    let mut graph = AudioContext::try_new_with_sample_rate(sample_rate).unwrap();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(0.5).unwrap();
        source
    };
    let gain = graph.create_gain();
    gain.gain().set_value(0.25).unwrap();
    let filter = graph.create_biquad_filter();
    filter.set_type(BiquadFilterType::Lowpass);
    let delay = graph.try_create_delay(0.5).unwrap();
    delay.delay_time().set_value(0.25).unwrap();
    let shaper = graph.create_wave_shaper();
    shaper.try_curve([-1.0, 0.0, 1.0]).unwrap();
    let panner = graph.create_stereo_panner();
    panner.pan().set_value(0.0).unwrap();
    let analyser = graph.create_analyser();
    let splitter = graph.try_create_channel_splitter(2).unwrap();
    let merger = graph.try_create_channel_merger(2).unwrap();
    let spatial = graph.create_panner();
    spatial.position_x().set_value(0.0).unwrap();
    spatial.position_y().set_value(0.0).unwrap();
    spatial.position_z().set_value(0.0).unwrap();
    let convolver = graph.create_convolver();
    convolver
        .try_buffer(audio_buffer_from_mono(sample_rate, 1, [1.0]))
        .unwrap();
    let compressor = graph.create_dynamics_compressor();
    let oscillator = graph.create_oscillator();
    oscillator.set_type(Waveform::Sine);
    oscillator.try_start(0.0).unwrap();
    oscillator.frequency().set_value(1.0).unwrap();
    let buffer_source =
        started_buffer_source_with_buffer(&mut graph, audio_buffer_from_mono(4, 1, [0.25]));
    let iir = graph.try_create_iir_filter([1.0], [1.0]).unwrap();
    let processor = graph.create_audio_worklet_node(Passthrough);

    graph.connect(source, &gain).expect("source connects");
    graph.connect(&gain, &filter).expect("gain connects");
    graph.connect(&filter, &delay).expect("filter connects");
    graph.connect(&delay, &shaper).expect("delay connects");
    graph.connect(&shaper, &panner).expect("shaper connects");
    graph.connect(&panner, &analyser).expect("panner connects");
    graph
        .connect(&analyser, graph.destination())
        .expect("analyser connects");

    assert_eq!(graph.node_info(&splitter).unwrap().number_of_outputs, 2);
    assert_eq!(graph.node_info(&merger).unwrap().number_of_inputs, 2);
    assert_eq!(graph.node_info(&spatial).unwrap().number_of_inputs, 1);
    assert_eq!(graph.node_info(&convolver).unwrap().number_of_inputs, 1);
    assert_eq!(graph.node_info(&compressor).unwrap().number_of_inputs, 1);
    assert_eq!(graph.node_info(&oscillator).unwrap().number_of_outputs, 1);
    assert_eq!(
        graph.node_info(&buffer_source).unwrap().number_of_outputs,
        1
    );
    assert_eq!(graph.node_info(&iir).unwrap().number_of_inputs, 1);
    assert_eq!(graph.node_info(&processor).unwrap().number_of_outputs, 1);
}

#[test]
fn graph_and_offline_context_create_webaudio_buffers_and_periodic_waves() {
    let graph = AudioContext::new();
    let buffer = graph.try_create_buffer(3, 4, 44_100).unwrap();
    let aliased_buffer = graph.create_buffer(3, 4, 44_100).unwrap();
    let wave = graph
        .try_create_periodic_wave([0.0, 1.0], [0.0, 0.5])
        .unwrap();
    let aliased_wave = graph.create_periodic_wave([0.0, 1.0], [0.0, 0.5]).unwrap();
    let context = OfflineAudioContext::try_new(2, 16, 44_100).unwrap();
    let context_buffer = context.try_create_buffer(1, 2, 44_100).unwrap();
    let context_aliased_buffer = context.create_buffer(1, 2, 44_100).unwrap();
    let context_wave = context
        .try_create_periodic_wave([0.0, 0.25], [0.0, 0.75])
        .unwrap();
    let context_aliased_wave = context
        .create_periodic_wave([0.0, 0.25], [0.0, 0.75])
        .unwrap();

    assert_eq!(buffer.number_of_channels(), 3);
    assert_eq!(buffer.length(), 4);
    assert_eq!(buffer.sample_rate(), 44_100);
    assert_eq!(buffer.channel_data(2), Some(&[0.0, 0.0, 0.0, 0.0][..]));
    assert_eq!(aliased_buffer.number_of_channels(), 3);
    assert_eq!(aliased_buffer.length(), 4);
    assert_eq!(
        wave,
        PeriodicWave::try_new_with_options(
            [0.0, 0.8944273],
            [0.0, 0.44721365],
            melody_bay::PeriodicWaveOptions {
                disable_normalization: true,
            },
        )
        .unwrap()
    );
    assert_eq!(
        aliased_wave,
        PeriodicWave::try_new_with_options(
            [0.0, 0.8944273],
            [0.0, 0.44721365],
            melody_bay::PeriodicWaveOptions {
                disable_normalization: true,
            },
        )
        .unwrap()
    );
    assert_eq!(context_buffer.channel_data(0), Some(&[0.0, 0.0][..]));
    assert_eq!(
        context_aliased_buffer.channel_data(0),
        Some(&[0.0, 0.0][..])
    );
    assert_eq!(
        context_wave,
        PeriodicWave::try_new_with_options(
            [0.0, 0.3162278],
            [0.0, 0.9486834],
            melody_bay::PeriodicWaveOptions {
                disable_normalization: true,
            },
        )
        .unwrap()
    );
    assert_eq!(
        context_aliased_wave,
        PeriodicWave::try_new_with_options(
            [0.0, 0.3162278],
            [0.0, 0.9486834],
            melody_bay::PeriodicWaveOptions {
                disable_normalization: true,
            },
        )
        .unwrap()
    );
}

#[test]
fn parameterized_nodes_support_named_webaudio_param_lookup() {
    let mut graph = AudioContext::new();
    let oscillator = graph.create_oscillator();
    oscillator.set_type(Waveform::Sine);
    oscillator.try_start(0.0).unwrap();
    let constant = started_constant_source(&mut graph);
    let buffer = started_buffer_source_with_buffer(&mut graph, audio_buffer_from_mono(4, 1, [1.0]));
    let gain = graph.create_gain();
    let stereo_panner = graph.create_stereo_panner();
    let filter = graph.create_biquad_filter();
    filter.set_type(BiquadFilterType::Lowpass);
    let delay = graph.create_delay();
    delay.delay_time().set_value(1.0).unwrap();
    let compressor = graph.create_dynamics_compressor();
    let spatial = graph.create_panner();

    assert_eq!(
        oscillator.param("frequency").unwrap().value(),
        oscillator.frequency().value()
    );
    assert_eq!(
        oscillator.param("detune").unwrap().value(),
        oscillator.detune().value()
    );
    assert_eq!(
        constant.param("offset").unwrap().value(),
        constant.offset().value()
    );
    assert_eq!(
        buffer.param("playbackRate").unwrap().value(),
        buffer.playback_rate().value()
    );
    assert_eq!(
        buffer.param("detune").unwrap().value(),
        buffer.detune().value()
    );
    assert_eq!(gain.param("gain").unwrap().value(), gain.gain().value());
    assert_eq!(
        stereo_panner.param("pan").unwrap().value(),
        stereo_panner.pan().value()
    );
    assert_eq!(
        filter.param("frequency").unwrap().value(),
        filter.frequency().value()
    );
    assert_eq!(
        filter.param("detune").unwrap().value(),
        filter.detune().value()
    );
    assert_eq!(filter.param("Q").unwrap().value(), filter.q().value());
    assert_eq!(filter.param("gain").unwrap().value(), filter.gain().value());
    assert_eq!(
        delay.param("delayTime").unwrap().value(),
        delay.delay_time().value()
    );
    assert_eq!(
        compressor.param("threshold").unwrap().value(),
        compressor.threshold().value()
    );
    assert_eq!(
        compressor.param("knee").unwrap().value(),
        compressor.knee().value()
    );
    assert_eq!(
        compressor.param("ratio").unwrap().value(),
        compressor.ratio().value()
    );
    assert_eq!(
        compressor.param("attack").unwrap().value(),
        compressor.attack().value()
    );
    assert_eq!(
        compressor.param("release").unwrap().value(),
        compressor.release().value()
    );
    assert_eq!(
        spatial.param("positionX").unwrap().value(),
        spatial.position_x().value()
    );
    assert_eq!(
        spatial.param("orientationZ").unwrap().value(),
        spatial.orientation_z().value()
    );
    assert!(spatial.param("missing").is_none());
}

#[test]
fn stereo_panner_moves_signal_between_channels() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let pan = graph.create_stereo_panner();
    pan.pan().set_value(1.0).unwrap();
    graph.connect(source, &pan).expect("source connects");
    graph
        .connect(&pan, graph.destination())
        .expect("pan connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [kira::Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[0].right, 1.0);
}

#[test]
fn stereo_panner_uses_equal_power_curve_for_mono_input() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let pan = graph.create_stereo_panner();
    pan.pan().set_value(0.0).unwrap();
    graph.connect(source, &pan).expect("source connects");
    graph
        .connect(&pan, graph.destination())
        .expect("pan connects");
    let out = render_graph(graph, 4, 1);
    let center_gain = 0.5_f32.sqrt();

    assert_close(out[0].left, center_gain);
    assert_close(out[0].right, center_gain);
}

#[test]
fn stereo_panner_default_channel_config_clamps_quad_input_to_stereo() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_channels(4, 1, [[0.2], [0.4], [0.6], [0.8]]),
    );
    let pan = graph.create_stereo_panner();
    pan.pan().set_value(0.0).unwrap();
    graph.connect(source, &pan).expect("source connects");
    graph
        .connect(&pan, graph.destination())
        .expect("pan connects");

    let out = render_graph(graph, 4, 1);

    assert_close(out[0].left, 0.5);
    assert_close(out[0].right, 0.8);
}

#[test]
fn audio_param_inputs_are_clamped_to_nominal_range() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let modulation = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(4.0).unwrap();
        source
    };
    let pan = graph.create_stereo_panner();
    graph
        .connect_param(modulation, pan.pan())
        .expect("modulator connects to pan");
    graph.connect(source, &pan).expect("source connects");
    graph
        .connect(&pan, graph.destination())
        .expect("pan connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [kira::Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[0].right, 1.0);
}

#[test]
fn channel_splitter_routes_individual_stereo_channels() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_stereo(4, 1, [0.25], [0.75]),
    );
    let splitter = graph.try_create_channel_splitter(2).unwrap();
    graph.connect(source, &splitter).expect("source connects");
    graph
        .connect_with_indices(&splitter, 1, graph.destination(), 0)
        .expect("splitter output connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.75);
    assert_close(out[0].right, 0.75);
}

#[test]
fn channel_merger_combines_inputs_into_stereo_output() {
    let mut graph = AudioContext::new();
    let left = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(0.25).unwrap();
        source
    };
    let right = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(0.75).unwrap();
        source
    };
    let merger = graph.try_create_channel_merger(2).unwrap();
    graph
        .connect_with_indices(left, 0, &merger, 0)
        .expect("left source connects");
    graph
        .connect_with_indices(right, 0, &merger, 1)
        .expect("right source connects");
    graph
        .connect(&merger, graph.destination())
        .expect("merger connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.25);
    assert_close(out[0].right, 0.75);
}

#[test]
fn explicit_mono_channel_count_downmixes_stereo_input() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_stereo(4, 1, [0.25], [0.75]),
    );
    let gain = graph.create_gain();
    gain.try_set_channel_config(
        1,
        ChannelCountMode::Explicit,
        ChannelInterpretation::Speakers,
    )
    .expect("channel config updates");
    graph.connect(source, &gain).expect("source connects");
    graph
        .connect(&gain, graph.destination())
        .expect("gain connects");

    let out = render_graph(graph, 4, 1);

    assert_close(out[0].left, 0.5);
    assert_close(out[0].right, 0.5);
}

#[test]
fn explicit_discrete_channel_count_zero_fills_missing_channels() {
    let mut graph = AudioContext::new();
    let source =
        started_buffer_source_with_buffer(&mut graph, audio_buffer_from_mono(4, 1, [0.75]));
    let gain = graph.create_gain();
    gain.try_set_channel_config(
        2,
        ChannelCountMode::Explicit,
        ChannelInterpretation::Discrete,
    )
    .expect("channel config updates");
    graph.connect(source, &gain).expect("source connects");
    graph
        .connect(&gain, graph.destination())
        .expect("gain connects");

    let out = render_graph(graph, 4, 1);

    assert_close(out[0].left, 0.75);
    assert_close(out[0].right, 0.0);
}

#[test]
fn explicit_speaker_channel_count_upmixes_to_quad_and_5_1_layouts() {
    let mut graph = OfflineAudioContext::try_new(4, 1, 3_000).unwrap();
    let source =
        started_offline_buffer_source_with_buffer(&mut graph, audio_buffer_from_mono(4, 1, [0.75]));
    let gain = graph.create_gain();
    gain.try_set_channel_config(
        4,
        ChannelCountMode::Explicit,
        ChannelInterpretation::Speakers,
    )
    .expect("channel config updates");
    graph.connect(source, &gain).expect("source connects");
    graph
        .connect(&gain, graph.destination())
        .expect("gain connects");

    let out = graph.start_rendering().expect("graph renders");

    assert_eq!(out.channel_data(0), Some(&[0.75][..]));
    assert_eq!(out.channel_data(1), Some(&[0.75][..]));
    assert_eq!(out.channel_data(2), Some(&[0.0][..]));
    assert_eq!(out.channel_data(3), Some(&[0.0][..]));

    let mut graph = OfflineAudioContext::try_new(6, 1, 3_000).unwrap();
    let source = started_offline_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_stereo(4, 1, [0.25], [0.5]),
    );
    let gain = graph.create_gain();
    gain.try_set_channel_config(
        6,
        ChannelCountMode::Explicit,
        ChannelInterpretation::Speakers,
    )
    .expect("channel config updates");
    graph.connect(source, &gain).expect("source connects");
    graph
        .connect(&gain, graph.destination())
        .expect("gain connects");

    let out = graph.start_rendering().expect("graph renders");

    assert_eq!(out.channel_data(0), Some(&[0.25][..]));
    assert_eq!(out.channel_data(1), Some(&[0.5][..]));
    assert_eq!(out.channel_data(2), Some(&[0.0][..]));
    assert_eq!(out.channel_data(3), Some(&[0.0][..]));
    assert_eq!(out.channel_data(4), Some(&[0.0][..]));
    assert_eq!(out.channel_data(5), Some(&[0.0][..]));
}

#[test]
fn explicit_speaker_channel_count_downmixes_quad_and_5_1_layouts() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_channels(4, 1, [vec![0.2], vec![0.4], vec![0.6], vec![0.8]]),
    );
    let gain = graph.create_gain();
    gain.try_set_channel_config(
        2,
        ChannelCountMode::Explicit,
        ChannelInterpretation::Speakers,
    )
    .expect("channel config updates");
    graph.connect(source, &gain).expect("source connects");
    graph
        .connect(&gain, graph.destination())
        .expect("gain connects");

    let out = render_context_offline_channels(&graph, 4, 1, 2).expect("graph renders");

    assert_close(out.channel_data(0).unwrap()[0], 0.2 + 0.5 * 0.6);
    assert_close(out.channel_data(1).unwrap()[0], 0.4 + 0.5 * 0.8);

    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_channels(
            4,
            1,
            [
                vec![0.2],
                vec![0.4],
                vec![0.6],
                vec![0.8],
                vec![1.0],
                vec![1.2],
            ],
        ),
    );
    let gain = graph.create_gain();
    gain.try_set_channel_config(
        2,
        ChannelCountMode::Explicit,
        ChannelInterpretation::Speakers,
    )
    .expect("channel config updates");
    graph.connect(source, &gain).expect("source connects");
    graph
        .connect(&gain, graph.destination())
        .expect("gain connects");

    let out = render_context_offline_channels(&graph, 4, 1, 2).expect("graph renders");
    let center_gain = 0.5_f32.sqrt();

    assert_close(
        out.channel_data(0).unwrap()[0],
        0.2 + center_gain * 0.6 + 0.5 * 1.0,
    );
    assert_close(
        out.channel_data(1).unwrap()[0],
        0.4 + center_gain * 0.6 + 0.5 * 1.2,
    );

    let mut graph = OfflineAudioContext::try_new(4, 1, 3_000).unwrap();
    let source = started_offline_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_channels(
            4,
            1,
            [
                vec![0.2],
                vec![0.4],
                vec![0.6],
                vec![0.8],
                vec![1.0],
                vec![1.2],
            ],
        ),
    );
    let gain = graph.create_gain();
    gain.try_set_channel_config(
        4,
        ChannelCountMode::Explicit,
        ChannelInterpretation::Speakers,
    )
    .expect("channel config updates");
    graph.connect(source, &gain).expect("source connects");
    graph
        .connect(&gain, graph.destination())
        .expect("gain connects");

    let out = graph.start_rendering().expect("graph renders");
    let center_gain = 0.5_f32.sqrt();

    assert_close(out.channel_data(0).unwrap()[0], 0.2 + center_gain * 0.6);
    assert_close(out.channel_data(1).unwrap()[0], 0.4 + center_gain * 0.6);
    assert_eq!(out.channel_data(2), Some(&[1.0][..]));
    assert_eq!(out.channel_data(3), Some(&[1.2][..]));
}

#[test]
fn delay_node_outputs_after_delay_time() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [1.0, 0.0, 0.0, 0.0]),
    );
    let delay = graph.create_delay();
    delay.delay_time().set_value(0.5).unwrap();
    graph.connect(source, &delay).expect("source connects");
    graph
        .connect(&delay, graph.destination())
        .expect("delay connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [kira::Frame::ZERO; 3];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[2].left, 1.0);
}

#[test]
fn delay_node_zero_delay_outputs_current_sample() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [0.25, 0.5, 0.75, 1.0]),
    );
    let delay = graph.create_delay();
    delay.delay_time().set_value(0.0).unwrap();
    graph.connect(source, &delay).expect("source connects");
    graph
        .connect(&delay, graph.destination())
        .expect("delay connects");

    let rendered = render_context_offline(&graph, 4, 4).expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[0.25, 0.5, 0.75, 1.0][..]));
}

#[test]
fn delay_node_preserves_arbitrary_channel_buses() {
    let mut graph = OfflineAudioContext::try_new(4, 2, 3_000).unwrap();
    let source = started_offline_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_channels(
            3_000,
            2,
            [
                vec![0.25, 0.0],
                vec![0.5, 0.0],
                vec![0.75, 0.0],
                vec![1.0, 0.0],
            ],
        ),
    );
    let delay = graph.create_delay();
    delay.delay_time().set_value(1.0 / 3_000.0).unwrap();
    graph.connect(source, &delay).expect("source connects");
    graph
        .connect(&delay, graph.destination())
        .expect("delay connects");

    let rendered = graph.start_rendering().expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[0.0, 0.25][..]));
    assert_eq!(rendered.channel_data(1), Some(&[0.0, 0.5][..]));
    assert_eq!(rendered.channel_data(2), Some(&[0.0, 0.75][..]));
    assert_eq!(rendered.channel_data(3), Some(&[0.0, 1.0][..]));
}

#[test]
fn delay_node_interpolates_fractional_delay_time() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [1.0, 0.0, 0.0, 0.0]),
    );
    let delay = graph.create_delay();
    delay.delay_time().set_value(0.375).unwrap();
    graph.connect(source, &delay).expect("source connects");
    graph
        .connect(&delay, graph.destination())
        .expect("delay connects");

    let buffer = render_context_offline(&graph, 4, 4).expect("graph renders");

    assert_eq!(buffer.channel_data(0), Some(&[0.0, 0.5, 0.5, 0.0][..]));
}

#[test]
fn delay_node_clamps_to_configured_max_delay_time() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [1.0, 0.0, 0.0, 0.0]),
    );
    let delay = graph.try_create_delay(0.25).unwrap();
    delay.delay_time().set_value(1.0).unwrap();
    graph.connect(source, &delay).expect("source connects");
    graph
        .connect(&delay, graph.destination())
        .expect("delay connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 2];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[1].left, 1.0);
}

#[test]
fn audio_rate_param_modulation_controls_delay_time() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [1.0, 0.0, 0.0, 0.0]),
    );
    let modulator = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(0.5).unwrap();
        source
    };
    let delay = graph.create_delay();
    delay.delay_time().set_value(0.0).unwrap();
    graph
        .connect_param(modulator, delay.delay_time())
        .expect("modulator connects to delay time");
    graph.connect(source, &delay).expect("source connects");
    graph
        .connect(&delay, graph.destination())
        .expect("delay connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [kira::Frame::ZERO; 3];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.0);
    assert_close(out[2].left, 1.0);
}

#[test]
fn waveshaper_maps_input_through_curve() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(0.5).unwrap();
        source
    };
    let shaper = graph.create_wave_shaper();
    shaper.try_curve([-1.0, 0.0, 1.0]).unwrap();
    graph.connect(source, &shaper).expect("source connects");
    graph
        .connect(&shaper, graph.destination())
        .expect("shaper connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [kira::Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.5);
}

#[test]
fn waveshaper_node_curve_can_be_replaced() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(0.5).unwrap();
        source
    };
    let shaper = graph.create_wave_shaper();
    shaper.set_oversample(Oversample::TwoX);
    shaper.try_curve([-1.0, 0.0, 1.0]).unwrap();
    shaper.try_curve([-1.0, 0.0, 0.25]).unwrap();
    graph.connect(source, &shaper).expect("source connects");
    graph
        .connect(&shaper, graph.destination())
        .expect("shaper connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.125);
}

#[test]
fn waveshaper_oversampling_lowpass_filters_before_downsampling() {
    let mut graph = AudioContext::new();
    let source =
        started_buffer_source_with_buffer(&mut graph, audio_buffer_from_mono(4, 2, [0.0, 1.0]));
    let shaper = graph.create_wave_shaper();
    shaper.set_oversample(Oversample::FourX);
    shaper.try_curve([0.0, 0.0, 1.0]).unwrap();
    graph.connect(source, &shaper).expect("source connects");
    graph
        .connect(&shaper, graph.destination())
        .expect("shaper connects");
    let buffer = render_context_offline(&graph, 4, 2).expect("graph renders");

    let output = buffer.channel_data(0).expect("mono output");
    assert_close(output[0], 0.0);
    assert!(
        output[1] > 0.0 && output[1] < 0.625,
        "expected oversampling to low-pass before downsampling, got {}",
        output[1]
    );
}

#[test]
fn waveshaper_oversampling_factor_changes_downsampled_result() {
    fn render_step(oversample: Oversample) -> f32 {
        let mut graph = AudioContext::new();
        let source =
            started_buffer_source_with_buffer(&mut graph, audio_buffer_from_mono(4, 2, [0.0, 1.0]));
        let shaper = graph.create_wave_shaper();
        shaper.set_oversample(oversample);
        shaper.try_curve([0.0, 0.0, 1.0]).unwrap();
        graph.connect(source, &shaper).expect("source connects");
        graph
            .connect(&shaper, graph.destination())
            .expect("shaper connects");
        let buffer = render_context_offline(&graph, 4, 2).expect("graph renders");
        buffer.channel_data(0).expect("mono output")[1]
    }

    let two_x = render_step(Oversample::TwoX);
    let four_x = render_step(Oversample::FourX);

    assert!(
        four_x < two_x,
        "expected 4x oversampling to apply a stronger downsampling filter than 2x, got 2x={two_x}, 4x={four_x}"
    );
}

#[test]
fn iir_filter_node_coefficients_can_be_replaced() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [1.0, 0.0, 0.0, 0.0]),
    );
    let filter = graph.try_create_iir_filter([1.0], [1.0]).unwrap();
    filter.coefficients([0.5, 0.5], [1.0]).unwrap();
    graph.connect(source, &filter).expect("source connects");
    graph
        .connect(&filter, graph.destination())
        .expect("filter connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 2];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.5);
    assert_close(out[1].left, 0.5);
}

#[test]
fn iir_filter_preserves_arbitrary_channel_buses() {
    let mut graph = OfflineAudioContext::try_new(4, 1, 3_000).unwrap();
    let source = started_offline_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_channels(4, 1, [vec![0.25], vec![0.5], vec![0.75], vec![1.0]]),
    );
    let filter = graph.try_create_iir_filter([1.0], [1.0]).unwrap();
    graph.connect(source, &filter).expect("source connects");
    graph
        .connect(&filter, graph.destination())
        .expect("filter connects");

    let rendered = graph.start_rendering().expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[0.25][..]));
    assert_eq!(rendered.channel_data(1), Some(&[0.5][..]));
    assert_eq!(rendered.channel_data(2), Some(&[0.75][..]));
    assert_eq!(rendered.channel_data(3), Some(&[1.0][..]));
}

#[test]
fn biquad_filter_preserves_arbitrary_channel_buses() {
    let mut graph = OfflineAudioContext::try_new(4, 1, 3_000).unwrap();
    let source = started_offline_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_channels(4, 1, [vec![0.25], vec![0.5], vec![0.75], vec![1.0]]),
    );
    let filter = graph.create_biquad_filter();
    filter.set_type(BiquadFilterType::Peaking);
    filter.gain().set_value(0.0).unwrap();
    graph.connect(source, &filter).expect("source connects");
    graph
        .connect(&filter, graph.destination())
        .expect("filter connects");

    let rendered = graph.start_rendering().expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[0.25][..]));
    assert_eq!(rendered.channel_data(1), Some(&[0.5][..]));
    assert_eq!(rendered.channel_data(2), Some(&[0.75][..]));
    assert_eq!(rendered.channel_data(3), Some(&[1.0][..]));
}

#[test]
fn lowpass_filter_smooths_step_input() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [0.0, 1.0, 1.0, 1.0]),
    );
    let filter = graph.create_biquad_filter();
    filter.set_type(BiquadFilterType::Lowpass);
    filter.frequency().set_value(1.0).unwrap();
    graph.connect(source, &filter).expect("source connects");
    graph
        .connect(&filter, graph.destination())
        .expect("filter connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [kira::Frame::ZERO; 3];

    sound.process(&mut out, 0.25, &info);

    assert!(out[1].left > 0.0);
    assert!(out[1].left < 1.0);
}

#[test]
fn biquad_frequency_accepts_audio_rate_modulation() {
    let mut slow_graph = AudioContext::new();
    let slow_source = started_buffer_source_with_buffer(
        &mut slow_graph,
        audio_buffer_from_mono(4, 4, [0.0, 1.0, 1.0, 1.0]),
    );
    let slow_filter = slow_graph.create_biquad_filter();
    slow_filter.set_type(BiquadFilterType::Lowpass);
    slow_filter.frequency().set_value(1.0).unwrap();
    slow_graph
        .connect(slow_source, &slow_filter)
        .expect("slow source connects");
    slow_graph
        .connect(&slow_filter, slow_graph.destination())
        .expect("slow filter connects");

    let mut modulated_graph = AudioContext::new();
    let modulated_source = started_buffer_source_with_buffer(
        &mut modulated_graph,
        audio_buffer_from_mono(4, 4, [0.0, 1.0, 1.0, 1.0]),
    );
    let modulation = started_constant_source(&mut modulated_graph);
    modulation.offset().set_value(9.0).unwrap();
    let modulated_filter = modulated_graph.create_biquad_filter();
    modulated_filter.set_type(BiquadFilterType::Lowpass);
    modulated_filter.frequency().set_value(1.0).unwrap();
    modulated_graph
        .connect_param(modulation, modulated_filter.frequency())
        .expect("modulator connects to filter frequency");
    modulated_graph
        .connect(modulated_source, &modulated_filter)
        .expect("modulated source connects");
    modulated_graph
        .connect(&modulated_filter, modulated_graph.destination())
        .expect("modulated filter connects");

    let info = MockInfoBuilder::new().build();
    let (mut slow_sound, _) = slow_graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("slow graph should build");
    let (mut modulated_sound, _) = modulated_graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("modulated graph should build");
    let mut slow = [Frame::ZERO; 2];
    let mut modulated = [Frame::ZERO; 2];

    slow_sound.process(&mut slow, 0.25, &info);
    modulated_sound.process(&mut modulated, 0.25, &info);

    assert!(modulated[1].left > slow[1].left);
}

#[test]
fn biquad_filter_exposes_webaudio_params() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let filter = graph.create_biquad_filter();
    filter.set_type(BiquadFilterType::Highpass);
    filter.frequency().set_value(10.0).unwrap();
    filter.detune().set_value(1200.0).unwrap();
    filter.q().set_value(0.5).unwrap();
    filter.gain().set_value(3.0).unwrap();
    let frequency_mod = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let detune_mod = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(0.0).unwrap();
        source
    };
    let q_mod = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(0.0).unwrap();
        source
    };
    let gain_mod = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(0.0).unwrap();
        source
    };

    graph
        .connect_param(frequency_mod, filter.frequency())
        .expect("frequency param connects");
    graph
        .connect_param(detune_mod, filter.detune())
        .expect("detune param connects");
    graph
        .connect_param(q_mod, filter.q())
        .expect("q param connects");
    graph
        .connect_param(gain_mod, filter.gain())
        .expect("gain param connects");
    graph.connect(source, &filter).expect("source connects");
    graph
        .connect(&filter, graph.destination())
        .expect("filter connects");

    let (_sound, _) = graph.sound_data().into_sound().expect("graph should build");
}

#[test]
fn biquad_filter_type_can_be_changed_after_creation() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(0.25).unwrap();
        source
    };
    let filter = graph.create_biquad_filter();
    filter.set_type(BiquadFilterType::Highshelf);
    filter.set_type(BiquadFilterType::Lowshelf);
    filter.frequency().set_value(10.0).unwrap();
    filter.gain().set_value(12.0).unwrap();
    graph.connect(source, &filter).expect("source connects");
    graph
        .connect(&filter, graph.destination())
        .expect("filter connects");

    let frames = render_graph(graph, 100, 400);

    assert!(frames[399].left > 0.75);
}

#[test]
fn biquad_filter_supports_bandpass_notch_and_allpass_modes() {
    let mut bandpass_graph = AudioContext::new();
    let bandpass_source = bandpass_graph.create_oscillator();
    bandpass_source.set_type(Waveform::Sine);
    bandpass_source.try_start(0.0).unwrap();
    bandpass_source.frequency().set_value(10.0).unwrap();
    let bandpass = bandpass_graph.create_biquad_filter();
    bandpass.set_type(BiquadFilterType::Bandpass);
    bandpass.frequency().set_value(10.0).unwrap();
    bandpass.q().set_value(4.0).unwrap();
    bandpass_graph
        .connect(bandpass_source, &bandpass)
        .expect("source connects");
    bandpass_graph
        .connect(&bandpass, bandpass_graph.destination())
        .expect("filter connects");

    let mut notch_graph = AudioContext::new();
    let notch_source = notch_graph.create_oscillator();
    notch_source.set_type(Waveform::Sine);
    notch_source.try_start(0.0).unwrap();
    notch_source.frequency().set_value(10.0).unwrap();
    let notch = notch_graph.create_biquad_filter();
    notch.set_type(BiquadFilterType::Notch);
    notch.frequency().set_value(10.0).unwrap();
    notch.q().set_value(8.0).unwrap();
    notch_graph
        .connect(notch_source, &notch)
        .expect("source connects");
    notch_graph
        .connect(&notch, notch_graph.destination())
        .expect("filter connects");

    let mut allpass_graph = AudioContext::new();
    let allpass_source = allpass_graph.create_oscillator();
    allpass_source.set_type(Waveform::Sine);
    allpass_source.try_start(0.0).unwrap();
    allpass_source.frequency().set_value(10.0).unwrap();
    let allpass = allpass_graph.create_biquad_filter();
    allpass.set_type(BiquadFilterType::Allpass);
    allpass.frequency().set_value(10.0).unwrap();
    allpass.q().set_value(1.0).unwrap();
    allpass_graph
        .connect(allpass_source, &allpass)
        .expect("source connects");
    allpass_graph
        .connect(&allpass, allpass_graph.destination())
        .expect("filter connects");

    let bandpass_frames = render_graph(bandpass_graph, 100, 400);
    let notch_frames = render_graph(notch_graph, 100, 400);
    let allpass_frames = render_graph(allpass_graph, 100, 400);

    assert!(rms(&bandpass_frames[200..]) > rms(&notch_frames[200..]) * 5.0);
    assert!(rms(&allpass_frames[200..]) > 0.5);
}

#[test]
fn biquad_filter_supports_shelf_and_peaking_modes() {
    let mut low_shelf_graph = AudioContext::new();
    let low_shelf_source = started_constant_source(&mut low_shelf_graph);
    low_shelf_source.offset().set_value(0.25).unwrap();
    let low_shelf = low_shelf_graph.create_biquad_filter();
    low_shelf.set_type(BiquadFilterType::Lowshelf);
    low_shelf.frequency().set_value(10.0).unwrap();
    low_shelf.gain().set_value(12.0).unwrap();
    low_shelf_graph
        .connect(low_shelf_source, &low_shelf)
        .expect("source connects");
    low_shelf_graph
        .connect(&low_shelf, low_shelf_graph.destination())
        .expect("filter connects");

    let mut peaking_graph = AudioContext::new();
    let peaking_source = peaking_graph.create_oscillator();
    peaking_source.set_type(Waveform::Sine);
    peaking_source.try_start(0.0).unwrap();
    peaking_source.frequency().set_value(10.0).unwrap();
    let peaking = peaking_graph.create_biquad_filter();
    peaking.set_type(BiquadFilterType::Peaking);
    peaking.frequency().set_value(10.0).unwrap();
    peaking.q().set_value(4.0).unwrap();
    peaking.gain().set_value(12.0).unwrap();
    peaking_graph
        .connect(peaking_source, &peaking)
        .expect("source connects");
    peaking_graph
        .connect(&peaking, peaking_graph.destination())
        .expect("filter connects");

    let low_shelf_frames = render_graph(low_shelf_graph, 100, 400);
    let peaking_frames = render_graph(peaking_graph, 100, 400);

    assert!(low_shelf_frames[399].left > 0.75);
    assert!(rms(&peaking_frames[200..]) > 1.0);
}

#[test]
fn convolver_applies_impulse_response() {
    let sample_rate = 3_000;
    let mut graph = AudioContext::try_new_with_sample_rate(sample_rate).unwrap();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(sample_rate, 4, [1.0, 0.0, 0.0, 0.0]),
    );
    let convolver = graph.create_convolver();
    convolver.set_normalize(false);
    convolver
        .try_buffer(audio_buffer_from_mono(sample_rate, 2, [0.5, 0.25]))
        .unwrap();
    graph.connect(source, &convolver).expect("source connects");
    graph
        .connect(&convolver, graph.destination())
        .expect("convolver connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(sample_rate)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [kira::Frame::ZERO; 3];

    sound.process(&mut out, 1.0 / sample_rate as f64, &info);

    assert_close(out[0].left, 0.5);
    assert_close(out[1].left, 0.25);
    assert_close(out[2].left, 0.0);
}

#[test]
fn convolver_node_buffer_can_be_replaced_and_normalized() {
    let sample_rate = 3_000;
    let mut graph = AudioContext::try_new_with_sample_rate(sample_rate).unwrap();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(sample_rate, 4, [1.0, 0.0, 0.0, 0.0]),
    );
    let convolver = graph.create_convolver();
    convolver.set_normalize(true);
    convolver
        .try_buffer(audio_buffer_from_mono(sample_rate, 1, [0.25]))
        .unwrap();
    convolver
        .try_buffer(audio_buffer_from_mono(sample_rate, 2, [2.0, 1.0]))
        .unwrap();
    graph.connect(source, &convolver).expect("source connects");
    graph
        .connect(&convolver, graph.destination())
        .expect("convolver connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(sample_rate)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 3];

    sound.process(&mut out, 1.0 / sample_rate as f64, &info);

    assert_close(out[0].left, 0.02324274);
    assert_close(out[1].left, 0.01162137);
    assert_close(out[2].left, 0.0);
}

#[test]
fn convolver_normalization_uses_webaudio_rms_calibration() {
    let sample_rate = 44_100;
    let mut graph = AudioContext::try_new_with_sample_rate(sample_rate).unwrap();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(sample_rate, 1, [1.0]),
    );
    let convolver = graph.create_convolver();
    convolver.set_normalize(true);
    convolver
        .try_buffer(audio_buffer_from_mono(sample_rate, 1, [0.25]))
        .unwrap();
    graph.connect(source, &convolver).expect("source connects");
    graph
        .connect(&convolver, graph.destination())
        .expect("convolver connects");

    let rendered = render_context_offline(&graph, sample_rate, 1).expect("graph renders");

    assert_close(rendered.channel_data(0).unwrap()[0], 0.00125);
}

#[test]
fn convolver_normalized_zero_impulse_renders_silence() {
    let sample_rate = 3_000;
    let mut graph = AudioContext::try_new_with_sample_rate(sample_rate).unwrap();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(sample_rate, 1, [1.0]),
    );
    let convolver = graph.create_convolver();
    convolver.set_normalize(true);
    convolver
        .try_buffer(audio_buffer_from_mono(sample_rate, 1, [0.0]))
        .unwrap();
    graph.connect(source, &convolver).expect("source connects");
    graph
        .connect(&convolver, graph.destination())
        .expect("convolver connects");

    let rendered = render_context_offline(&graph, sample_rate, 1).expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[0.0][..]));
}

#[test]
fn convolver_normalize_changes_apply_only_to_subsequent_buffers() {
    let sample_rate = 3_000;
    let mut graph = AudioContext::try_new_with_sample_rate(sample_rate).unwrap();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(sample_rate, 1, [1.0]),
    );
    let convolver = graph.create_convolver();
    convolver.set_normalize(true);
    convolver
        .try_buffer(audio_buffer_from_mono(sample_rate, 1, [0.25]))
        .unwrap();
    convolver.set_normalize(false);
    graph.connect(source, &convolver).expect("source connects");
    graph
        .connect(&convolver, graph.destination())
        .expect("convolver connects");

    let rendered = render_context_offline(&graph, sample_rate, 1).expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[0.018375][..]));
}

#[test]
fn convolver_mono_input_with_mono_impulse_outputs_mono() {
    let sample_rate = 3_000;
    let mut graph = AudioContext::try_new_with_sample_rate(sample_rate).unwrap();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(sample_rate, 1, [1.0]),
    );
    let convolver = graph.create_convolver();
    convolver.set_normalize(false);
    convolver
        .try_buffer(audio_buffer_from_mono(sample_rate, 1, [0.5]))
        .unwrap();
    let splitter = graph.try_create_channel_splitter(2).unwrap();
    let merger = graph.try_create_channel_merger(2).unwrap();
    graph.connect(source, &convolver).expect("source connects");
    graph
        .connect(&convolver, &splitter)
        .expect("convolver connects");
    graph
        .connect_with_indices(&splitter, 1, &merger, 0)
        .expect("right splitter output connects to destination left");
    graph
        .connect(&merger, graph.destination())
        .expect("merger connects");

    let rendered = render_context_offline(&graph, sample_rate, 1).expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[0.0][..]));
}

#[test]
fn convolver_mono_input_with_stereo_impulse_outputs_stereo() {
    let sample_rate = 3_000;
    let mut graph = AudioContext::try_new_with_sample_rate(sample_rate).unwrap();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(sample_rate, 1, [2.0]),
    );
    let convolver = graph.create_convolver();
    convolver.set_normalize(false);
    convolver
        .try_buffer(audio_buffer_from_stereo(sample_rate, 1, [0.25], [0.5]))
        .unwrap();
    graph.connect(source, &convolver).expect("source connects");
    graph
        .connect(&convolver, graph.destination())
        .expect("convolver connects");

    let rendered = render_context_offline(&graph, sample_rate, 1).expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[0.5][..]));
    assert_eq!(rendered.channel_data(1), Some(&[1.0][..]));
}

#[test]
fn convolver_stereo_input_with_mono_impulse_preserves_stereo_channels() {
    let sample_rate = 3_000;
    let mut graph = AudioContext::try_new_with_sample_rate(sample_rate).unwrap();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_stereo(sample_rate, 1, [2.0], [4.0]),
    );
    let convolver = graph.create_convolver();
    convolver.set_normalize(false);
    convolver
        .try_buffer(audio_buffer_from_mono(sample_rate, 1, [0.5]))
        .unwrap();
    graph.connect(source, &convolver).expect("source connects");
    graph
        .connect(&convolver, graph.destination())
        .expect("convolver connects");

    let rendered = render_context_offline(&graph, sample_rate, 1).expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[1.0][..]));
    assert_eq!(rendered.channel_data(1), Some(&[2.0][..]));
}

#[test]
fn convolver_four_channel_impulse_crossfeeds_stereo_input() {
    let sample_rate = 3_000;
    let mut graph = AudioContext::try_new_with_sample_rate(sample_rate).unwrap();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_stereo(sample_rate, 1, [1.0], [2.0]),
    );
    let convolver = graph.create_convolver();
    convolver.set_normalize(false);
    convolver
        .try_buffer(audio_buffer_from_channels(
            sample_rate,
            1,
            [vec![0.25], vec![0.5], vec![0.75], vec![1.0]],
        ))
        .unwrap();
    graph.connect(source, &convolver).expect("source connects");
    graph
        .connect(&convolver, graph.destination())
        .expect("convolver connects");

    let rendered = render_context_offline(&graph, sample_rate, 1).expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[1.75][..]));
    assert_eq!(rendered.channel_data(1), Some(&[2.5][..]));
}

#[test]
fn convolver_default_channel_config_clamps_quad_input_to_stereo() {
    let sample_rate = 3_000;
    let mut graph = AudioContext::try_new_with_sample_rate(sample_rate).unwrap();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_channels(sample_rate, 1, [vec![0.2], vec![0.4], vec![0.6], vec![0.8]]),
    );
    let convolver = graph.create_convolver();
    convolver.set_normalize(false);
    convolver
        .try_buffer(audio_buffer_from_mono(sample_rate, 1, [1.0]))
        .unwrap();
    graph.connect(source, &convolver).expect("source connects");
    graph
        .connect(&convolver, graph.destination())
        .expect("convolver connects");

    let rendered = render_context_offline(&graph, sample_rate, 1).expect("graph renders");

    assert_close(rendered.channel_data(0).unwrap()[0], 0.2 + 0.5 * 0.6);
    assert_close(rendered.channel_data(1).unwrap()[0], 0.4 + 0.5 * 0.8);
}

#[test]
fn dynamics_compressor_reduces_signal_above_threshold() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let compressor = graph.create_dynamics_compressor();
    compressor.threshold().set_value(-12.0).unwrap();
    compressor.ratio().set_value(4.0).unwrap();
    compressor.knee().set_value(0.0).unwrap();
    graph.connect(source, &compressor).expect("source connects");
    graph
        .connect(&compressor, graph.destination())
        .expect("compressor connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [kira::Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert!(out[0].left < 1.0);
    assert!(out[0].left > 0.25);
}

#[test]
fn dynamics_compressor_applies_fixed_lookahead_delay() {
    let sample_rate = 3_000;
    let mut graph = AudioContext::try_new_with_sample_rate(sample_rate).unwrap();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(sample_rate, 1, [1.0]),
    );
    let compressor = graph.create_dynamics_compressor();
    compressor.threshold().set_value(-24.0).unwrap();
    compressor.ratio().set_value(20.0).unwrap();
    compressor.knee().set_value(0.0).unwrap();
    compressor.attack().set_value(0.0).unwrap();
    compressor.release().set_value(0.0).unwrap();
    graph.connect(source, &compressor).expect("source connects");
    graph
        .connect(&compressor, graph.destination())
        .expect("compressor connects");

    let rendered = render_context_offline(&graph, sample_rate, 24).expect("graph renders");
    let samples = rendered.channel_data(0).expect("left channel");

    assert!(
        samples[..18]
            .iter()
            .all(|sample| sample.abs() <= f32::EPSILON)
    );
    let delayed_impulse = samples[18..]
        .iter()
        .copied()
        .find(|sample| *sample > f32::EPSILON)
        .expect("lookahead-delayed impulse should emerge after 6ms");
    assert!(
        delayed_impulse < 1.0,
        "delayed impulse should be compressed"
    );
}

#[test]
fn dynamics_compressor_soft_knee_gently_reduces_below_threshold() {
    let sample_rate = 3_000;
    let input = 10.0_f32.powf(-34.0 / 20.0);
    let mut graph = AudioContext::try_new_with_sample_rate(sample_rate).unwrap();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(input).unwrap();
        source
    };
    let compressor = graph.create_dynamics_compressor();
    compressor.threshold().set_value(-24.0).unwrap();
    compressor.knee().set_value(30.0).unwrap();
    compressor.ratio().set_value(12.0).unwrap();
    compressor.attack().set_value(0.0).unwrap();
    compressor.release().set_value(0.0).unwrap();
    graph.connect(source, &compressor).expect("source connects");
    graph
        .connect(&compressor, graph.destination())
        .expect("compressor connects");

    let rendered = render_context_offline(&graph, sample_rate, 20).expect("graph renders");
    let delayed_sample = rendered.channel_data(0).expect("left channel")[18];

    assert!(
        delayed_sample > input * 0.9,
        "soft knee below threshold should be a gentle reduction: input={input}, output={delayed_sample}"
    );
    assert!(
        delayed_sample < input,
        "soft knee inside the knee should still reduce slightly"
    );
}

#[test]
fn dynamics_compressor_attack_delays_gain_reduction() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let compressor = graph.create_dynamics_compressor();
    compressor.threshold().set_value(-24.0).unwrap();
    compressor.ratio().set_value(12.0).unwrap();
    compressor.knee().set_value(0.0).unwrap();
    compressor.attack().set_value(1.0).unwrap();
    graph.connect(source, &compressor).expect("source connects");
    graph
        .connect(&compressor, graph.destination())
        .expect("compressor connects");
    let rendered = render_context_offline(&graph, 4, 12).expect("graph renders");
    let samples = rendered.channel_data(0).expect("left channel");

    assert!(samples[0] > 0.5, "attack should delay initial compression");
    assert!(
        samples[11] < 0.25,
        "compressor should settle into reduction"
    );
}

#[test]
fn dynamics_compressor_params_are_sampled_at_k_rate() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let compressor = graph.create_dynamics_compressor();
    compressor.threshold().set_value_at_time(0.0, 0.0).unwrap();
    compressor
        .threshold()
        .linear_ramp_to_value_at_time(-60.0, 1.0)
        .unwrap();
    compressor.ratio().set_value(20.0).unwrap();
    compressor.knee().set_value(0.0).unwrap();
    graph.connect(source, &compressor).expect("source connects");
    graph
        .connect(&compressor, graph.destination())
        .expect("compressor connects");
    let rendered = render_context_offline(&graph, 4, 4).expect("graph renders");

    assert_eq!(rendered.channel_data(0), Some(&[1.0, 1.0, 1.0, 1.0][..]));
}

#[test]
fn dynamics_compressor_default_channel_config_clamps_to_stereo() {
    let mut graph = OfflineAudioContext::try_new(4, 20, 3_000).unwrap();
    let source = started_offline_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_channels(4, 1, vec![vec![0.25], vec![0.5], vec![0.75], vec![1.0]]),
    );
    let compressor = graph.create_dynamics_compressor();
    compressor.threshold().set_value(0.0).unwrap();
    compressor.ratio().set_value(1.0).unwrap();
    compressor.knee().set_value(0.0).unwrap();
    graph.connect(source, &compressor).expect("source connects");
    graph
        .connect(&compressor, graph.destination())
        .expect("compressor connects");

    let rendered = graph.start_rendering().expect("graph renders");

    assert_close(rendered.channel_data(0).unwrap()[18], 0.625);
    assert_close(rendered.channel_data(1).unwrap()[18], 1.0);
    assert_close(rendered.channel_data(2).unwrap()[18], 0.0);
    assert_close(rendered.channel_data(3).unwrap()[18], 0.0);
}

#[test]
fn panner_uses_position_for_left_right_balance() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let panner = graph.create_panner();
    panner.position_x().set_value(1.0).unwrap();
    panner.position_y().set_value(0.0).unwrap();
    panner.position_z().set_value(0.0).unwrap();
    graph.connect(source, &panner).expect("source connects");
    graph
        .connect(&panner, graph.destination())
        .expect("panner connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [kira::Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert!(out[0].right > out[0].left);
}

#[test]
fn panner_equalpower_crossfeeds_stereo_input() {
    let mut graph = AudioContext::new();
    let source =
        started_buffer_source_with_buffer(&mut graph, audio_buffer_from_stereo(4, 1, [1.0], [0.0]));
    let panner = graph.create_panner();
    panner.position_x().set_value(1.0).unwrap();
    panner.position_y().set_value(0.0).unwrap();
    panner.position_z().set_value(0.0).unwrap();
    graph.connect(source, &panner).expect("source connects");
    graph
        .connect(&panner, graph.destination())
        .expect("panner connects");

    let rendered = render_context_offline(&graph, 4, 1).expect("graph renders");

    assert!(rendered.channel_data(1).unwrap()[0] > 0.5);
}

#[test]
fn panner_equalpower_mono_input_does_not_duplicate_before_panning() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let panner = graph.create_panner();
    panner.position_x().set_value(1.0).unwrap();
    panner.position_y().set_value(0.0).unwrap();
    panner.position_z().set_value(0.0).unwrap();
    graph.connect(source, &panner).expect("source connects");
    graph
        .connect(&panner, graph.destination())
        .expect("panner connects");

    let rendered = render_context_offline(&graph, 4, 1).expect("graph renders");

    assert_close(rendered.channel_data(0).unwrap()[0], 0.38268343);
    assert_close(rendered.channel_data(1).unwrap()[0], 0.9238795);
}

#[test]
fn panner_default_channel_config_clamps_quad_input_to_stereo() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_channels(4, 1, [[0.2], [0.4], [0.6], [0.8]]),
    );
    let panner = graph.create_panner();
    panner.position_x().set_value(0.0).unwrap();
    panner.position_y().set_value(0.0).unwrap();
    panner.position_z().set_value(0.0).unwrap();
    graph.connect(source, &panner).expect("source connects");
    graph
        .connect(&panner, graph.destination())
        .expect("panner connects");

    let rendered = render_context_offline(&graph, 4, 1).expect("graph renders");

    assert_close(rendered.channel_data(0).unwrap()[0], 0.2 + 0.5 * 0.6);
    assert_close(rendered.channel_data(1).unwrap()[0], 0.4 + 0.5 * 0.8);
}

#[test]
fn audio_listener_exposes_position_orientation_and_up_vectors() {
    let graph = AudioContext::new();
    let listener = graph.listener();
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
fn audio_listener_position_offsets_panner_position() {
    let mut graph = AudioContext::new();
    let listener = graph.listener();
    listener.position_x().set_value(1.0).unwrap();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let panner = graph.create_panner();
    panner.position_x().set_value(1.0).unwrap();
    panner.position_y().set_value(0.0).unwrap();
    panner.position_z().set_value(0.0).unwrap();
    graph.connect(source, &panner).expect("source connects");
    graph
        .connect(&panner, graph.destination())
        .expect("panner connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.70710677);
    assert_close(out[0].right, 0.70710677);
}

#[test]
fn audio_listener_position_automation_affects_panner_over_time() {
    let mut graph = AudioContext::new();
    let listener = graph.listener();
    listener.position_x().set_value_at_time(0.0, 0.0).unwrap();
    listener.position_x().set_value_at_time(1.0, 0.25).unwrap();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let panner = graph.create_panner();
    panner.position_x().set_value(1.0).unwrap();
    panner.position_y().set_value(0.0).unwrap();
    panner.position_z().set_value(0.0).unwrap();
    graph.connect(source, &panner).expect("source connects");
    graph
        .connect(&panner, graph.destination())
        .expect("panner connects");

    let rendered = render_context_offline(&graph, 4, 2).expect("graph renders");
    let left = rendered.channel_data(0).expect("left channel");
    let right = rendered.channel_data(1).expect("right channel");

    assert!(right[0] > left[0]);
    assert_close(left[1], right[1]);
}

#[test]
fn audio_listener_forward_vector_rotates_panner_coordinates() {
    let mut graph = AudioContext::new();
    let listener = graph.listener();
    listener.forward_x().set_value(1.0).unwrap();
    listener.forward_y().set_value(0.0).unwrap();
    listener.forward_z().set_value(0.0).unwrap();
    listener.up_x().set_value(0.0).unwrap();
    listener.up_y().set_value(1.0).unwrap();
    listener.up_z().set_value(0.0).unwrap();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let panner = graph.create_panner();
    panner.position_x().set_value(1.0).unwrap();
    panner.position_y().set_value(0.0).unwrap();
    panner.position_z().set_value(0.0).unwrap();
    graph.connect(source, &panner).expect("source connects");
    graph
        .connect(&panner, graph.destination())
        .expect("panner connects");

    let rendered = render_context_offline(&graph, 4, 1).expect("graph renders");

    assert_close(rendered.channel_data(0).unwrap()[0], 0.70710677);
    assert_close(rendered.channel_data(1).unwrap()[0], 0.70710677);
}

#[test]
fn panner_distance_model_controls_attenuation() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let panner = graph.create_panner();
    panner.set_distance_model(DistanceModel::Linear);
    panner.try_ref_distance(1.0).unwrap();
    panner.try_max_distance(5.0).unwrap();
    panner.try_rolloff_factor(1.0).unwrap();
    panner.position_x().set_value(0.0).unwrap();
    panner.position_y().set_value(0.0).unwrap();
    panner.position_z().set_value(4.0).unwrap();
    graph.connect(source, &panner).expect("source connects");
    graph
        .connect(&panner, graph.destination())
        .expect("panner connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.17677669);
    assert_close(out[0].right, 0.17677669);
}

#[test]
fn panner_inverse_distance_uses_ref_distance() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let panner = graph.create_panner();
    panner.set_distance_model(DistanceModel::Inverse);
    panner.try_ref_distance(1.0).unwrap();
    panner.try_rolloff_factor(1.0).unwrap();
    panner.position_x().set_value(0.0).unwrap();
    panner.position_y().set_value(0.0).unwrap();
    panner.position_z().set_value(2.0).unwrap();
    graph.connect(source, &panner).expect("source connects");
    graph
        .connect(&panner, graph.destination())
        .expect("panner connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].left, 0.35355338);
    assert_close(out[0].right, 0.35355338);
}

#[test]
fn panner_cone_controls_directional_attenuation() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let panner = graph.create_panner();
    panner.try_cone_inner_angle(30.0).unwrap();
    panner.try_cone_outer_angle(60.0).unwrap();
    panner.try_cone_outer_gain(0.25).unwrap();
    panner.position_x().set_value(1.0).unwrap();
    panner.position_y().set_value(0.0).unwrap();
    panner.position_z().set_value(0.0).unwrap();
    panner.orientation_x().set_value(0.0).unwrap();
    panner.orientation_y().set_value(0.0).unwrap();
    panner.orientation_z().set_value(1.0).unwrap();
    graph.connect(source, &panner).expect("source connects");
    graph
        .connect(&panner, graph.destination())
        .expect("panner connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert_close(out[0].right, 0.23096988);
}

#[test]
fn panner_cone_uses_source_to_listener_direction() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let panner = graph.create_panner();
    panner.try_cone_inner_angle(30.0).unwrap();
    panner.try_cone_outer_angle(60.0).unwrap();
    panner.try_cone_outer_gain(0.25).unwrap();
    panner.position_x().set_value(1.0).unwrap();
    panner.position_y().set_value(0.0).unwrap();
    panner.position_z().set_value(0.0).unwrap();
    panner.orientation_x().set_value(-1.0).unwrap();
    panner.orientation_y().set_value(0.0).unwrap();
    panner.orientation_z().set_value(0.0).unwrap();
    graph.connect(source, &panner).expect("source connects");
    graph
        .connect(&panner, graph.destination())
        .expect("panner connects");

    let rendered = render_context_offline(&graph, 4, 1).expect("graph renders");

    assert_close(rendered.channel_data(1).unwrap()[0], 0.9238795);
}

#[test]
fn panner_orientation_accepts_audio_rate_modulation() {
    let mut graph = AudioContext::new();
    let source = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(1.0).unwrap();
        source
    };
    let panner = graph.create_panner();
    panner.try_cone_inner_angle(30.0).unwrap();
    panner.try_cone_outer_angle(60.0).unwrap();
    panner.try_cone_outer_gain(0.25).unwrap();
    panner.position_x().set_value(1.0).unwrap();
    panner.position_y().set_value(0.0).unwrap();
    panner.position_z().set_value(0.0).unwrap();
    panner.orientation_x().set_value(0.0).unwrap();
    panner.orientation_y().set_value(0.0).unwrap();
    panner.orientation_z().set_value(1.0).unwrap();
    let orientation_x = {
        let source = started_constant_source(&mut graph);
        source.offset().set_value(-10.0).unwrap();
        source
    };
    graph
        .connect_param(
            orientation_x,
            panner.param("orientationX").expect("orientationX param"),
        )
        .expect("orientationX connects");
    graph.connect(source, &panner).expect("source connects");
    graph
        .connect(&panner, graph.destination())
        .expect("panner connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 1];

    sound.process(&mut out, 0.25, &info);

    assert!(out[0].right > 0.45);
}

#[test]
fn analyser_reports_recent_peak_and_rms() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [1.0, -0.5, 0.0, 0.5]),
    );
    let analyser = graph.create_analyser();
    analyser.try_fft_size(32).unwrap();
    graph.connect(source, &analyser).expect("source connects");
    graph
        .connect(&analyser, graph.destination())
        .expect("analyser connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [kira::Frame::ZERO; 4];

    sound.process(&mut out, 0.25, &info);

    assert_close(analyser.peak(), 1.0);
    assert_close(analyser.rms(), (1.5_f32 / 4.0).sqrt());
}

#[test]
fn analyser_downmixes_all_input_channels_for_analysis() {
    let mut graph = OfflineAudioContext::try_new(4, 1, 3_000).unwrap();
    let source = started_offline_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_channels(4, 1, vec![vec![0.0], vec![0.0], vec![1.0], vec![1.0]]),
    );
    let analyser = graph.create_analyser();
    analyser.try_fft_size(32).unwrap();
    graph.connect(source, &analyser).expect("source connects");
    graph
        .connect(&analyser, graph.destination())
        .expect("analyser connects");

    let rendered = graph.start_rendering().expect("graph renders");

    assert_eq!(rendered.channel_data(2), Some(&[1.0][..]));
    assert_eq!(rendered.channel_data(3), Some(&[1.0][..]));
    assert_close(analyser.peak(), 0.5);
    assert_close(analyser.rms(), 0.5);
}

#[test]
fn analyser_uses_speaker_downmix_for_5_1_input() {
    let mut graph = OfflineAudioContext::try_new(6, 1, 3_000).unwrap();
    let source = started_offline_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_channels(
            6,
            1,
            vec![
                vec![0.0],
                vec![0.0],
                vec![1.0],
                vec![0.0],
                vec![0.0],
                vec![0.0],
            ],
        ),
    );
    let analyser = graph.create_analyser();
    analyser.try_fft_size(32).unwrap();
    graph.connect(source, &analyser).expect("source connects");
    graph
        .connect(&analyser, graph.destination())
        .expect("analyser connects");

    graph.start_rendering().expect("graph renders");

    assert_close(analyser.peak(), 1.0);
    assert_close(analyser.rms(), 1.0);
}

#[test]
fn analyser_exposes_configured_webaudio_properties() {
    let mut graph = AudioContext::new();
    let analyser = graph.create_analyser();
    analyser.try_fft_size(32).unwrap();
    analyser.try_min_decibels(-80.0).unwrap();
    analyser.try_max_decibels(-6.0).unwrap();
    analyser.try_smoothing_time_constant(0.25).unwrap();

    assert_eq!(analyser.fft_size_value(), 32);
    assert_eq!(analyser.frequency_bin_count(), 16);
    assert_close(analyser.min_decibels_value(), -80.0);
    assert_close(analyser.max_decibels_value(), -6.0);
    assert_close(analyser.smoothing_time_constant_value(), 0.25);
}

#[test]
fn analyser_exposes_webaudio_style_time_and_frequency_data() {
    let mut graph = AudioContext::new();
    let source = started_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(4, 4, [-1.0, 0.0, 1.0, 0.0]),
    );
    let analyser = graph.create_analyser();
    analyser.try_fft_size(32).unwrap();
    analyser.try_min_decibels(-100.0).unwrap();
    analyser.try_max_decibels(0.0).unwrap();
    analyser.try_smoothing_time_constant(0.0).unwrap();
    graph.connect(source, &analyser).expect("source connects");
    graph
        .connect(&analyser, graph.destination())
        .expect("analyser connects");
    let (mut sound, _) = graph
        .sound_data()
        .sample_rate(4)
        .into_sound()
        .expect("graph should build");
    let info = MockInfoBuilder::new().build();
    let mut out = [Frame::ZERO; 4];

    sound.process(&mut out, 0.25, &info);

    let mut expected_float_time = vec![-1.0, 0.0, 1.0, 0.0];
    expected_float_time.resize(32, 0.0);
    let mut expected_byte_time = vec![0, 128, 255, 128];
    expected_byte_time.resize(32, 128);
    assert_eq!(analyser.float_time_domain_data(), expected_float_time);
    assert_eq!(analyser.byte_time_domain_data(), expected_byte_time);
    assert_eq!(analyser.float_frequency_data().len(), 16);
    assert_eq!(analyser.byte_frequency_data().len(), 16);
    assert!(analyser.byte_frequency_data()[1] > 0);
}

#[test]
fn analyser_frequency_data_applies_webaudio_blackman_window() {
    let mut graph = OfflineAudioContext::try_new(1, 32, 3_000).unwrap();
    let source = started_offline_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(32, 32, [1.0; 32]),
    );
    let analyser = graph.create_analyser();
    analyser.try_fft_size(32).unwrap();
    analyser.try_min_decibels(-100.0).unwrap();
    analyser.try_max_decibels(0.0).unwrap();
    analyser.try_smoothing_time_constant(0.0).unwrap();
    graph.connect(source, &analyser).expect("source connects");
    graph
        .connect(&analyser, graph.destination())
        .expect("analyser connects");

    graph.start_rendering().expect("graph renders");

    let frequency = analyser.float_frequency_data();

    assert_close(frequency[0], 20.0 * 0.42_f32.log10());
}

#[test]
fn analyser_frequency_getters_do_not_resmooth_without_new_audio() {
    let mut graph = OfflineAudioContext::try_new(1, 32, 3_000).unwrap();
    let source = started_offline_buffer_source_with_buffer(
        &mut graph,
        audio_buffer_from_mono(32, 32, [1.0; 32]),
    );
    let analyser = graph.create_analyser();
    analyser.try_fft_size(32).unwrap();
    analyser.try_smoothing_time_constant(0.5).unwrap();
    graph.connect(source, &analyser).expect("source connects");
    graph
        .connect(&analyser, graph.destination())
        .expect("analyser connects");

    graph.start_rendering().expect("graph renders");

    let first = analyser.float_frequency_data();
    let second = analyser.float_frequency_data();
    let byte_after_float = analyser.byte_frequency_data();
    let byte_again = analyser.byte_frequency_data();

    assert_eq!(first, second);
    assert_eq!(byte_after_float, byte_again);
}
