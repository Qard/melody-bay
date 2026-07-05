use std::{error::Error, thread, time::Duration};

use kira::{AudioManager, AudioManagerSettings, backend::cpal::CpalBackend};
use melody_bay::{
    AudioContext, AudioContextOptions, ChannelMergerOptions, ChannelSplitterOptions, DelayOptions,
    GainOptions, PannerOptions, StereoPannerOptions, Waveform,
};

const SAMPLE_RATE: u32 = 48_000;

fn main() -> Result<(), Box<dyn Error>> {
    let mut manager = AudioManager::<CpalBackend>::new(AudioManagerSettings::default())?;
    let mut context = AudioContext::try_new_with_options(AudioContextOptions {
        sample_rate: Some(SAMPLE_RATE),
        ..Default::default()
    })?;
    build_spatial_scene(&mut context, 5.0)?;
    let handle = manager.play(context.sound_data().sample_rate(SAMPLE_RATE))?;
    println!("Playing moving panner, stereo sends, indexed routing, and disconnect helpers...");
    thread::sleep(Duration::from_millis(5_200));
    handle.stop();
    Ok(())
}

fn build_spatial_scene(context: &mut AudioContext, stop_time: f64) -> Result<(), Box<dyn Error>> {
    let listener = context.listener();
    listener.position_x().set_value_at_time(-0.6, 0.0)?;
    listener
        .position_x()
        .linear_ramp_to_value_at_time(0.6, stop_time)?;
    listener.forward_z().set_value(-1.0)?;

    let lead = context.create_oscillator();
    lead.set_type(Waveform::Triangle);
    lead.frequency().set_value(330.0)?;
    lead.try_start(0.0)?;
    lead.try_stop(stop_time)?;

    let source_amp = context.try_create_gain_with_options(GainOptions { gain: 0.12 })?;
    source_amp.gain().set_value_at_time(0.0, 0.0)?;
    source_amp.gain().linear_ramp_to_value_at_time(0.12, 0.12)?;
    source_amp
        .gain()
        .linear_ramp_to_value_at_time(0.0, stop_time)?;

    let panner = context.try_create_panner_with_options(PannerOptions {
        position_x: -1.2,
        position_z: 1.5,
        ..Default::default()
    })?;
    panner
        .position_x()
        .linear_ramp_to_value_at_time(1.2, stop_time)?;
    panner.orientation_z().set_value(-1.0)?;

    let stereo =
        context.try_create_stereo_panner_with_options(StereoPannerOptions { pan: -0.35 })?;
    stereo.pan().linear_ramp_to_value_at_time(0.35, stop_time)?;

    let splitter = context.try_create_channel_splitter_with_options(ChannelSplitterOptions {
        number_of_outputs: 2,
    })?;
    let merger = context.try_create_channel_merger_with_options(ChannelMergerOptions {
        number_of_inputs: 2,
    })?;
    let delay = context.try_create_delay_with_options(DelayOptions {
        max_delay_time: 0.6,
        delay_time: 0.16,
    })?;
    let send_gain = context.try_create_gain_with_options(GainOptions { gain: 0.18 })?;
    let return_gain = context.try_create_gain_with_options(GainOptions { gain: 0.22 })?;
    let output = context.try_create_gain_with_options(GainOptions { gain: 0.8 })?;

    let control = context.create_constant_source();
    control.offset().set_value(0.04)?;
    control.try_start(0.0)?;
    control.try_stop(stop_time)?;

    context.connect(lead, &source_amp)?;
    context.connect(&source_amp, &panner)?;
    context.connect(&panner, &stereo)?;
    context.connect(&stereo, &splitter)?;
    context.connect_with_indices(&splitter, 0, &merger, 0)?;
    context.connect_with_indices(&splitter, 1, &merger, 1)?;
    context.connect_with_indices(&splitter, 0, &send_gain, 0)?;
    context.connect(&send_gain, &delay)?;
    context.connect(&delay, &return_gain)?;
    context.connect(&return_gain, &merger)?;
    context.connect(&merger, &output)?;
    context.connect(&output, context.destination())?;
    context.connect_param_from_output(&splitter, 0, stereo.pan())?;
    context.connect_param(&control, delay.delay_time())?;

    context.disconnect_param_from_output(&splitter, 0, stereo.pan())?;
    context.disconnect_with_indices(&splitter, 0, &send_gain, 0)?;
    context.connect_with_indices(&splitter, 1, &send_gain, 0)?;
    context.disconnect_outputs(&return_gain)?;
    context.connect(&return_gain, &merger)?;
    context.disconnect_param_outputs(&control)?;
    context.connect_param(&control, delay.delay_time())?;

    println!("listener position: {:?}", listener.position_value());
    println!("routing mixer output: {:?}", context.node_info(&output)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use melody_bay::OfflineAudioContext;

    #[test]
    fn spatial_scene_renders_offline() {
        let mut context = OfflineAudioContext::try_new(2, SAMPLE_RATE as usize, SAMPLE_RATE)
            .expect("offline context");
        let osc = context.create_oscillator();
        osc.set_type(Waveform::Sine);
        osc.frequency().set_value(220.0).unwrap();
        osc.try_start(0.0).unwrap();
        osc.try_stop(0.5).unwrap();
        let panner = context
            .try_create_panner_with_options(PannerOptions {
                position_x: 0.5,
                position_z: 1.0,
                ..Default::default()
            })
            .unwrap();
        context.connect(osc, &panner).unwrap();
        context.connect(&panner, context.destination()).unwrap();
        let rendered = context.start_rendering().unwrap();
        let left = rendered.channel_data(0).unwrap();
        assert!(left.iter().any(|sample| sample.abs() > 0.001));
    }
}
