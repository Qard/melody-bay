#[derive(Debug, Clone)]
pub struct GainNode {
    id: NodeId,
    graph: Arc<Mutex<GraphInner>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GainOptions {
    pub gain: f32,
}

impl Default for GainOptions {
    fn default() -> Self {
        Self { gain: 1.0 }
    }
}

impl GainNode {
    #[must_use]
    pub fn gain(&self) -> AudioParamHandle {
        AudioParamHandle {
            graph: Arc::clone(&self.graph),
            id: self.gain_param_id(),
        }
    }

    #[must_use]
    fn gain_param_id(&self) -> ParamId {
        ParamId {
            node: self.id,
            param: ParamKind::Gain,
        }
    }

    #[must_use]
    pub fn param(&self, name: &str) -> Option<AudioParamHandle> {
        self.parameter(name)
    }

    #[must_use]
    pub fn parameter(&self, name: &str) -> Option<AudioParamHandle> {
        match name {
            "gain" => Some(self.gain()),
            _ => None,
        }
    }

    #[must_use]
    pub fn channel_count(&self) -> usize {
        node_channel_config(&self.graph, self.id).channel_count
    }

    #[must_use]
    pub fn channel_count_mode(&self) -> ChannelCountMode {
        node_channel_config(&self.graph, self.id).channel_count_mode
    }

    #[must_use]
    pub fn channel_interpretation(&self) -> ChannelInterpretation {
        node_channel_config(&self.graph, self.id).channel_interpretation
    }

    pub fn try_set_channel_config(
        &self,
        channel_count: usize,
        channel_count_mode: ChannelCountMode,
        channel_interpretation: ChannelInterpretation,
    ) -> Result<(), GraphError> {
        try_set_node_channel_config(
            &self.graph,
            self.id,
            channel_count,
            channel_count_mode,
            channel_interpretation,
        )
    }
}

impl From<GainNode> for NodeId {
    fn from(value: GainNode) -> Self {
        value.id
    }
}

impl From<&GainNode> for NodeId {
    fn from(value: &GainNode) -> Self {
        value.id
    }
}

#[derive(Debug, Clone)]
pub struct AudioParamHandle {
    graph: Arc<Mutex<GraphInner>>,
    id: ParamId,
}

impl AudioParamHandle {
    #[must_use]
    pub fn default_value(&self) -> f32 {
        self.with_param(ParamTimeline::default_value).unwrap_or(0.0)
    }

    #[must_use]
    pub fn min_value(&self) -> f32 {
        self.with_param(ParamTimeline::min_value).unwrap_or(0.0)
    }

    #[must_use]
    pub fn max_value(&self) -> f32 {
        self.with_param(ParamTimeline::max_value).unwrap_or(0.0)
    }

    #[must_use]
    pub fn automation_rate(&self) -> AutomationRate {
        self.with_param(ParamTimeline::automation_rate)
            .unwrap_or(AutomationRate::ARate)
    }

    pub fn set_automation_rate(&self, automation_rate: AutomationRate) -> Result<(), GraphError> {
        self.try_set_automation_rate(automation_rate)
    }

    pub fn try_set_automation_rate(
        &self,
        automation_rate: AutomationRate,
    ) -> Result<(), GraphError> {
        self.update_param_result(|param| param.try_set_automation_rate(automation_rate))
    }

    #[must_use]
    pub fn value(&self) -> f32 {
        self.with_param(ParamTimeline::value).unwrap_or(0.0)
    }

    #[must_use]
    pub fn value_at(&self, time: f64) -> f32 {
        self.with_param(|param| param.value_at(time)).unwrap_or(0.0)
    }

    pub fn set_value(&self, value: f32) -> Result<(), GraphError> {
        validate_automation_value(value)?;
        self.update_param(|param| {
            param.current_value = param.clamp_value(value);
            param.events.retain(|event| {
                !matches!(
                    event,
                    AudioParamEvent::SetValue { time, .. } if *time <= f64::EPSILON
                )
            });
            param
                .events
                .push(AudioParamEvent::SetValue { time: 0.0, value });
            param.sort_events();
        })
    }

    pub fn set_value_at_time(&self, value: f32, time: f64) -> Result<(), GraphError> {
        validate_automation_time(time)?;
        validate_automation_value(value)?;
        self.update_param_result(|param| {
            param.validate_event_time_for_value_curves(time)?;
            *param = param.clone().set_value_at_time(value, time);
            Ok(())
        })
    }

