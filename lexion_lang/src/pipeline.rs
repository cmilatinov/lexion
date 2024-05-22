use crate::diagnostic::DiagnosticConsumer;

pub trait PipelineStage {
    type Input;
    type Output;
    fn new(input: Self::Input) -> Self;
    fn exec(self, diag: &mut dyn DiagnosticConsumer) -> Option<Self::Output>;
}
