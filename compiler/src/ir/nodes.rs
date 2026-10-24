use crate::parser::BinaryOp;
use crate::types::Type;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Register(pub usize);

impl fmt::Display for Register {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "%v{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub usize);

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "block_{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum IrOperand {
    ConstantInt(i64),
    ConstantLong(i64),
    ConstantDecimal(f64),
    ConstantBool(bool),
    ConstantString(String),
    Null,
    NullOf(Type),
    Register(Register, Type),
}

impl IrOperand {
    pub fn get_type(&self) -> Type {
        match self {
            IrOperand::ConstantInt(_) => Type::Int,
            IrOperand::ConstantLong(_) => Type::Long,
            IrOperand::ConstantDecimal(_) => Type::Decimal,
            IrOperand::ConstantBool(_) => Type::Bool,
            IrOperand::ConstantString(_) => Type::String,
            IrOperand::Null => Type::Nullable(Box::new(Type::Void)),
            IrOperand::NullOf(ty) => ty.clone(),
            IrOperand::Register(_, ty) => ty.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum IrInstruction {
    Alloca { dest: Register, ty: Type },
    Load { dest: Register, ptr: Register, ty: Type },
    Store { val: IrOperand, ptr: Register },
    GetField { dest: Register, ptr: Register, class_name: String, field_index: usize, field_ty: Type },
    StoreField { ptr: Register, class_name: String, field_index: usize, val: IrOperand },
    Binary {
        dest: Register,
        op: BinaryOp,
        left: IrOperand,
        right: IrOperand,
        operand_ty: Type,
        result_ty: Type,
    },
    CallMethod { dest: Option<Register>, class_name: String, method_name: String, receiver: IrOperand, args: Vec<IrOperand>, ret_ty: Type },
    CallRuntime { dest: Option<Register>, symbol: String, args: Vec<IrOperand>, ret_ty: Type },
    SpawnAsyncCall {
        dest: Register,
        callback: String,
        class_name: String,
        method_name: String,
        receiver: IrOperand,
        args: Vec<IrOperand>,
        result_ty: Type,
    },
    SpawnAsyncExpression {
        dest: Register,
        callback: String,
        class_name: String,
        method_name: String,
        captures: Vec<IrOperand>,
        result_ty: Type,
    },
    AwaitTask {
        dest: Register,
        error_dest: Register,
        task: IrOperand,
        result_ty: Type,
    },
    InstantiateClass { dest: Register, class_name: String, args: Vec<IrOperand> },
    Jump { target: BlockId },
    Branch { cond: IrOperand, then_block: BlockId, else_block: BlockId },
    Return { val: Option<IrOperand> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct BasicBlock {
    pub id: BlockId,
    pub instructions: Vec<IrInstruction>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrClass {
    pub name: String,
    pub fields: Vec<(String, Type)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrFunction {
    pub class_name: String,
    pub name: String,
    pub receiver: (Register, Type), // this pointer
    pub params: Vec<(Register, Type)>,
    pub return_type: Type,
    pub is_async: bool,
    pub blocks: Vec<BasicBlock>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrProgram {
    pub classes: Vec<IrClass>,
    pub functions: Vec<IrFunction>,
}