    pub fn linear_ramp_to_value_at_time(
        &self,
        value: f32,
        end_time: f64,
    ) -> Result<(), GraphError> {
        validate_automation_time(end_time)?;
        validate_automation_value(value)?;
        self.update_param_result(|param| {
            param.validate_event_time_for_value_curves(end_time)?;
            *param = param.clone().linear_ramp_to_value_at_time(value, end_time);
            Ok(())
        })
    }

    pub fn exponential_ramp_to_value_at_time(
        &self,
        value: f32,
        end_time: f64,
    ) -> Result<(), GraphError> {
        self.try_exponential_ramp_to_value_at_time(value, end_time)
    }

    pub fn try_exponential_ramp_to_value_at_time(
        &self,
        value: f32,
        end_time: f64,
    ) -> Result<(), GraphError> {
        validate_automation_time(end_time)?;
        validate_automation_value(value)?;
        self.update_param_result(|param| {
            param.validate_event_time_for_value_curves(end_time)?;
            *param = param
                .clone()
                .try_exponential_ramp_to_value_at_time(value, end_time)?;
            Ok(())
        })
    }

    pub fn set_target_at_time(
        &self,
        target: f32,
        start_time: f64,
        time_constant: f64,
    ) -> Result<(), GraphError> {
        validate_automation_time(start_time)?;
        validate_automation_value(target)?;
        if time_constant < 0.0 || !time_constant.is_finite() {
            return Err(GraphError::InvalidAutomationValue);
        }
        self.update_param_result(|param| {
            param.validate_event_time_for_value_curves(start_time)?;
            *param = param
                .clone()
                .set_target_at_time(target, start_time, time_constant);
            Ok(())
        })
    }

    pub fn set_value_curve_at_time(
        &self,
        values: impl IntoIterator<Item = f32>,
        start_time: f64,
        duration: f64,
    ) -> Result<(), GraphError> {
        validate_automation_time(start_time)?;
        let values = values.into_iter().collect::<Vec<_>>();
        if values.len() < 2
            || duration <= 0.0
            || !duration.is_finite()
            || values.iter().any(|value| !value.is_finite())
        {
            return Err(GraphError::InvalidAutomationValue);
        }
        self.update_param_result(|param| {
            param.validate_value_curve_interval(start_time, duration)?;
            *param = param
                .clone()
                .set_value_curve_at_time(values, start_time, duration);
            Ok(())
        })
    }

    pub fn cancel_scheduled_values(&self, cancel_time: f64) -> Result<(), GraphError> {
        validate_automation_time(cancel_time)?;
        self.update_param(|param| {
            *param = param.clone().cancel_scheduled_values(cancel_time);
        })
    }

    pub fn cancel_and_hold_at_time(&self, cancel_time: f64) -> Result<(), GraphError> {
        validate_automation_time(cancel_time)?;
        self.update_param(|param| {
            *param = param.clone().cancel_and_hold_at_time(cancel_time);
        })
    }

    fn with_param<T>(&self, read: impl FnOnce(&ParamTimeline) -> T) -> Option<T> {
        let inner = self.graph.lock().expect("graph mutex poisoned");
        inner.param(self.id).map(read)
    }

    fn update_param(&self, update: impl FnOnce(&mut ParamTimeline)) -> Result<(), GraphError> {
        self.update_param_result(|param| {
            update(param);
            Ok(())
        })
    }

    fn update_param_result(
        &self,
        update: impl FnOnce(&mut ParamTimeline) -> Result<(), GraphError>,
    ) -> Result<(), GraphError> {
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        let Some(param) = inner.param_mut(self.id) else {
            return Err(GraphError::UnknownParam);
        };
        update(param)
    }

    fn context_identity(&self) -> Option<usize> {
        Some(Arc::as_ptr(&self.graph) as usize)
    }
}

fn validate_automation_time(time: f64) -> Result<(), GraphError> {
    if time < 0.0 {
        return Err(GraphError::NegativeTime);
    }
    if !time.is_finite() {
        return Err(GraphError::InvalidAutomationValue);
    }
    Ok(())
}

fn validate_automation_value(value: f32) -> Result<(), GraphError> {
    if !value.is_finite() {
        return Err(GraphError::InvalidAutomationValue);
    }
    Ok(())
}

fn validate_source_time(time: f64) -> Result<(), GraphError> {
    validate_automation_time(time)
}
