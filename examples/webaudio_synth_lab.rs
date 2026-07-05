use std::{error::Error, thread, time::Duration};

use kira::{
    AudioManager, AudioManagerSettings, Frame, backend::cpal::CpalBackend, info::MockInfoBuilder,
    sound::SoundData,
};
use melody_bay::{
    AnalyserNode, AnalyserOptions, AudioBuffer, AudioBufferSourceOptions, AudioContext,
    AudioContextOptions, BiquadFilterOptions, BiquadFilterType, ChannelMergerOptions,
    ChannelSplitterOptions, ConstantSourceOptions, ConvolverOptions, DelayOptions,
    DynamicsCompressorOptions, GainOptions, IirFilterOptions, OscillatorOptions, OscillatorType,
    Oversample, PannerOptions, PeriodicWaveOptions, StereoPannerOptions, WaveShaperOptions,
    Waveform,
};

const SAMPLE_RATE: u32 = 48_000;
const DURATION: f64 = 6.0;

fn main() -> Result<(), Box<dyn Error>> {
    let mut manager = AudioManager::<CpalBackend>::new(AudioManagerSettings::default())?;
    let mut context = AudioContext::try_new_with_options(AudioContextOptions {
        sample_rate: Some(SAMPLE_RATE),
        ..Default::default()
    })?;
    let analyser = build_synth_lab(&mut context, DURATION)?;

    println!(
        "destination: {:?}",
        context.node_info(context.destination())?
    );
    println!("analyser: {:?}", context.node_info(&analyser)?);
    let stats = preflight(&context, 2.0)?;
    println!(
        "preflight peak={:.3} rms={:.3} max_step={:.4}",
        stats.peak, stats.rms, stats.max_step
    );

    let handle = manager.play(context.sound_data().sample_rate(SAMPLE_RATE))?;
    for tick in 0..12 {
        thread::sleep(Duration::from_millis(500));
        let mut bins = vec![0.0; analyser.frequency_bin_count().min(10)];
        analyser.get_float_frequency_data(&mut bins);
        let brightest = bins.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        println!(
            "t={:.1}s peak={:.3} rms={:.3} brightest_bin_db={:.1}",
            (tick + 1) as f32 * 0.5,
            analyser.peak(),
            analyser.rms(),
            brightest
        );
    }
    handle.stop();
    Ok(())
}

