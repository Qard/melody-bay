# melody-bay examples

These examples are live sound demos. Each one is meant to be run by a human and
to make an audible result, while still compiling and smoke-testing under
`cargo test --examples`.

## Commands

| Example | Command | Notes |
| --- | --- | --- |
| `webaudio_synth_lab` | `cargo run --example webaudio_synth_lab` | Live WebAudio patch with synthesis, processing, routing, automation, and analyser stats. |
| `spatial_effects_mixer` | `cargo run --example spatial_effects_mixer` | Live listener/panner motion, indexed routing, sends, and disconnect helpers. |
| `sequenced_arrangement` | `cargo run --example sequenced_arrangement` | Live tempo-mapped arrangement with graph and sample instruments. |
| `worklet_instrument` | `cargo run --example worklet_instrument` | Live AudioWorklet processor with descriptors, options, and automated params. |
| `imported_song_player` | `cargo run --example imported_song_player` | Live MIDI/MOD/XM playback using bundled assets; accepts `midi\|mod\|xm <path>`. |
| `sound_data_bridge` | `cargo run --example sound_data_bridge` | Live Kira `SoundData` source routed through a WebAudio graph. |

## Coverage Matrix

| Capability | Primary examples |
| --- | --- |
| WebAudio graph nodes | `webaudio_synth_lab`, `spatial_effects_mixer`, `sound_data_bridge` |
| automation | `webaudio_synth_lab`, `sequenced_arrangement`, `worklet_instrument` |
| offline rendering | embedded smoke tests for `webaudio_synth_lab`, `spatial_effects_mixer`, `worklet_instrument`, and `sequenced_arrangement` |
| Kira SoundData | `sound_data_bridge`, `sequenced_arrangement`, `imported_song_player` |
| sequencing | `sequenced_arrangement`, `imported_song_player` |
| importers | `imported_song_player` |
| routing | `spatial_effects_mixer`, `webaudio_synth_lab`, `sound_data_bridge` |
| worklets | `worklet_instrument` |
| analysis | `webaudio_synth_lab`, `sound_data_bridge` |

## Requirements

All examples use CPAL and need an audio output device. `imported_song_player`
uses the bundled assets in `examples/assets` when no path is provided.
