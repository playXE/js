#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u8)]
#[allow(non_camel_case_types)]
pub enum Opcode {
    OP_NOP = 0,
    OP_MOVE,
    OP_LOAD_CONSTANT,
    OP_LOAD_INT,
    OP_LOAD_TRUE,
    OP_LOAD_FALSE,
    OP_LOAD_UNDEF,
    OP_LOAD_NAN,
    OP_LOAD_FUNCTION,
    OP_LOAD_NULL,

    OP_LOOPHINT,
    OP_CALL,
    OP_NEW,
    OP_CALL_BUILTIN,
    OP_NEWARRAY,
    OP_NEWOBJECT,
    OP_RET,
    OP_JMP,
    OP_JMP_IF_TRUE,
    OP_JMP_IF_FALSE,
    OP_ADD,
    OP_SUB,
    OP_DIV,
    OP_MUL,
    OP_REM,
    OP_SHR,
    OP_SHL,
    OP_USHR,
    OP_OR,
    OP_AND,
    OP_XOR,
    OP_IN,
    OP_EQ,
    OP_STRICTEQ,
    OP_NEQ,
    OP_NSTRICTEQ,
    OP_GREATER,
    OP_GREATEREQ,
    OP_LESS,
    OP_LESSEQ,
    OP_INSTANCEOF,

    OP_TYPEOF,
    OP_NOT,
    OP_LOGICAL_NOT,
    OP_POS,
    OP_NEG,
    OP_THROW,
    OP_PUSH_CATCH,
    OP_POP_CATCH,

    OP_GET_BY_ID,
    OP_GET_BY_VAL,
    OP_PUT_BY_ID,
    OP_PUT_BY_VAL,

    OP_PUSH_ENV,
    OP_POP_ENV,
    OP_GET_ENV,
    OP_GET_VAR,
    OP_SET_VAR,
    OP_SET_GLOBAL,
    OP_GET_GLOBAL,
    OP_DECL_LET,
    OP_DECL_CONST,
    OP_LOAD_THIS,
    OP_YIELD,
    OP_NEWGENERATOR,

    /// stack.push(Spread::new(...stack.pop()));
    OP_SPREAD,

    OP_DELETE_VAR,
    OP_DELETE_BY_ID,
    OP_DELETE_BY_VAL,
    OP_GLOBALTHIS,
}

pub enum HirOpcode {
    Nop,
    Move(u8, u8),
    LoadConstant(u8, u32),
    LoadInt(u8, i32),
    LoadTrue(u8),
    LoadFalse(u8),
    LoadUndef(u8),
    LoadNaN(u8),
    LoadFunction(u8, u16),
    LoadNull(u8),

    LoopHint,
    /// R(A) = R(B)(R(A)..C)
    Call(u8, u8, u8),
    New(u8, u8, u8),
    CallBuiltin(u8, u8, u8, u32, u8),

    NewArray,
}
