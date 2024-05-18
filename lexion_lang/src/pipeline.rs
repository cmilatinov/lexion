use crate::diagnostic::DiagnosticConsumer;

pub trait PipelineStage {
    type Input;
    type Output;
    fn exec(self, diag: &mut dyn DiagnosticConsumer, input: &Self::Input) -> Option<Self::Output>;
}
