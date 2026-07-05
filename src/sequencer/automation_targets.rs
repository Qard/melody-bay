#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SequencerParamTarget<'a> {
    label: Option<&'a str>,
    param: ParamKind,
    index: Option<usize>,
}

impl SequencerParamTarget<'_> {
    fn matches_label(&self, label: Option<&str>) -> bool {
        self.label
            .is_none_or(|target_label| label == Some(target_label))
    }
}

fn sequencer_param_target(target: &str) -> Option<SequencerParamTarget<'_>> {
    let (target, index) = match target.rsplit_once('#') {
        Some((name, index)) if !name.is_empty() => (name, Some(index.parse::<usize>().ok()?)),
        Some(_) => return None,
        None => (target, None),
    };
    let (label, name) = match target.split_once('.') {
        Some((label, name)) if !label.is_empty() && !name.is_empty() => (Some(label), name),
        Some(_) => return None,
        None => (None, target),
    };
    named_sequencer_param(name).map(|param| SequencerParamTarget {
        label,
        param,
        index,
    })
}

fn validate_automation_shape(
    track_id: &TrackId,
    automation: &TimedAutomationEvent,
) -> Result<(), SequencerValidationError> {
    match &automation.shape {
        AutomationShape::SetValue { value } | AutomationShape::LinearRamp { value } => {
            validate_automation_value(*value).map_err(|_| {
                SequencerValidationError::InvalidAutomationValue {
                    track_id: track_id.clone(),
                    target: automation.target.clone(),
                    value: *value,
                }
            })
        }
        AutomationShape::ValueCurve {
            values,
            duration_seconds,
        } => {
            if !duration_seconds.is_finite() || *duration_seconds <= 0.0 {
                return Err(SequencerValidationError::InvalidAutomationDuration {
                    track_id: track_id.clone(),
                    target: automation.target.clone(),
                    duration_seconds: *duration_seconds,
                });
            }
            for value in values {
                validate_automation_value(*value).map_err(|_| {
                    SequencerValidationError::InvalidAutomationValue {
                        track_id: track_id.clone(),
                        target: automation.target.clone(),
                        value: *value,
                    }
                })?;
            }
            Ok(())
        }
    }
}

fn named_sequencer_param(target: &str) -> Option<ParamKind> {
    match target {
        "gain" => Some(ParamKind::Gain),
        "frequency" => Some(ParamKind::Frequency),
        "detune" => Some(ParamKind::Detune),
        "offset" => Some(ParamKind::Offset),
        "playbackRate" | "playback_rate" => Some(ParamKind::PlaybackRate),
        "pan" => Some(ParamKind::Pan),
        "delayTime" | "delay_time" => Some(ParamKind::DelayTime),
        "q" | "Q" => Some(ParamKind::Q),
        "filterGain" | "filter_gain" => Some(ParamKind::FilterGain),
        "threshold" => Some(ParamKind::Threshold),
        "knee" => Some(ParamKind::Knee),
        "ratio" => Some(ParamKind::Ratio),
        "attack" => Some(ParamKind::Attack),
        "release" => Some(ParamKind::Release),
        _ => None,
    }
}