fn build_synth_lab(
    context: &mut AudioContext,
    stop_time: f64,
) -> Result<AnalyserNode, Box<dyn Error>> {
    let custom_wave = context.try_create_periodic_wave_with_options(
        [0.0, 0.0, 0.22, 0.12],
        [0.0, 1.0, 0.35, 0.18],
        PeriodicWaveOptions {
            disable_normalization: false,
        },
    )?;
    let carrier = context.try_create_oscillator_with_options(OscillatorOptions {
        oscillator_type: OscillatorType::Custom(custom_wave),
        frequency: 110.0,
        detune: -8.0,
    })?;
    carrier
        .frequency()
        .linear_ramp_to_value_at_time(146.83, 3.0)?;
    carrier.try_start(0.0)?;
    carrier.try_stop(stop_time)?;

    let overtone = context.try_create_oscillator_with_options(OscillatorOptions {
        oscillator_type: OscillatorType::Basic(Waveform::Triangle),
        frequency: 220.0,
        detune: 4.0,
    })?;
    overtone.try_start(0.0)?;
    overtone.try_stop(stop_time)?;

    let buffer = AudioBuffer::try_from_mono(
        SAMPLE_RATE,
        SAMPLE_RATE as usize / 2,
        (0..SAMPLE_RATE / 2).map(|i| {
            let t = i as f32 / SAMPLE_RATE as f32;
            let burst = (1.0 - t * 2.0).max(0.0);
            (t * std::f32::consts::TAU * 880.0).sin() * burst * 0.18
        }),
    )?;
    let buffer_source =
        context.try_create_buffer_source_with_options(AudioBufferSourceOptions {
            buffer: Some(buffer),
            playback_rate: 0.85,
            detune: 0.0,
            looping: false,
            loop_start: 0.0,
            loop_end: 0.5,
        })?;
    buffer_source.try_start(2.0)?;
    buffer_source.try_stop(3.2)?;

    let lfo =
        context.try_create_constant_source_with_options(ConstantSourceOptions { offset: 0.012 })?;
    lfo.try_start(0.0)?;
    lfo.try_stop(stop_time)?;

    let carrier_gain = context.try_create_gain_with_options(GainOptions { gain: 0.0 })?;
    carrier_gain.gain().set_value_at_time(0.0, 0.0)?;
    carrier_gain
        .gain()
        .linear_ramp_to_value_at_time(0.11, 0.18)?;
    carrier_gain
        .gain()
        .set_value_curve_at_time([0.11, 0.08, 0.13, 0.09, 0.12], 0.4, 2.8)?;
    carrier_gain.gain().set_target_at_time(0.07, 3.4, 0.55)?;
    carrier_gain
        .gain()
        .cancel_and_hold_at_time(stop_time - 0.35)?;
    carrier_gain
        .gain()
        .linear_ramp_to_value_at_time(0.0, stop_time)?;

    let burst_gain = context.try_create_gain_with_options(GainOptions { gain: 0.12 })?;
    let delay = context.try_create_delay_with_options(DelayOptions {
        max_delay_time: 0.75,
        delay_time: 0.08,
    })?;
    let biquad = context.try_create_biquad_filter_with_options(BiquadFilterOptions {
        filter_type: BiquadFilterType::Lowpass,
        frequency: 1_200.0,
        q: 0.9,
        ..Default::default()
    })?;
    biquad
        .frequency()
        .linear_ramp_to_value_at_time(2_400.0, 2.5)?;
    biquad.frequency().set_target_at_time(700.0, 3.5, 0.8)?;

    let iir = context.try_create_iir_filter_with_options(IirFilterOptions {
        feedforward: vec![0.55, 0.35],
        feedback: vec![1.0, -0.18],
    })?;
    let shaper = context.try_create_wave_shaper_with_options(WaveShaperOptions {
        curve: Some(vec![-1.0, -0.35, 0.0, 0.35, 1.0]),
        oversample: Oversample::TwoX,
    })?;
    let stereo =
        context.try_create_stereo_panner_with_options(StereoPannerOptions { pan: -0.15 })?;
    stereo.pan().linear_ramp_to_value_at_time(0.25, stop_time)?;
    let panner = context.try_create_panner_with_options(PannerOptions {
        position_x: 0.8,
        position_z: 1.5,
        ..Default::default()
    })?;
    let compressor =
        context.try_create_dynamics_compressor_with_options(DynamicsCompressorOptions {
            threshold: -20.0,
            knee: 18.0,
            ratio: 2.5,
            attack: 0.01,
            release: 0.22,
        })?;
    let convolver = context.try_create_convolver_with_options(ConvolverOptions {
        buffer: Some(AudioBuffer::try_from_mono(
            SAMPLE_RATE,
            8,
            [0.55, 0.28, 0.15, 0.08, 0.04, 0.02, 0.01, 0.005],
        )?),
        disable_normalization: true,
    })?;
    let analyser = context.try_create_analyser_with_options(AnalyserOptions {
        fft_size: 1024,
        min_decibels: -100.0,
        max_decibels: -12.0,
        smoothing_time_constant: 0.35,
    })?;
    let splitter = context.try_create_channel_splitter_with_options(ChannelSplitterOptions {
        number_of_outputs: 2,
    })?;
    let merger = context.try_create_channel_merger_with_options(ChannelMergerOptions {
        number_of_inputs: 2,
    })?;
    let output = context.try_create_gain_with_options(GainOptions { gain: 0.82 })?;

    context.connect(carrier, &carrier_gain)?;
    context.connect(overtone, &carrier_gain)?;
    context.connect(buffer_source, &burst_gain)?;
    context.connect(&carrier_gain, &delay)?;
    context.connect(&delay, &biquad)?;
    context.connect_param(lfo, delay.delay_time())?;
    context.connect(&biquad, &iir)?;
    context.connect(&iir, &shaper)?;
    context.connect(&shaper, &stereo)?;
    context.connect(&stereo, &panner)?;
    context.connect(&burst_gain, &splitter)?;
    context.connect_with_indices(&splitter, 0, &merger, 0)?;
    context.connect_with_indices(&splitter, 1, &merger, 1)?;
    context.connect(&panner, &compressor)?;
    context.connect(&merger, &compressor)?;
    context.connect(&compressor, &convolver)?;
    context.connect(&convolver, &output)?;
    context.connect(&output, &analyser)?;
    context.connect(&analyser, context.destination())?;

    println!("output node info: {:?}", context.node_info(&output)?);
    Ok(analyser)
}

