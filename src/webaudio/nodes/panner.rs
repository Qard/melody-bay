#[derive(Debug, Clone)]
pub struct PannerNode {
    id: NodeId,
    graph: Arc<Mutex<GraphInner>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanningModel {
    EqualPower,
    Hrtf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistanceModel {
    Linear,
    Inverse,
    Exponential,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PannerOptions {
    pub panning_model: PanningModel,
    pub distance_model: DistanceModel,
    pub position_x: f32,
    pub position_y: f32,
    pub position_z: f32,
    pub orientation_x: f32,
    pub orientation_y: f32,
    pub orientation_z: f32,
    pub ref_distance: f32,
    pub max_distance: f32,
    pub rolloff_factor: f32,
    pub cone_inner_angle: f32,
    pub cone_outer_angle: f32,
    pub cone_outer_gain: f32,
}

impl Default for PannerOptions {
    fn default() -> Self {
        Self {
            panning_model: PanningModel::EqualPower,
            distance_model: DistanceModel::Inverse,
            position_x: 0.0,
            position_y: 0.0,
            position_z: 0.0,
            orientation_x: 1.0,
            orientation_y: 0.0,
            orientation_z: 0.0,
            ref_distance: 1.0,
            max_distance: 10_000.0,
            rolloff_factor: 1.0,
            cone_inner_angle: 360.0,
            cone_outer_angle: 360.0,
            cone_outer_gain: 0.0,
        }
    }
}

impl PannerNode {
    #[must_use]
    pub fn position_x(&self) -> AudioParamHandle {
        AudioParamHandle {
            graph: Arc::clone(&self.graph),
            id: self.position_x_param(),
        }
    }

    #[must_use]
    pub fn position_y(&self) -> AudioParamHandle {
        AudioParamHandle {
            graph: Arc::clone(&self.graph),
            id: self.position_y_param(),
        }
    }

    #[must_use]
    pub fn position_z(&self) -> AudioParamHandle {
        AudioParamHandle {
            graph: Arc::clone(&self.graph),
            id: self.position_z_param(),
        }
    }

    #[must_use]
    pub fn orientation_x(&self) -> AudioParamHandle {
        AudioParamHandle {
            graph: Arc::clone(&self.graph),
            id: self.orientation_x_param(),
        }
    }

    #[must_use]
    pub fn orientation_y(&self) -> AudioParamHandle {
        AudioParamHandle {
            graph: Arc::clone(&self.graph),
            id: self.orientation_y_param(),
        }
    }

    #[must_use]
    pub fn orientation_z(&self) -> AudioParamHandle {
        AudioParamHandle {
            graph: Arc::clone(&self.graph),
            id: self.orientation_z_param(),
        }
    }

    pub fn set_distance_model(&self, model: DistanceModel) {
        self.try_distance_model(model)
            .expect("distance model is always valid");
    }

    pub fn set_panning_model(&self, model: PanningModel) -> Result<(), GraphError> {
        if model == PanningModel::Hrtf {
            return Err(GraphError::UnsupportedPanningModel);
        }
        self.set_panner(|kind| {
            if let NodeKind::Panner { panning_model, .. } = kind {
                *panning_model = model;
            }
            Ok(())
        })
    }

    pub fn try_distance_model(&self, model: DistanceModel) -> Result<(), GraphError> {
        self.set_panner(|kind| {
            if let NodeKind::Panner { distance_model, .. } = kind {
                *distance_model = model;
            }
            Ok(())
        })
    }

    pub fn try_cone_inner_angle(&self, degrees: f32) -> Result<(), GraphError> {
        validate_panner_angle(degrees)?;
        self.set_panner(|kind| {
            if let NodeKind::Panner {
                cone_inner_angle, ..
            } = kind
            {
                *cone_inner_angle = degrees;
            }
            Ok(())
        })
    }

    pub fn try_cone_outer_angle(&self, degrees: f32) -> Result<(), GraphError> {
        validate_panner_angle(degrees)?;
        self.set_panner(|kind| {
            if let NodeKind::Panner {
                cone_outer_angle, ..
            } = kind
            {
                *cone_outer_angle = degrees;
            }
            Ok(())
        })
    }

    pub fn try_cone_outer_gain(&self, gain: f32) -> Result<(), GraphError> {
        if !gain.is_finite() || !(0.0..=1.0).contains(&gain) {
            return Err(GraphError::InvalidPannerConfig);
        }
        self.set_panner(|kind| {
            if let NodeKind::Panner {
                cone_outer_gain, ..
            } = kind
            {
                *cone_outer_gain = gain;
            }
            Ok(())
        })
    }

    pub fn try_ref_distance(&self, distance: f32) -> Result<(), GraphError> {
        validate_non_negative_panner_value(distance)?;
        self.set_panner(|kind| {
            if let NodeKind::Panner { ref_distance, .. } = kind {
                *ref_distance = distance;
            }
            Ok(())
        })
    }

    pub fn try_max_distance(&self, distance: f32) -> Result<(), GraphError> {
        validate_positive_panner_value(distance)?;
        self.set_panner(|kind| {
            if let NodeKind::Panner { max_distance, .. } = kind {
                *max_distance = distance;
            }
            Ok(())
        })
    }

    pub fn try_rolloff_factor(&self, factor: f32) -> Result<(), GraphError> {
        if !factor.is_finite() || factor < 0.0 {
            return Err(GraphError::InvalidPannerConfig);
        }
        self.set_panner(|kind| {
            if let NodeKind::Panner { rolloff_factor, .. } = kind {
                *rolloff_factor = factor;
            }
            Ok(())
        })
    }

    #[must_use]
    pub fn panning_model_value(&self) -> PanningModel {
        let inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::Panner { panning_model, .. } = &inner.nodes[self.id.0].kind {
            *panning_model
        } else {
            PanningModel::EqualPower
        }
    }

    #[must_use]
    pub fn distance_model_value(&self) -> DistanceModel {
        let inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::Panner { distance_model, .. } = &inner.nodes[self.id.0].kind {
            *distance_model
        } else {
            DistanceModel::Inverse
        }
    }

    #[must_use]
    pub fn cone_inner_angle_value(&self) -> f32 {
        self.panner_value(|kind| match kind {
            NodeKind::Panner {
                cone_inner_angle, ..
            } => *cone_inner_angle,
            _ => 360.0,
        })
    }

    #[must_use]
    pub fn cone_outer_angle_value(&self) -> f32 {
        self.panner_value(|kind| match kind {
            NodeKind::Panner {
                cone_outer_angle, ..
            } => *cone_outer_angle,
            _ => 360.0,
        })
    }

    #[must_use]
    pub fn cone_outer_gain_value(&self) -> f32 {
        self.panner_value(|kind| match kind {
            NodeKind::Panner {
                cone_outer_gain, ..
            } => *cone_outer_gain,
            _ => 0.0,
        })
    }

    #[must_use]
    pub fn ref_distance_value(&self) -> f32 {
        self.panner_value(|kind| match kind {
            NodeKind::Panner { ref_distance, .. } => *ref_distance,
            _ => 1.0,
        })
    }

    #[must_use]
    pub fn max_distance_value(&self) -> f32 {
        self.panner_value(|kind| match kind {
            NodeKind::Panner { max_distance, .. } => *max_distance,
            _ => 10_000.0,
        })
    }

    #[must_use]
    pub fn rolloff_factor_value(&self) -> f32 {
        self.panner_value(|kind| match kind {
            NodeKind::Panner { rolloff_factor, .. } => *rolloff_factor,
            _ => 1.0,
        })
    }

    fn set_panner(
        &self,
        update: impl FnOnce(&mut NodeKind) -> Result<(), GraphError>,
    ) -> Result<(), GraphError> {
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        update(&mut inner.nodes[self.id.0].kind)
    }

    fn panner_value(&self, read: impl FnOnce(&NodeKind) -> f32) -> f32 {
        let inner = self.graph.lock().expect("graph mutex poisoned");
        read(&inner.nodes[self.id.0].kind)
    }

    #[must_use]
    fn position_x_param(&self) -> ParamId {
        ParamId {
            node: self.id,
            param: ParamKind::PositionX,
        }
    }

    #[must_use]
    fn position_y_param(&self) -> ParamId {
        ParamId {
            node: self.id,
            param: ParamKind::PositionY,
        }
    }

    #[must_use]
    fn position_z_param(&self) -> ParamId {
        ParamId {
            node: self.id,
            param: ParamKind::PositionZ,
        }
    }

    #[must_use]
    fn orientation_x_param(&self) -> ParamId {
        ParamId {
            node: self.id,
            param: ParamKind::OrientationX,
        }
    }

    #[must_use]
    fn orientation_y_param(&self) -> ParamId {
        ParamId {
            node: self.id,
            param: ParamKind::OrientationY,
        }
    }

    #[must_use]
    fn orientation_z_param(&self) -> ParamId {
        ParamId {
            node: self.id,
            param: ParamKind::OrientationZ,
        }
    }

    #[must_use]
    pub fn param(&self, name: &str) -> Option<AudioParamHandle> {
        self.parameter(name)
    }

    #[must_use]
    pub fn parameter(&self, name: &str) -> Option<AudioParamHandle> {
        match name {
            "positionX" | "position_x" => Some(self.position_x()),
            "positionY" | "position_y" => Some(self.position_y()),
            "positionZ" | "position_z" => Some(self.position_z()),
            "orientationX" | "orientation_x" => Some(self.orientation_x()),
            "orientationY" | "orientation_y" => Some(self.orientation_y()),
            "orientationZ" | "orientation_z" => Some(self.orientation_z()),
            _ => None,
        }
    }
}

impl From<PannerNode> for NodeId {
    fn from(value: PannerNode) -> Self {
        value.id
    }
}

impl From<&PannerNode> for NodeId {
    fn from(value: &PannerNode) -> Self {
        value.id
    }
}

impl_node_channel_config!(PannerNode);

macro_rules! impl_audio_node_handle_with_graph {
    ($($node:ty),* $(,)?) => {
        $(
            impl private::AudioNodeHandlePrivate for $node {
                fn node_id(&self) -> NodeId {
                    self.id
                }

                fn context_identity(&self) -> Option<usize> {
                    Some(Arc::as_ptr(&self.graph) as usize)
                }
            }
        )*
    };
}

impl_audio_node_handle_with_graph!(
    OscillatorNode,
    ConstantSourceNode,
    AudioBufferSourceNode,
    SoundDataSourceNode,
    GainNode,
    StereoPannerNode,
    BiquadFilterHandle,
    IirFilterNode,
    WaveShaperNode,
    ConvolverNode,
    DelayNodeHandle,
    DynamicsCompressorNode,
    AnalyserNode,
    AudioWorkletNode,
    PannerNode,
);

impl_node_channel_config!(AudioDestinationNode);
