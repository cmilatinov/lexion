use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::ops::{Deref, DerefMut};
use std::string::ToString;

use generational_arena::{Arena, Index};
use lazy_static::lazy_static;

#[derive(Debug, Clone, PartialEq, Eq)]
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

lazy_static! {
    pub static ref TYPE_VOID: Type = Type::Struct {
        ident: String::from("void"),
        ref_count: 0
    };
    pub static ref TYPE_U32: Type = Type::Struct {
        ident: String::from("u32"),
        ref_count: 0
    };
    pub static ref TYPE_F32: Type = Type::Struct {
        ident: String::from("f32"),
        ref_count: 0
    };
    pub static ref TYPE_STR: Type = Type::Struct {
        ident: String::from("str"),
        ref_count: 0
    };
    pub static ref TYPE_BOOL: Type = Type::Struct {
        ident: String::from("bool"),
        ref_count: 0
    };
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

#[derive(Default)]
pub struct TypeCollection {
    pub arena: Arena<Type>,
    pub type_strings: HashMap<String, Index>,
    pub type_map: HashMap<Index, Index>,
}

impl Deref for TypeCollection {
    type Target = Arena<Type>;
    fn deref(&self) -> &Self::Target {
        &self.arena
    }
}

impl DerefMut for TypeCollection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.arena
    }
}

impl TypeCollection {
    pub fn insert(&mut self, ty: Type) -> Index {
        let string = ty.to_string();
        if let Some(index) = self.type_strings.get(&string) {
            *index
        } else {
            let index = self.arena.insert(ty);
            self.type_strings.insert(string, index);
            index
        }
    }

    pub fn canonicalize(&self, ty: Index) -> Index {
        let mut result = ty;
        while let Some(next) = self.type_map.get(&ty) {
            result = *next;
        }
        result
    }

    pub fn eq(&self, mut a: Index, mut b: Index) -> bool {
        a = self.canonicalize(a);
        b = self.canonicalize(b);
        a == b
    }
}
