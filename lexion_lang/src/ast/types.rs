use crate::ast::Type as AstType;
use crate::generators::x86::{
    Align, Bitness, Layout, MemoryLayout, MemoryLayoutBuilder, SizeAlign,
};
use generational_arena::{Arena, Index};
use lazy_static::lazy_static;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::ops::{Deref, DerefMut};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TupleType {
    pub types: Vec<Index>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StructMember {
    pub name: String,
    pub ty: Index,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StructType {
    pub ident: String,
    pub members: Vec<StructMember>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RefType {
    pub to: Index,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FunctionType {
    pub params: Vec<Index>,
    pub return_type: Index,
    pub is_vararg: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TypeDefType {
    pub ident: String,
    pub ty: Index,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PrimitiveType {
    U32,
    I32,
    F32,
    BOOL,
    CHAR,
    STR,
}

pub struct PrimitiveTypeLayout {
    primitive: PrimitiveType,
    bitness: Bitness,
}

impl PrimitiveType {
    pub fn layout(&self, bitness: Bitness) -> PrimitiveTypeLayout {
        PrimitiveTypeLayout {
            bitness,
            primitive: *self,
        }
    }
}

impl Layout for PrimitiveTypeLayout {
    fn size_align(&self) -> SizeAlign {
        match self.primitive {
            PrimitiveType::U32 => SizeAlign::from_size(4),
            PrimitiveType::I32 => SizeAlign::from_size(4),
            PrimitiveType::F32 => SizeAlign::from_size(4),
            PrimitiveType::BOOL => SizeAlign::from_size(1),
            PrimitiveType::CHAR => SizeAlign::from_size(1),
            PrimitiveType::STR => SizeAlign {
                size: 0,
                align: Align::none(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    TupleType(TupleType),
    StructType(StructType),
    RefType(RefType),
    FunctionType(FunctionType),
    TypeDefType(TypeDefType),
    PrimitiveType(PrimitiveType),
    Unknown,
}

pub enum TypeKind {
    Integer,
    Float,
    Double,
    Vector,
    Memory,
    Unknown,
}

lazy_static! {
    pub static ref TYPE_UNKNOWN: Type = Type::Unknown;
    pub static ref TYPE_UNIT: Type = Type::TupleType(TupleType { types: vec![] });
    pub static ref TYPE_U32: Type = Type::PrimitiveType(PrimitiveType::U32);
    pub static ref TYPE_I32: Type = Type::PrimitiveType(PrimitiveType::I32);
    pub static ref TYPE_F32: Type = Type::PrimitiveType(PrimitiveType::F32);
    pub static ref TYPE_BOOL: Type = Type::PrimitiveType(PrimitiveType::BOOL);
    pub static ref TYPE_CHAR: Type = Type::PrimitiveType(PrimitiveType::CHAR);
    pub static ref TYPE_STR: Type = Type::PrimitiveType(PrimitiveType::STR);
}

pub struct TypeCollection {
    pub arena: Arena<Type>,
    pub type_strings: HashMap<String, Index>,
    pub type_map: HashMap<Index, Index>,
    pub memory_layouts: HashMap<Index, MemoryLayout>,
}

impl Default for TypeCollection {
    fn default() -> Self {
        let mut types = Self {
            arena: Default::default(),
            type_strings: Default::default(),
            type_map: Default::default(),
            memory_layouts: Default::default(),
        };
        types.insert(&TYPE_UNKNOWN);
        types.insert(&TYPE_UNIT);
        types.insert(&TYPE_U32);
        types.insert(&TYPE_I32);
        types.insert(&TYPE_F32);
        types.insert(&TYPE_BOOL);
        types.insert(&TYPE_CHAR);
        let str = types.insert(&TYPE_STR);
        types.reference(str);
        types
    }
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

impl Display for TypeCollection {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        for (_, ty) in self.arena.iter() {
            writeln!(f, " - {}", self.to_string_type(ty))?;
        }
        Ok(())
    }
}

impl TypeCollection {
    pub fn to_string_list(&self, types: &[Index], vararg: bool) -> String {
        let mut types = types
            .iter()
            .map(|i| self.to_string_index(*i))
            .collect::<Vec<_>>();
        if vararg {
            types.push("...".into());
        }
        types.join(", ")
    }

    pub fn to_string_index(&self, ty: Index) -> Cow<str> {
        match self.arena.get(ty) {
            Some(ty) => self.to_string_type(ty),
            None => "".into(),
        }
    }

    pub fn to_string_type(&self, ty: &Type) -> Cow<'static, str> {
        match ty {
            Type::TupleType(TupleType { types }) => {
                format!("({})", self.to_string_list(types, false)).into()
            }
            Type::StructType(StructType { ident, .. }) => ident.to_string().into(),
            Type::RefType(RefType { to }) => format!("&{}", self.to_string_index(*to)).into(),
            Type::FunctionType(FunctionType {
                params,
                return_type,
                is_vararg,
            }) => format!(
                "fn ({}) -> {}",
                self.to_string_list(params, *is_vararg),
                self.to_string_index(*return_type)
            )
            .into(),
            Type::TypeDefType(TypeDefType { ident, .. }) => ident.to_string().into(),
            Type::PrimitiveType(pty) => match pty {
                PrimitiveType::U32 => "u32".into(),
                PrimitiveType::I32 => "i32".into(),
                PrimitiveType::F32 => "f32".into(),
                PrimitiveType::BOOL => "bool".into(),
                PrimitiveType::CHAR => "char".into(),
                PrimitiveType::STR => "str".into(),
            },
            Type::Unknown => "<unknown>".into(),
        }
    }

    pub fn reference(&mut self, to: Index) -> Index {
        let ty = Type::RefType(RefType { to });
        self.insert(&ty)
    }

    pub fn dereference(&self, from: Index) -> Option<Index> {
        if let Type::RefType(RefType { to }) = &self.arena[from] {
            Some(*to)
        } else {
            None
        }
    }

    pub fn dereference_all(&self, from: Index) -> Index {
        let mut result = from;
        loop {
            match self.dereference(result) {
                Some(to) => {
                    result = to;
                }
                None => {
                    return result;
                }
            }
        }
    }

    pub fn insert(&mut self, ty: &Type) -> Index {
        let string = self.to_string_type(ty);
        if let Some(index) = self.type_strings.get(string.as_ref()) {
            *index
        } else {
            let index = self.arena.insert(ty.clone());
            self.type_strings.insert(string.into_owned(), index);
            index
        }
    }

    pub fn insert_ast_type(&mut self, ty: &AstType) -> Option<Index> {
        match ty {
            AstType::Path(ty) => {
                let ident = ty
                    .path
                    .segments
                    .iter()
                    .map(|s| s.value.as_str())
                    .collect::<Vec<_>>()
                    .join("::");
                self.type_strings.get(&ident).cloned()
            }
            AstType::Reference(ty) => {
                let to = self.insert_ast_type(&ty.to.value)?;
                Some(self.reference(to))
            }
            AstType::Tuple(ty) => {
                let types = ty
                    .types
                    .iter()
                    .map(|ty| self.insert_ast_type(&ty.value))
                    .collect::<Vec<_>>();
                if types.iter().any(|ty| ty.is_none()) {
                    None
                } else {
                    let types = types
                        .into_iter()
                        .map(|opt| opt.unwrap())
                        .collect::<Vec<_>>();
                    Some(self.insert(&Type::TupleType(TupleType { types })))
                }
            }
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
        let unknown = self.unknown();
        if a == unknown || b == unknown {
            false
        } else {
            a == b
        }
    }

    fn aggregate_layout<I, F, Builder>(
        arena: &Arena<Type>,
        layouts: &mut HashMap<Index, MemoryLayout>,
        bitness: Bitness,
        ty: Index,
        members: I,
        get_type: F,
    ) -> SizeAlign
    where
        I: IntoIterator,
        F: Fn(&I::Item) -> Index,
        Builder: MemoryLayoutBuilder,
    {
        let mut builder = Builder::new();
        layouts.insert(ty, MemoryLayout::incomplete());
        for member in members {
            let SizeAlign { size, align } =
                Self::layout::<Builder>(arena, layouts, bitness, get_type(&member));
            builder.member(size, align);
        }

        let layout = builder.build();
        let result = layout.size_align();
        layouts.insert(ty, layout);
        result
    }

    fn layout<Builder: MemoryLayoutBuilder>(
        arena: &Arena<Type>,
        layouts: &mut HashMap<Index, MemoryLayout>,
        bitness: Bitness,
        ty: Index,
    ) -> SizeAlign {
        match &arena[ty] {
            Type::TupleType(tuple_ty) => Self::aggregate_layout::<_, _, Builder>(
                arena,
                layouts,
                bitness,
                ty,
                &tuple_ty.types,
                |t| **t,
            ),
            Type::StructType(struct_ty) => Self::aggregate_layout::<_, _, Builder>(
                arena,
                layouts,
                bitness,
                ty,
                &struct_ty.members,
                |m| m.ty,
            ),
            Type::RefType(ref_ty) => {
                if arena[ref_ty.to] == Type::PrimitiveType(PrimitiveType::STR) {
                    SizeAlign::slice(bitness)
                } else {
                    SizeAlign::ptr(bitness)
                }
            }
            Type::FunctionType(_) => SizeAlign::ptr(bitness),
            Type::TypeDefType(typedef_ty) => {
                Self::layout::<Builder>(arena, layouts, bitness, typedef_ty.ty)
            }
            Type::PrimitiveType(primitive_ty) => primitive_ty.layout(bitness).size_align(),
            Type::Unknown => panic!("cannot layout unknown type"),
        }
    }

    pub fn compute_memory_layouts<Builder: MemoryLayoutBuilder>(&mut self, bitness: Bitness) {
        for (ty, _) in self.arena.iter() {
            if self.memory_layouts.contains_key(&ty) {
                continue;
            }
            Self::layout::<Builder>(&self.arena, &mut self.memory_layouts, bitness, ty);
        }
    }

    pub fn size_align(&self, ty: Index, bitness: Bitness) -> SizeAlign {
        let ty = self.canonicalize(ty);
        match &self.arena[ty] {
            Type::TupleType(_) | Type::StructType(_) => self
                .memory_layouts
                .get(&ty)
                .map(|l| l.size_align())
                .unwrap_or(SizeAlign::none()),
            Type::RefType(_) | Type::FunctionType(_) => SizeAlign::ptr(bitness),
            Type::PrimitiveType(primitive_ty) => primitive_ty.layout(bitness).size_align(),
            Type::TypeDefType(_) | Type::Unknown => SizeAlign::none(),
        }
    }

    pub fn kind(&self, ty: Index) -> TypeKind {
        let ty = self.canonicalize(ty);
        match &self.arena[ty] {
            Type::TupleType(_) | Type::StructType(_) => TypeKind::Memory,
            Type::RefType(_) | Type::FunctionType(_) => TypeKind::Integer,
            Type::TypeDefType(_) => unreachable!(),
            Type::PrimitiveType(primitive_ty) => match primitive_ty {
                PrimitiveType::U32
                | PrimitiveType::I32
                | PrimitiveType::BOOL
                | PrimitiveType::CHAR => TypeKind::Integer,
                PrimitiveType::F32 => TypeKind::Float,
                PrimitiveType::STR => TypeKind::Unknown,
            },
            Type::Unknown => TypeKind::Unknown,
        }
    }
}

impl TypeCollection {
    fn primitive_type(&self, ty: &Type) -> Index {
        assert!(
            matches!(ty, Type::TupleType(TupleType { types, .. }) if types.is_empty())
                || matches!(ty, Type::PrimitiveType(_) | Type::Unknown)
        );
        let ty_str = self.to_string_type(ty);
        self.type_strings.get(ty_str.as_ref()).cloned().unwrap()
    }

    pub fn unit(&self) -> Index {
        self.primitive_type(&TYPE_UNIT)
    }

    pub fn unknown(&self) -> Index {
        self.primitive_type(&TYPE_UNKNOWN)
    }

    pub fn bool(&self) -> Index {
        self.primitive_type(&TYPE_BOOL)
    }

    pub fn char(&self) -> Index {
        self.primitive_type(&TYPE_CHAR)
    }

    pub fn str(&self) -> Index {
        self.primitive_type(&TYPE_STR)
    }

    pub fn str_ref(&mut self) -> Index {
        self.reference(self.str())
    }

    pub fn u32(&self) -> Index {
        self.primitive_type(&TYPE_U32)
    }

    pub fn i32(&self) -> Index {
        self.primitive_type(&TYPE_I32)
    }

    pub fn f32(&self) -> Index {
        self.primitive_type(&TYPE_F32)
    }
}
