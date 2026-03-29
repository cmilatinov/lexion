use crate::diagnostic::DiagnosticConsumer;

pub trait PipelineStage {
    type Input;
    type Options;
    type Output;
    fn new(input: Self::Input) -> Self;
    fn exec(self, diag: &mut dyn DiagnosticConsumer, opts: Self::Options) -> Option<Self::Output>;
}
