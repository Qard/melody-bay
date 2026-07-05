# melody-bay

`melody-bay` is a Kira-compatible audio graph and sequencing crate with a
Rust-shaped WebAudio API. It is designed for games, tools, tests, and offline
audio workflows that want WebAudio-style nodes without a browser runtime.
The name is a nod to Melody Bay, the music puzzle location in Super Mario RPG.

The core graph models WebAudio concepts such as `AudioContext`, source nodes,
`AudioParam` automation, filters, panners, analysers, worklets, offline
rendering, and channel routing. The sequencer layer adds tempo-mapped and
timestamped note events that can render offline or play through Kira
`SoundData`.

## Quick Start

```rust
use melody_bay::{AudioContext, Waveform};

let mut context = AudioContext::new();
let osc = context.create_oscillator();
osc.set_type(Waveform::Sine);
osc.frequency().set_value(440.0)?;
osc.try_start(0.0)?;
osc.try_stop(1.0)?;

let gain = context.create_gain();
gain.gain().set_value(0.2)?;
context.connect(osc, &gain)?;
context.connect(&gain, context.destination())?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

For live playback, pass `context.sound_data()` to a Kira `AudioManager`. For
headless verification or asset generation, use `OfflineAudioContext` and
`start_rendering()`.

## Sequencing

Use `IndexedSequence` for tempo-mapped musical material and `TimedSequence` for
events that already have exact timestamps. Indexed sequences resolve to timed
sequences before rendering or playback.

```rust
use melody_bay::{AudioContext, IndexedSequence, IndexedTrack, Instrument, Note, TrackId};

let mut graph = AudioContext::new();
let source = graph.create_constant_source();
let gain = graph.create_gain();
graph.connect(source, &gain)?;
graph.connect(&gain, graph.destination())?;

let mut sequence = IndexedSequence::new(4);
sequence.tempo_at(0, 120.0);
sequence.add_track(
    TrackId::named("lead"),
    IndexedTrack::new(Instrument::graph(graph)).note(0, Note::from_midi(60), 4),
);

let timed = sequence.resolve();
let _sound_data = timed.try_sound_data()?.sample_rate(44_100);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Infallible sequencer helpers follow Kira's ergonomic `SoundData` style and skip
invalid automation. Use `validate`, `try_sound_data`, `try_render_offline`, and
`try_track_sound_data` when invalid tracks or automation should be reported.

## Importers

The crate includes MIDI, MOD, and XM import helpers:

```rust
let bytes = std::fs::read("song.mid")?;
let imported = melody_bay::import_midi(&bytes)?;
let sequence = imported.sequence.resolve();
# Ok::<(), Box<dyn std::error::Error>>(())
```

Importers preserve supported musical structure and report approximations or
unsupported effects as `ImportWarning` values. They are intentionally practical
instead of bit-exact emulators; use the warning list when building conversion
tools.

## WebAudio Compatibility

`melody-bay` follows WebAudio graph shape where it maps cleanly to Rust and
Kira. Browser-only integration points are intentionally unsupported: DOM media
elements, `MediaStream`, sink/device selection, `decodeAudioData`,
`ScriptProcessorNode`, `AudioRenderCapacity`, JavaScript module loading,
`MessagePort`, promises, and `EventTarget`.

Use `create_sound_data_source` for Kira media/source integration and
`create_audio_worklet_node` with an `AudioWorkletProcessor` implementation for
custom render-quantum processing. Panner nodes use equal-power spatialization;
HRTF panning is not currently implemented.

## Examples

Run examples from the crate root:

```sh
cargo run --example webaudio_synth_lab
cargo run --example spatial_effects_mixer
cargo run --example sequenced_arrangement
cargo run --example worklet_instrument
cargo run --example imported_song_player
cargo run --example sound_data_bridge
```

Live examples use CPAL and require an audio output device. Offline examples and
tests run headlessly. See `examples/README.md` for the curated live demo map.

## License

Licensed under the MIT license.
