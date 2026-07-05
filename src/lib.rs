//! Kira-compatible generated synthesis sound sources.
//!
//! This crate follows the WebAudio graph shape where it maps cleanly to Rust
//! and Kira. Browser-only integration points are intentionally unsupported:
//! DOM media elements, MediaStream, sink/device selection,
//! `decodeAudioData`, deprecated `ScriptProcessorNode`, `AudioRenderCapacity`
//! events, JavaScript module loading, MessagePort, promises, and EventTarget. Use
//! [`AudioContext::create_sound_data_source`] for Kira [`SoundData`] media/source
//! integration, and use Rust [`Result`] values plus source state handles for
//! errors and ended status. HRTF panning is also unsupported; panner nodes use
//! equal-power spatialization.
//!
//! Use [`AudioContext::create_audio_worklet_node`] with an
//! [`AudioWorkletProcessor`] implementation for custom render-quantum
//! processing.
//!
//! ```
//! # use melody_bay::{AudioContext, AudioWorkletProcessContext, AudioWorkletProcessor};
//! struct Passthrough;
//!
//! impl AudioWorkletProcessor for Passthrough {
//!     fn process(
//!         &mut self,
//!         inputs: &[Vec<Vec<f32>>],
//!         outputs: &mut [Vec<Vec<f32>>],
//!         _context: AudioWorkletProcessContext,
//!     ) -> bool {
//!         if let (Some(input), Some(output)) = (inputs.first(), outputs.first_mut()) {
//!             for (output_channel, input_channel) in output.iter_mut().zip(input) {
//!                 output_channel.copy_from_slice(input_channel);
//!             }
//!         }
//!         true
//!     }
//! }
//!
//! let mut context = AudioContext::new();
//! let _node = context.create_audio_worklet_node(Passthrough);
//! ```
//!
//! ```compile_fail
//! let _channel = melody_bay::Channel::main(melody_bay::AudioContext::new());
//! ```
//!
//! Author new musical material with [`IndexedSequence`] when events should sit
//! on a tempo-mapped grid, or [`TimedSequence`] when events already have exact
//! timestamps. Indexed sequences resolve into timed sequences before playback.
//! Automation targets use WebAudio-style parameter names such as `"gain"` or
//! `"playback_rate"`. Use [`AudioContext::label_node`] and `"label.param"` to
//! target a specific graph node, for example `"output.gain"`. Add `#n` to
//! target the nth matching parameter in graph node order, for example
//! `"gain#1"`. Infallible render/playback helpers follow Kira's ergonomic
//! `SoundData` style and ignore invalid sequencer automation. Use
//! [`TimedSequence::validate`] or [`TimedSequence::try_sound_data`] when
//! automation should be checked before rendering.
//!
//! ```
//! # use kira::sound::SoundData;
//! # use melody_bay::{
//! #     AudioContext, IndexedSequence, IndexedTrack, Instrument, Note, TrackId, Velocity,
//! # };
//! let mut graph = AudioContext::new();
//! let source = graph.create_constant_source();
//! let gain = graph.create_gain();
//! graph.connect(source, &gain).unwrap();
//! graph.connect(&gain, graph.destination()).unwrap();
//!
//! let mut sequence = IndexedSequence::new(4);
//! sequence.tempo_at(0, 120.0);
//! sequence.add_track(
//!     TrackId::named("lead"),
//!     IndexedTrack::new(Instrument::graph(graph))
//!         .note_with_velocity(0, Note::from_midi(60), 4, Velocity::new(0.8))
//!         .linear_ramp_to_value_at_index(4, "gain", 0.25),
//! );
//!
//! let timed = sequence.resolve();
//! let _sound_data = timed.try_sound_data().unwrap().sample_rate(44_100);
//! ```
//!
//! ```compile_fail
//! let _id = melody_bay::EventId(1);
//! ```
//!
//! ```compile_fail
//! let _time = melody_bay::SequenceTime::seconds(0.0);
//! ```
//!
//! ```compile_fail
//! let _stem = melody_bay::StemId::named("lead");
//! ```
//!
//! ```compile_fail
//! let _envelope = melody_bay::Envelope::Adsr(melody_bay::Adsr::default());
//! ```
//!
//! ```compile_fail
//! let _curve = melody_bay::AutomationCurve::constant(1.0);
//! ```
//!
//! ```compile_fail
//! let _param = melody_bay::AudioParam::new(0.5);
//! ```
//!
//! ```compile_fail
//! let _filter = melody_bay::BiquadFilterNode::new(melody_bay::BiquadFilterType::Lowpass);
//! ```
//!
//! ```compile_fail
//! let _delay = melody_bay::DelayNode::new(0.25);
//! ```
//!
//! ```compile_fail
//! let _instrument = melody_bay::InstrumentGraph::constant(1.0);
//! ```
//!
//! ```compile_fail
//! let _oscillator = melody_bay::Oscillator::new(melody_bay::Waveform::Sine, 440.0);
//! ```
//!
//! ```compile_fail
//! let _gain = melody_bay::Gain { amount: 1.0 };
//! ```
//!
//! ```compile_fail
//! let _pan = melody_bay::Pan { amount: 0.0 };
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _compressor = context.dynamics_compressor();
//! ```
//!
//! ```compile_fail
//! let _graph = melody_bay::Graph::new();
//! ```
//!
//! ```compile_fail
//! let _data: Option<melody_bay::GraphSoundData> = None;
//! ```
//!
//! ```compile_fail
//! let _handle: Option<melody_bay::GraphSoundHandle> = None;
//! ```
//!
//! ```compile_fail
//! let live = melody_bay::AudioContext::new();
//! let mut offline = melody_bay::OfflineAudioContext::try_new(2, 128, 48_000).unwrap();
//! let _buffer = offline.render_graph(&live);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _gain = context.create_gain().set_gain(0.5);
//! ```
//!
//! ```compile_fail
//! let context = melody_bay::AudioContext::new();
//! let _listener = context.listener().position(1.0, 2.0, 3.0);
//! ```
//!
//! ```compile_fail
//! let context = melody_bay::AudioContext::new();
//! let _listener = context.listener().forward(0.0, 0.0, -1.0);
//! ```
//!
//! ```compile_fail
//! let context = melody_bay::AudioContext::new();
//! let _listener = context.listener().up(0.0, 1.0, 0.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _gain = context
//!     .create_gain()
//!     .gain_param(melody_bay::AudioParam::new(0.5));
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _param = context.create_gain().gain_param_id();
//! ```
//!
//! Use [`GainNode::gain`] to access the live node-owned
//! [`AudioParamHandle`] instead of raw parameter ids.
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _id = context.create_gain().gain().id();
//! ```
//!
//! ```compile_fail
//! let _id: Option<melody_bay::ParamId> = None;
//! ```
//!
//! ```compile_fail
//! let _id: Option<melody_bay::NodeId> = None;
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _id = melody_bay::NodeId::from(context.create_gain());
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! context.set_sample_rate(48_000);
//! ```
//!
//! ```compile_fail
//! let _context = melody_bay::AudioContext::new_with_sample_rate(48_000);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _source = context.create_constant_source().set_offset(1.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _pan = context.create_stereo_panner().set_pan(0.5);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _source = context
//!     .create_buffer_source()
//!     .set_playback_rate(2.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _delay = context.create_delay().set_delay_time(0.25);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _osc = context.create_oscillator().set_detune(1200.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _source = context.create_buffer_source().set_detune(1200.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _filter = context.create_biquad_filter().set_detune(1200.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _compressor = context.create_dynamics_compressor().set_threshold(-24.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _compressor = context.create_dynamics_compressor().set_knee(0.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _compressor = context.create_dynamics_compressor().set_ratio(12.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _compressor = context.create_dynamics_compressor().set_attack(0.003);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _compressor = context.create_dynamics_compressor().set_release(0.25);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _filter = context.create_biquad_filter().set_frequency(1_000.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _filter = context.create_biquad_filter().set_q(0.707);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _filter = context.create_biquad_filter().set_gain(3.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _osc = context.create_oscillator().set_frequency(440.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _panner = context.create_panner().position(1.0, 2.0, 3.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _panner = context.create_panner().orientation(0.0, 0.0, -1.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _analyser = context.create_analyser().fft_size(2048);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _analyser = context.create_analyser().min_decibels(-100.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _analyser = context.create_analyser().max_decibels(-30.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _analyser = context.create_analyser().smoothing_time_constant(0.5);
//! ```
//!
//! ```
//! let context = melody_bay::AudioContext::new();
//! let _buffer = context.create_buffer(2, 128, 44_100).unwrap();
//! ```
//!
//! ```
//! let context = melody_bay::OfflineAudioContext::try_new(2, 128, 44_100).unwrap();
//! let _buffer = context.create_buffer(2, 128, 44_100).unwrap();
//! ```
//!
//! ```
//! let context = melody_bay::AudioContext::new();
//! let _wave = context
//!     .create_periodic_wave([0.0, 1.0], [0.0, 0.5])
//!     .unwrap();
//! ```
//!
//! ```
//! let context = melody_bay::OfflineAudioContext::try_new(2, 128, 44_100).unwrap();
//! let _wave = context
//!     .create_periodic_wave([0.0, 1.0], [0.0, 0.5])
//!     .unwrap();
//! ```
//!
//! ```
//! let mut context = melody_bay::AudioContext::new();
//! let _filter = context.create_iir_filter([1.0], [1.0]).unwrap();
//! ```
//!
//! ```compile_fail
//! let _wave = melody_bay::PeriodicWave::new([0.0, 1.0], [0.0, 0.5]);
//! ```
//!
//! ```
//! let mut context = melody_bay::OfflineAudioContext::try_new(2, 128, 44_100).unwrap();
//! let _filter = context.create_iir_filter([1.0], [1.0]).unwrap();
//! ```
//!
//! ```compile_fail
//! let _context = melody_bay::OfflineAudioContext::new(2, 128, 44_100);
//! ```
//!
//! ```compile_fail
//! let context = melody_bay::AudioContext::new();
//! let _rendered = context.render_offline(44_100, 128);
//! ```
//!
//! ```compile_fail
//! let context = melody_bay::AudioContext::new();
//! let _rendered = context.render_offline_channels(44_100, 128, 2);
//! ```
//!
//! ```compile_fail
//! let context = melody_bay::AudioContext::new();
//! let _rendered = context.render_offline_seconds(44_100, 1.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _splitter = context.create_channel_splitter(2);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _merger = context.create_channel_merger(2);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::OfflineAudioContext::try_new(2, 128, 44_100).unwrap();
//! let _splitter = context.create_channel_splitter(2);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::OfflineAudioContext::try_new(2, 128, 44_100).unwrap();
//! let _merger = context.create_channel_merger(2);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _shaper = context.create_wave_shaper().curve([-1.0, 0.0, 1.0]);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let buffer = melody_bay::AudioBuffer::from_mono(44_100, 1, [1.0]);
//! let _source = context.create_buffer_source().buffer(buffer);
//! ```
//!
//! ```compile_fail
//! let _buffer = melody_bay::AudioBuffer::from_frames(44_100, &[]);
//! ```
//!
//! ```compile_fail
//! let _buffer = melody_bay::AudioBuffer::from_mono(44_100, 1, [1.0]);
//! ```
//!
//! ```compile_fail
//! let _buffer = melody_bay::AudioBuffer::from_stereo(44_100, 1, [1.0], [1.0]);
//! ```
//!
//! ```compile_fail
//! let _buffer = melody_bay::AudioBuffer::from_channels(44_100, 1, [[1.0]]);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _delay = context.create_delay().max_delay_time(0.5);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _panner = context.create_panner().cone_inner_angle(30.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _panner = context.create_panner().cone_outer_angle(60.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _panner = context.create_panner().cone_outer_gain(0.25);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _panner = context.create_panner().ref_distance(1.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _panner = context.create_panner().max_distance(10.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _panner = context.create_panner().rolloff_factor(1.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _source = context.create_buffer_source().loop_range(0.0, 1.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _osc = context.create_oscillator().start(0.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _osc = context.create_oscillator().stop(1.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _source = context.create_constant_source().start(0.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _source = context.create_constant_source().stop(1.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _source = context.create_buffer_source().start(0.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _source = context.create_buffer_source().start_with_offset(0.0, 0.25);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _source = context
//!     .create_buffer_source()
//!     .start_with_offset_and_duration(0.0, 0.25, 1.0);
//! ```
//!
//! ```compile_fail
//! let mut context = melody_bay::AudioContext::new();
//! let _source = context.create_buffer_source().stop(1.0);
//! ```

