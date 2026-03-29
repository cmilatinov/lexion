use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy)]
pub struct Label {
    prefix: &'static str,
    pad: Option<usize>,
    counter: usize,
}

impl Display for Label {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let pad = self.pad.unwrap_or(0);
        write!(f, "{}{:0>pad$}", self.prefix, self.counter, pad = pad)
    }
}

pub struct LabelGenerator {
    prefix: &'static str,
    pad: Option<usize>,
    next: usize,
}

impl LabelGenerator {
    pub fn new(prefix: &'static str, pad: Option<usize>) -> Self {
        Self {
            prefix,
            pad,
            next: 1,
        }
    }

    pub fn next(&mut self) -> Label {
        let Self {
            prefix,
            pad,
            next: counter,
        } = *self;
        self.next = counter + 1;
        Label {
            prefix,
            pad,
            counter,
        }
    }
}
