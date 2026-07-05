#[derive(Debug, Clone, PartialEq)]
pub struct ParamTimeline {
    default_value: f32,
    current_value: f32,
    min_value: f32,
    max_value: f32,
    automation_rate: AutomationRate,
    fixed_automation_rate: Option<AutomationRate>,
    time_domain: ParamTimeDomain,
    events: Vec<AudioParamEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationRate {
    ARate,
    KRate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParamTimeDomain {
    Local,
    Global,
}

impl ParamTimeline {
    #[must_use]
    pub fn new(default_value: f32) -> Self {
        Self {
            default_value,
            current_value: default_value,
            min_value: f32::NEG_INFINITY,
            max_value: f32::INFINITY,
            automation_rate: AutomationRate::ARate,
            fixed_automation_rate: None,
            time_domain: ParamTimeDomain::Local,
            events: Vec::new(),
        }
    }

    #[must_use]
    pub fn default_value(&self) -> f32 {
        self.default_value
    }

    #[must_use]
    pub fn min_value(&self) -> f32 {
        self.min_value
    }

    #[must_use]
    pub fn max_value(&self) -> f32 {
        self.max_value
    }

    #[must_use]
    pub fn automation_rate(&self) -> AutomationRate {
        self.automation_rate
    }

    #[must_use]
    pub fn with_nominal_range(mut self, min_value: f32, max_value: f32) -> Self {
        self.min_value = min_value.min(max_value);
        self.max_value = max_value.max(min_value);
        self.default_value = self.clamp_value(self.default_value);
        self.current_value = self.clamp_value(self.current_value);
        self
    }

    #[must_use]
    pub fn with_automation_rate(mut self, automation_rate: AutomationRate) -> Self {
        self.automation_rate = automation_rate;
        self.fixed_automation_rate = Some(automation_rate);
        self
    }

    fn with_time_domain(mut self, time_domain: ParamTimeDomain) -> Self {
        self.time_domain = time_domain;
        self
    }

    pub fn try_set_automation_rate(
        &mut self,
        automation_rate: AutomationRate,
    ) -> Result<(), GraphError> {
        if self
            .fixed_automation_rate
            .is_some_and(|fixed| fixed != automation_rate)
        {
            return Err(GraphError::InvalidAutomationRate);
        }
        self.automation_rate = automation_rate;
        Ok(())
    }

    #[must_use]
    pub fn value(&self) -> f32 {
        self.value_at(0.0)
    }

    #[must_use]
    pub fn set_value_at_time(mut self, value: f32, time: f64) -> Self {
        self.push_event(AudioParamEvent::SetValue {
            time: time.max(0.0),
            value,
        });
        self
    }

    #[must_use]
    pub fn linear_ramp_to_value_at_time(mut self, value: f32, end_time: f64) -> Self {
        self.push_event(AudioParamEvent::LinearRamp {
            end_time: end_time.max(0.0),
            value,
        });
        self
    }

    #[must_use]
    pub fn exponential_ramp_to_value_at_time(mut self, value: f32, end_time: f64) -> Self {
        self.push_event(AudioParamEvent::ExponentialRamp {
            end_time: end_time.max(0.0),
            value,
        });
        self
    }

    pub fn try_exponential_ramp_to_value_at_time(
        mut self,
        value: f32,
        end_time: f64,
    ) -> Result<Self, GraphError> {
        let end_time = end_time.max(0.0);
        if !value.is_finite() || value == 0.0 {
            return Err(GraphError::InvalidAutomationValue);
        }
        self.push_event(AudioParamEvent::ExponentialRamp { end_time, value });
        Ok(self)
    }

    #[must_use]
    pub fn set_target_at_time(mut self, target: f32, start_time: f64, time_constant: f64) -> Self {
        self.push_event(AudioParamEvent::SetTarget {
            start_time: start_time.max(0.0),
            target,
            time_constant: time_constant.max(f64::MIN_POSITIVE),
        });
        self
    }

    #[must_use]
    pub fn set_value_curve_at_time(
        mut self,
        values: impl IntoIterator<Item = f32>,
        start_time: f64,
        duration: f64,
    ) -> Self {
        self.push_event(AudioParamEvent::ValueCurve {
            start_time: start_time.max(0.0),
            duration: duration.max(f64::MIN_POSITIVE),
            active_until: None,
            values: values.into_iter().collect(),
        });
        self
    }

    #[must_use]
    pub fn cancel_scheduled_values(mut self, cancel_time: f64) -> Self {
        let cancel_time = cancel_time.max(0.0);
        self.events
            .retain(|event| event_is_before_cancel_scheduled(event, cancel_time));
        self
    }

    #[must_use]
    pub fn cancel_and_hold_at_time(mut self, cancel_time: f64) -> Self {
        let cancel_time = cancel_time.max(0.0);
        let value = self.value_at(cancel_time);
        let replacement = self.replacement_event_for_hold(cancel_time, value);
        self.truncate_active_value_curves(cancel_time);
        self.events
            .retain(|event| event_is_before_cancel_hold(event, cancel_time));
        self.events.push(replacement);
        self.sort_events();
        self
    }

    #[must_use]
    pub fn value_at(&self, time: f64) -> f32 {
        let mut previous_time = 0.0;
        let mut previous_value = self.current_value;
        for (event_index, event) in self.events.iter().enumerate() {
            match *event {
                AudioParamEvent::SetValue {
                    time: event_time,
                    value,
                } => {
                    if time < event_time {
                        return self.clamp_value(previous_value);
                    }
                    previous_time = event_time;
                    previous_value = value;
                }
                AudioParamEvent::LinearRamp { end_time, value } => {
                    if time <= end_time {
                        let span = end_time - previous_time;
                        if span <= f64::EPSILON {
                            return self.clamp_value(value);
                        }
                        let amount = ((time - previous_time) / span).clamp(0.0, 1.0) as f32;
                        return self
                            .clamp_value(previous_value + (value - previous_value) * amount);
                    }
                    previous_time = end_time;
                    previous_value = value;
                }
                AudioParamEvent::ExponentialRamp { end_time, value } => {
                    if time <= end_time {
                        let span = end_time - previous_time;
                        if span <= f64::EPSILON {
                            return self.clamp_value(value);
                        }
                        if previous_value == 0.0 {
                            if time < end_time {
                                return self.clamp_value(previous_value);
                            }
                            return self.clamp_value(value);
                        }
                        if previous_value.signum() != value.signum() {
                            if time < end_time {
                                return self.clamp_value(previous_value);
                            }
                            return self.clamp_value(value);
                        }
                        let start = previous_value;
                        let amount = ((time - previous_time) / span).clamp(0.0, 1.0) as f32;
                        return self.clamp_value(start * (value / start).powf(amount));
                    }
                    previous_time = end_time;
                    previous_value = value;
                }
                AudioParamEvent::SetTarget {
                    start_time,
                    target,
                    time_constant,
                } => {
                    if time < start_time {
                        return self.clamp_value(previous_value);
                    }
                    if time_constant <= f64::EPSILON {
                        return self.clamp_value(target);
                    }
                    if self
                        .events
                        .get(event_index + 1)
                        .is_some_and(AudioParamEvent::is_ramp)
                    {
                        previous_time = start_time;
                        continue;
                    }
                    previous_value = target
                        + (previous_value - target)
                            * (-(time - start_time) / time_constant).exp() as f32;
                    previous_time = time;
                }
                AudioParamEvent::ValueCurve {
                    start_time,
                    duration,
                    active_until,
                    ref values,
                } => {
                    if time < start_time {
                        return self.clamp_value(previous_value);
                    }
                    if values.is_empty() {
                        continue;
                    }
                    let active_end = active_until.unwrap_or(start_time + duration);
                    if time < active_end {
                        return self
                            .clamp_value(sample_value_curve(values, start_time, duration, time));
                    }
                    previous_time = active_end;
                    previous_value = sample_value_curve(values, start_time, duration, active_end);
                }
            }
        }
        self.clamp_value(previous_value)
    }

    fn value_at_monotonic(&self, time: f64, runtime: &mut ParamTimelineRuntime) -> f32 {
        if self
            .events
            .iter()
            .any(|event| matches!(event, AudioParamEvent::SetTarget { .. }))
        {
            return self.value_at(time);
        }

        if !runtime.initialized || time < runtime.last_time {
            runtime.initialized = true;
            runtime.next_event_index = 0;
            runtime.previous_time = 0.0;
            runtime.previous_value = self.current_value;
        }
        runtime.last_time = time;

        while let Some(event) = self.events.get(runtime.next_event_index) {
            match event {
                AudioParamEvent::SetValue {
                    time: event_time,
                    value,
                } => {
                    if time < *event_time {
                        return self.clamp_value(runtime.previous_value);
                    }
                    runtime.previous_time = *event_time;
                    runtime.previous_value = *value;
                    runtime.next_event_index += 1;
                }
                AudioParamEvent::LinearRamp { end_time, value } => {
                    if time <= *end_time {
                        let span = *end_time - runtime.previous_time;
                        if span <= f64::EPSILON {
                            return self.clamp_value(*value);
                        }
                        let amount = ((time - runtime.previous_time) / span).clamp(0.0, 1.0) as f32;
                        return self.clamp_value(
                            runtime.previous_value + (*value - runtime.previous_value) * amount,
                        );
                    }
                    runtime.previous_time = *end_time;
                    runtime.previous_value = *value;
                    runtime.next_event_index += 1;
                }
                AudioParamEvent::ExponentialRamp { end_time, value } => {
                    if time <= *end_time {
                        let span = *end_time - runtime.previous_time;
                        if span <= f64::EPSILON {
                            return self.clamp_value(*value);
                        }
                        if runtime.previous_value == 0.0
                            || runtime.previous_value.signum() != value.signum()
                        {
                            if time < *end_time {
                                return self.clamp_value(runtime.previous_value);
                            }
                            return self.clamp_value(*value);
                        }
                        let amount = ((time - runtime.previous_time) / span).clamp(0.0, 1.0) as f32;
                        return self.clamp_value(
                            runtime.previous_value * (*value / runtime.previous_value).powf(amount),
                        );
                    }
                    runtime.previous_time = *end_time;
                    runtime.previous_value = *value;
                    runtime.next_event_index += 1;
                }
                AudioParamEvent::SetTarget { .. } => unreachable!("set target falls back above"),
                AudioParamEvent::ValueCurve {
                    start_time,
                    duration,
                    active_until,
                    values,
                } => {
                    if time < *start_time {
                        return self.clamp_value(runtime.previous_value);
                    }
                    if values.is_empty() {
                        runtime.next_event_index += 1;
                        continue;
                    }
                    let active_end = active_until.unwrap_or(*start_time + *duration);
                    if time < active_end {
                        return self.clamp_value(sample_value_curve(
                            values,
                            *start_time,
                            *duration,
                            time,
                        ));
                    }
                    runtime.previous_time = active_end;
                    runtime.previous_value =
                        sample_value_curve(values, *start_time, *duration, active_end);
                    runtime.next_event_index += 1;
                }
            }
        }

        self.clamp_value(runtime.previous_value)
    }

    fn clamp_value(&self, value: f32) -> f32 {
        value.clamp(self.min_value, self.max_value)
    }

    fn sort_events(&mut self) {
        self.events.sort_by(|a, b| a.time().total_cmp(&b.time()));
    }

    fn push_event(&mut self, event: AudioParamEvent) {
        self.events.push(event);
        self.sort_events();
    }

    fn validate_event_time_for_value_curves(&self, time: f64) -> Result<(), GraphError> {
        if self.events.iter().any(|event| {
            matches!(
                event,
                AudioParamEvent::ValueCurve {
                    start_time,
                    duration,
                    ..
                } if time > *start_time && time < *start_time + *duration
            )
        }) {
            return Err(GraphError::InvalidAutomationValue);
        }
        Ok(())
    }

    fn validate_value_curve_interval(
        &self,
        start_time: f64,
        duration: f64,
    ) -> Result<(), GraphError> {
        let end_time = start_time + duration;
        if self.events.iter().any(|event| match event {
            AudioParamEvent::ValueCurve {
                start_time: existing_start,
                duration: existing_duration,
                active_until,
                ..
            } => {
                if (*existing_start - start_time).abs() <= f64::EPSILON {
                    return false;
                }
                let existing_end = active_until.unwrap_or(*existing_start + *existing_duration);
                start_time < existing_end && *existing_start < end_time
            }
            _ => {
                let event_time = event.time();
                event_time > start_time && event_time < end_time
            }
        }) {
            return Err(GraphError::InvalidAutomationValue);
        }
        Ok(())
    }

    fn replacement_event_for_hold(&self, cancel_time: f64, value: f32) -> AudioParamEvent {
        let mut previous_time = 0.0;
        for event in &self.events {
            match *event {
                AudioParamEvent::SetValue { time, .. } => {
                    if cancel_time < time {
                        break;
                    }
                    previous_time = time;
                }
                AudioParamEvent::LinearRamp { end_time, .. } => {
                    if cancel_time <= end_time && cancel_time >= previous_time {
                        return AudioParamEvent::LinearRamp {
                            end_time: cancel_time,
                            value,
                        };
                    }
                    previous_time = end_time;
                }
                AudioParamEvent::ExponentialRamp { end_time, .. } => {
                    if cancel_time <= end_time && cancel_time >= previous_time {
                        return AudioParamEvent::ExponentialRamp {
                            end_time: cancel_time,
                            value,
                        };
                    }
                    previous_time = end_time;
                }
                AudioParamEvent::SetTarget { start_time, .. } => {
                    if cancel_time < start_time {
                        break;
                    }
                    previous_time = start_time;
                }
                AudioParamEvent::ValueCurve {
                    start_time,
                    duration,
                    active_until,
                    ..
                } => {
                    let active_end = active_until.unwrap_or(start_time + duration);
                    if cancel_time >= start_time && cancel_time <= active_end {
                        break;
                    }
                    previous_time = active_end;
                }
            }
        }
        AudioParamEvent::SetValue {
            time: cancel_time,
            value,
        }
    }

    fn truncate_active_value_curves(&mut self, cancel_time: f64) {
        for event in &mut self.events {
            if let AudioParamEvent::ValueCurve {
                start_time,
                duration,
                active_until,
                ..
            } = event
            {
                let event_end = *start_time + *duration;
                if cancel_time >= *start_time && cancel_time <= event_end {
                    *active_until = Some(cancel_time);
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum AudioParamEvent {
    SetValue {
        time: f64,
        value: f32,
    },
    LinearRamp {
        end_time: f64,
        value: f32,
    },
    ExponentialRamp {
        end_time: f64,
        value: f32,
    },
    SetTarget {
        start_time: f64,
        target: f32,
        time_constant: f64,
    },
    ValueCurve {
        start_time: f64,
        duration: f64,
        active_until: Option<f64>,
        values: Vec<f32>,
    },
}

fn sample_value_curve(values: &[f32], start_time: f64, duration: f64, time: f64) -> f32 {
    let position = ((time - start_time) / duration).clamp(0.0, 1.0);
    let index = position * (values.len() - 1) as f64;
    let left = index.floor() as usize;
    let right = index.ceil() as usize;
    if left == right {
        values[left]
    } else {
        let amount = (index - left as f64) as f32;
        values[left] + (values[right] - values[left]) * amount
    }
}

fn event_is_before_cancel_scheduled(event: &AudioParamEvent, cancel_time: f64) -> bool {
    match event {
        AudioParamEvent::ValueCurve {
            duration,
            active_until,
            ..
        } => active_until.unwrap_or(event.time() + *duration) <= cancel_time,
        _ => event.time() < cancel_time,
    }
}

fn event_is_before_cancel_hold(event: &AudioParamEvent, cancel_time: f64) -> bool {
    match event {
        AudioParamEvent::ValueCurve {
            start_time,
            duration,
            active_until,
            ..
        } => active_until.unwrap_or(*start_time + *duration) <= cancel_time,
        _ => event.time() < cancel_time,
    }
}

impl AudioParamEvent {
    fn time(&self) -> f64 {
        match self {
            Self::SetValue { time, .. } => *time,
            Self::LinearRamp { end_time, .. } | Self::ExponentialRamp { end_time, .. } => *end_time,
            Self::SetTarget { start_time, .. } | Self::ValueCurve { start_time, .. } => *start_time,
        }
    }

    fn is_ramp(&self) -> bool {
        matches!(self, Self::LinearRamp { .. } | Self::ExponentialRamp { .. })
    }
}

#[derive(Debug, Default)]
struct ParamTimelineRuntime {
    initialized: bool,
    last_time: f64,
    next_event_index: usize,
    previous_time: f64,
    previous_value: f32,
}

impl ParamTimelineRuntime {
    #[cfg(test)]
    fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod param_runtime_tests {
    use super::*;

    #[test]
    fn monotonic_param_runtime_matches_uncached_value_at() {
        let param = ParamTimeline::new(0.0)
            .set_value_at_time(0.25, 0.1)
            .linear_ramp_to_value_at_time(1.0, 0.5)
            .set_value_curve_at_time([1.0, 0.5, 0.0], 0.75, 0.25);
        let mut runtime = ParamTimelineRuntime::new();

        for step in 0..=100 {
            let time = step as f64 / 100.0;
            let cached = param.value_at_monotonic(time, &mut runtime);
            let uncached = param.value_at(time);
            assert!(
                (cached - uncached).abs() <= 0.0001,
                "cached value {cached} should match uncached value {uncached} at {time}"
            );
        }
    }
}
