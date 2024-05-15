use std::fmt::{Display, Formatter, Result};

#[derive(Debug, Copy, Clone)]
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

    pub fn from_start_end(start: SourceLocation, end: SourceLocation) -> Self {
        Self {
            file: start.file,
            start: start.loc,
            end: end.loc,
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
