use miette::SourceSpan;

#[allow(dead_code)]
pub struct SpanBuilder;

impl SpanBuilder {
    pub fn merge(mut a: SourceSpan, mut b: SourceSpan) -> SourceSpan {
        if a.offset() > b.offset() {
            (a, b) = (b, a);
        }
        (a.offset()..(b.offset() + b.len())).into()
    }

    pub fn start(span: SourceSpan) -> SourceSpan {
        span.offset().into()
    }

    pub fn end(span: SourceSpan) -> SourceSpan {
        (span.offset() + span.len()).into()
    }
}
