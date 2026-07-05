#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ParamKind {
    Gain,
    Frequency,
    Detune,
    Offset,
    PlaybackRate,
    Q,
    FilterGain,
    Pan,
    DelayTime,
    Threshold,
    Knee,
    Ratio,
    Attack,
    Release,
    PositionX,
    PositionY,
    PositionZ,
    OrientationX,
    OrientationY,
    OrientationZ,
    ForwardX,
    ForwardY,
    ForwardZ,
    UpX,
    UpY,
    UpZ,
    WorkletParam(usize),
}

const LISTENER_PARAM_NODE: NodeId = NodeId(usize::MAX);