#[derive(Clone, Copy, Debug)]
struct AudioStats {
    peak: f32,
    rms: f32,
    max_step: f32,
}

fn preflight(context: &AudioContext, seconds: f64) -> Result<AudioStats, Box<dyn Error>> {
    let (mut sound, _) = context.sound_data().sample_rate(SAMPLE_RATE).into_sound()?;
    let info = MockInfoBuilder::new().build();
    let mut out = vec![Frame::ZERO; (seconds * SAMPLE_RATE as f64) as usize];
    sound.process(&mut out, 1.0 / SAMPLE_RATE as f64, &info);
    Ok(audio_stats(&out))
}

fn audio_stats(out: &[Frame]) -> AudioStats {
    let peak = out
        .iter()
        .map(|frame| frame.left.abs().max(frame.right.abs()))
        .fold(0.0, f32::max);
    let rms = (out
        .iter()
        .map(|frame| {
            let mono = (frame.left + frame.right) * 0.5;
            mono * mono
        })
        .sum::<f32>()
        / out.len().max(1) as f32)
        .sqrt();
    let max_step = out
        .windows(2)
        .map(|frames| {
            (frames[1].left - frames[0].left)
                .abs()
                .max((frames[1].right - frames[0].right).abs())
        })
        .fold(0.0, f32::max);
    AudioStats {
        peak,
        rms,
        max_step,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use melody_bay::OfflineAudioContext;

    #[test]
    fn synth_lab_live_graph_stays_listenable() {
        let mut context = AudioContext::try_new_with_sample_rate(SAMPLE_RATE).unwrap();
        build_synth_lab(&mut context, DURATION).unwrap();
        let stats = preflight(&context, 4.0).unwrap();
        assert!(stats.peak > 0.02, "rendered near silence: {stats:?}");
        assert!(stats.peak < 0.55, "peak too hot: {stats:?}");
        assert!(stats.rms < 0.18, "rms too dense: {stats:?}");
        assert!(stats.max_step < 0.08, "click-like sample jump: {stats:?}");
    }

    #[test]
    fn synth_lab_offline_context_renders() {
        let mut context = OfflineAudioContext::try_new(2, SAMPLE_RATE as usize, SAMPLE_RATE)
            .expect("offline context");
        let osc = context.create_oscillator();
        osc.set_type(Waveform::Sine);
        osc.frequency().set_value(220.0).unwrap();
        osc.try_start(0.0).unwrap();
        osc.try_stop(0.25).unwrap();
        let gain = context.create_gain();
        gain.gain().set_value(0.1).unwrap();
        context.connect(osc, &gain).unwrap();
        context.connect(&gain, context.destination()).unwrap();
        let rendered = context.start_rendering().unwrap();
        assert!(
            rendered
                .channel_data(0)
                .unwrap()
                .iter()
                .any(|sample| sample.abs() > 0.001)
        );
    }
}