#![forbid(unsafe_op_in_unsafe_fn)]

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, TAU};
use std::fmt;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use kira::sound::{Sound, SoundData};
use kira::{
    Frame,
    info::{Info, MockInfoBuilder},
};

const RENDER_QUANTUM_SIZE_USIZE: usize = 128;
const RENDER_QUANTUM_SIZE: f64 = RENDER_QUANTUM_SIZE_USIZE as f64;
const DETUNE_NOMINAL_LIMIT: f32 = 153_600.0;
const BIQUAD_GAIN_MAX: f32 = 1541.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct NodeId(usize);

include!("core/private.rs");
include!("import/module_aliases.rs");
include!("webaudio/node.rs");
include!("webaudio/buffer.rs");
include!("webaudio/param/kind.rs");
include!("webaudio/context.rs");
include!("sequencer/automation_targets.rs");
include!("webaudio/channel_config.rs");
include!("render/graph_state.rs");
include!("webaudio/nodes/sources.rs");
include!("webaudio/nodes/gain_param.rs");
include!("webaudio/nodes/processing.rs");
include!("webaudio/nodes/panner.rs");
include!("webaudio/nodes/channel_private.rs");
include!("render/compiled.rs");
include!("sequencer/models.rs");
include!("import/api.rs");
include!("import/midi.rs");
include!("import/tracker/types.rs");
include!("import/tracker/mod_tracker.rs");
include!("import/tracker/xm.rs");
include!("import/tracker/effects.rs");
include!("import/bytes.rs");
include!("core/waveform.rs");
include!("webaudio/param/timeline.rs");
include!("webaudio/types.rs");
include!("render/sample_voice.rs");
include!("render/dsp.rs");
