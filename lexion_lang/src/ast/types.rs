use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

use generational_arena::{Arena, Index};
use lazy_static::lazy_static;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TupleType {
    pub types: Vec<Index>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StructType {
    pub ident: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RefType {
    pub to: Index,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FunctionType {
    pub params: Vec<Index>,
    pub return_type: Index,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TypeDefType {
    pub ident: String,
    pub ty: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    TupleType(TupleType),
    StructType(StructType),
    RefType(RefType),
    FunctionType(FunctionType),
    TypeDefType(TypeDefType),
}

lazy_static! {
    pub static ref TYPE_UNIT: Type = Type::TupleType(TupleType { types: vec![] });
    pub static ref TYPE_U32: Type = Type::StructType(StructType {
        ident: String::from("u32")
    });
    pub static ref TYPE_F32: Type = Type::StructType(StructType {
        ident: String::from("f32")
    });
    pub static ref TYPE_STR: Type = Type::StructType(StructType {
        ident: String::from("str")
    });
    pub static ref TYPE_BOOL: Type = Type::StructType(StructType {
        ident: String::from("bool")
    });
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
    pub fn to_string_list(&self, types: &Vec<Index>) -> String {
        types
            .iter()
            .map(|i| self.to_string_index(*i))
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn to_string_index(&self, ty: Index) -> String {
        match self.arena.get(ty) {
            Some(ty) => self.to_string(ty),
            None => String::from(""),
        }
    }

    pub fn to_string(&self, ty: &Type) -> String {
        match ty {
            Type::TupleType(TupleType { types }) => {
                format!("({})", self.to_string_list(types))
            }
            Type::StructType(StructType { ident }) => {
                format!("{}", ident)
            }
            Type::RefType(RefType { to }) => {
                format!("&{}", self.to_string_index(*to))
            }
            Type::FunctionType(FunctionType {
                params,
                return_type,
            }) => {
                format!(
                    "fn ({}) -> {}",
                    self.to_string_list(params),
                    self.to_string_index(*return_type)
                )
            }
            Type::TypeDefType(TypeDefType { ident, .. }) => {
                format!("{}", ident)
            }
        }
    }

    pub fn reference(&mut self, to: Index) -> Index {
        let ty = Type::RefType(RefType { to });
        self.insert(&ty)
    }

    pub fn insert(&mut self, ty: &Type) -> Index {
        let string = self.to_string(&ty);
        if let Some(index) = self.type_strings.get(&string) {
            *index
        } else {
            let index = self.arena.insert(ty.clone());
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
