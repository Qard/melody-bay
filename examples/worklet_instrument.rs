use std::{collections::HashMap, error::Error, thread, time::Duration};

use kira::{AudioManager, AudioManagerSettings, backend::cpal::CpalBackend};
use melody_bay::{
    AudioContext, AudioContextOptions, AudioWorkletNode, AudioWorkletNodeOptions,
    AudioWorkletParameterDescriptor, AudioWorkletProcessContext, AudioWorkletProcessor,
    AutomationRate, Waveform,
};

const SAMPLE_RATE: u32 = 48_000;

#[derive(Clone)]
struct FoldbackTremolo {
    phase: f32,
}

impl AudioWorkletProcessor for FoldbackTremolo {
    fn process(
        &mut self,
        inputs: &[Vec<Vec<f32>>],
        outputs: &mut [Vec<Vec<f32>>],
        context: AudioWorkletProcessContext,
    ) -> bool {
        let Some(output_port) = outputs.first_mut() else {
            return true;
        };
        let rate = context
            .processor_options
            .get("rate_hz")
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(5.0);
        let drive = context
            .processor_options
            .get("drive")
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(1.2);
        let stop_after = context
            .processor_options
            .get("stop_after")
            .and_then(|value| value.parse::<f64>().ok());

        for (channel_index, output_channel) in output_port.iter_mut().enumerate() {
            for (frame, output_sample) in output_channel.iter_mut().enumerate() {
                let input = inputs
                    .first()
                    .and_then(|port| port.get(channel_index).or_else(|| port.first()))
                    .and_then(|channel| channel.get(frame))
                    .copied()
                    .unwrap_or(0.0);
                let time = context.current_time + frame as f64 * context.sample_dt;
                let depth = context
                    .parameter_values
                    .get("depth")
                    .and_then(|values| values.get(frame))
                    .copied()
                    .or_else(|| context.parameters.get("depth").copied())
                    .unwrap_or(0.5);
                let mix = context
                    .parameter_values
                    .get("mix")
                    .and_then(|values| values.get(frame))
                    .copied()
                    .or_else(|| context.parameters.get("mix").copied())
                    .unwrap_or(0.6);
                let lfo = (time as f32 * std::f32::consts::TAU * rate + self.phase).sin();
                let tremolo = 1.0 - depth * 0.5 + depth * 0.5 * lfo;
                let folded = (input * drive).sin() * tremolo;
                *output_sample = input * (1.0 - mix) + folded * mix;
            }
        }
        self.phase = (self.phase + 0.01).rem_euclid(std::f32::consts::TAU);
        stop_after.is_none_or(|stop_after| context.current_time < stop_after)
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut manager = AudioManager::<CpalBackend>::new(AudioManagerSettings::default())?;
    let mut context = AudioContext::try_new_with_options(AudioContextOptions {
        sample_rate: Some(SAMPLE_RATE),
        ..Default::default()
    })?;
    build_worklet_patch(&mut context, 4.0)?;
    let handle = manager.play(context.sound_data().sample_rate(SAMPLE_RATE))?;
    println!("Playing AudioWorklet foldback tremolo with automated parameters...");
    thread::sleep(Duration::from_millis(4_200));
    handle.stop();
    Ok(())
}

fn build_worklet_patch(
    context: &mut AudioContext,
    stop_time: f64,
) -> Result<AudioWorkletNode, Box<dyn Error>> {
    let osc = context.create_oscillator();
    osc.set_type(Waveform::Sawtooth);
    osc.frequency().set_value(165.0)?;
    osc.frequency()
        .linear_ramp_to_value_at_time(220.0, stop_time)?;
    osc.try_start(0.0)?;
    osc.try_stop(stop_time)?;

    let mut parameter_data = HashMap::new();
    parameter_data.insert("depth".to_owned(), 0.65);
    parameter_data.insert("mix".to_owned(), 0.55);
    let mut processor_options = HashMap::new();
    processor_options.insert("rate_hz".to_owned(), "5.5".to_owned());
    processor_options.insert("drive".to_owned(), "1.35".to_owned());
    processor_options.insert("stop_after".to_owned(), stop_time.to_string());
    let worklet = context.try_create_audio_worklet_node(
        FoldbackTremolo { phase: 0.0 },
        AudioWorkletNodeOptions {
            number_of_inputs: 1,
            number_of_outputs: 1,
            output_channel_count: Some(vec![2]),
            parameter_descriptors: vec![
                AudioWorkletParameterDescriptor {
                    name: "depth".to_owned(),
                    default_value: 0.5,
                    min_value: 0.0,
                    max_value: 1.0,
                    automation_rate: AutomationRate::ARate,
                },
                AudioWorkletParameterDescriptor {
                    name: "mix".to_owned(),
                    default_value: 0.6,
                    min_value: 0.0,
                    max_value: 1.0,
                    automation_rate: AutomationRate::KRate,
                },
            ],
            parameter_data,
            processor_options,
        },
    )?;
    worklet
        .param("depth")
        .expect("depth parameter")
        .linear_ramp_to_value_at_time(0.15, stop_time)?;
    worklet
        .param("mix")
        .expect("mix parameter")
        .set_target_at_time(0.85, 1.25, 0.5)?;
    let output = context.create_gain();
    output.gain().set_value(0.18)?;
    context.connect(osc, &worklet)?;
    context.connect(&worklet, &output)?;
    context.connect(&output, context.destination())?;
    Ok(worklet)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worklet_processor_transforms_a_render_quantum() {
        let mut processor = FoldbackTremolo { phase: 0.0 };
        let inputs = vec![vec![vec![0.2; 128], vec![0.1; 128]]];
        let mut outputs = vec![vec![vec![0.0; 128], vec![0.0; 128]]];
        let mut parameters = HashMap::new();
        parameters.insert("depth".to_owned(), 0.5);
        parameters.insert("mix".to_owned(), 0.75);
        let mut parameter_values = HashMap::new();
        parameter_values.insert("depth".to_owned(), vec![0.5; 128]);
        let mut processor_options = HashMap::new();
        processor_options.insert("rate_hz".to_owned(), "5.5".to_owned());
        processor_options.insert("drive".to_owned(), "1.35".to_owned());
        processor_options.insert("stop_after".to_owned(), "1.0".to_owned());

        assert!(processor.process(
            &inputs,
            &mut outputs,
            AudioWorkletProcessContext {
                current_time: 0.0,
                sample_dt: 1.0 / SAMPLE_RATE as f64,
                parameters,
                parameter_values,
                processor_options,
            },
        ));
        let peak = outputs[0]
            .iter()
            .flatten()
            .map(|sample| sample.abs())
            .fold(0.0, f32::max);
        assert!(peak > 0.01, "processor rendered near silence");
        assert!(
            outputs[0][0]
                .iter()
                .any(|sample| (*sample - 0.2).abs() > 0.001)
        );
    }

    #[test]
    fn worklet_exposes_expected_parameters() {
        let mut context = AudioContext::try_new_with_sample_rate(SAMPLE_RATE).unwrap();
        let worklet = build_worklet_patch(&mut context, 0.5).unwrap();
        assert!(worklet.param("depth").is_some());
        assert!(worklet.param("mix").is_some());
    }
}
