use std::fmt::{Display, Formatter};

#[derive(Debug, PartialEq, Eq)]
pub enum Type {
    Tuple {
        types: Vec<Type>,
    },
    Struct {
        ident: String,
        ref_count: usize,
    },
    Function {
        params: Vec<Type>,
        return_type: Box<Type>,
    },
    TypeDef {
        ident: String,
        ty: String,
    },
}

impl Display for Type {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Tuple { types } => {
                write!(
                    f,
                    "({})",
                    types
                        .iter()
                        .map(|t| t.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            Type::Struct { ident, ref_count } => {
                write!(f, "{}{}", "&".repeat(*ref_count), ident)
            }
            Type::Function {
                params,
                return_type,
            } => {
                write!(
                    f,
                    "fn ({}) -> {}",
                    params
                        .iter()
                        .map(|p| p.to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                    return_type
                )
            }
            Type::TypeDef { ident, .. } => {
                write!(f, "{}", ident)
            }
        }
    }
}

impl Type {
    pub fn void() -> Self {
        Self::Struct {
            ident: "void".to_string(),
            ref_count: 0,
        }
    }
}
