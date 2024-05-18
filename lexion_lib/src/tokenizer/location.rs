use std::cmp::Ordering;
use std::fmt::{Display, Formatter, Result};

use crate::tokenizer::TokenInstance;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Ord)]
pub struct FileLocation {
    pub line: usize,
    pub col: usize,
}

impl Default for FileLocation {
    fn default() -> Self {
        Self { line: 1, col: 1 }
    }
}

impl Display for FileLocation {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

impl PartialOrd for FileLocation {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let line_diff = (self.line as isize) - (other.line as isize);
        if line_diff < 0 {
            return Some(Ordering::Less);
        } else if line_diff > 0 {
            return Some(Ordering::Greater);
        }
        let col_diff = (self.col as isize) - (other.col as isize);
        if col_diff < 0 {
            Some(Ordering::Less)
        } else if col_diff > 0 {
            Some(Ordering::Greater)
        } else {
            Some(Ordering::Equal)
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct SourceLocation {
    pub file: &'static str,
    pub loc: FileLocation,
}

impl Default for SourceLocation {
    fn default() -> Self {
        Self {
            file: "inline".into(),
            loc: Default::default(),
        }
    }
}

impl Display for SourceLocation {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "(file:///{}:{})", self.file, self.loc)
    }
}

#[derive(Debug, Copy, Clone)]
pub struct SourceRange {
    pub file: &'static str,
    pub start: FileLocation,
    pub end: FileLocation,
}

impl Display for SourceRange {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "(file:///{}:[{}-{}])", self.file, self.start, self.end)
    }
}

impl From<SourceLocation> for SourceRange {
    fn from(value: SourceLocation) -> Self {
        Self {
            file: value.file,
            start: value.loc,
            end: value.loc,
        }
    }
}

impl From<&TokenInstance> for SourceRange {
    fn from(value: &TokenInstance) -> Self {
        Self::from_loc_len(value.loc, value.value.len())
    }
}

impl From<(&TokenInstance, &TokenInstance)> for SourceRange {
    fn from((start, end): (&TokenInstance, &TokenInstance)) -> Self {
        Self::from_start_end(start.loc, end.loc)
    }
}

impl SourceRange {
    pub fn from_loc_len(loc: SourceLocation, len: usize) -> Self {
        Self {
            file: loc.file,
            start: loc.loc,
            end: FileLocation {
                line: loc.loc.line,
                col: loc.loc.col + len,
            },
        }
    }

    pub fn from_start_end(mut start: SourceLocation, mut end: SourceLocation) -> Self {
        if let Ordering::Greater = start.loc.cmp(&end.loc) {
            (start, end) = (end, start);
        }
        Self {
            file: start.file,
            start: start.loc,
            end: end.loc,
        }
    }

    pub fn extend(mut self, loc: SourceLocation) -> Self {
        if let Ordering::Greater = self.start.cmp(&loc.loc) {
            self.start = loc.loc;
        }
        if let Ordering::Less = self.end.cmp(&loc.loc) {
            self.end = loc.loc;
        }
        self
    }

    pub fn merge(self, other: SourceRange) -> Self {
        let slice = &[self.start, self.end, other.start, other.end];
        let min = slice.iter().min().unwrap();
        let max = slice.iter().max().unwrap();
        Self {
            file: self.file,
            start: *min,
            end: *max,
        }
    }

    pub fn start(&self) -> SourceLocation {
        SourceLocation {
            file: self.file,
            loc: self.start,
        }
    }

    pub fn end(&self) -> SourceLocation {
        SourceLocation {
            file: self.file,
            loc: self.end,
        }
    }
}
