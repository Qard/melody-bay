mod private {
    use super::NodeId;

    pub(crate) trait AudioNodeHandlePrivate {
        fn node_id(&self) -> NodeId;

        fn context_identity(&self) -> Option<usize> {
            None
        }
    }
}

