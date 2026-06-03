//! Bytecode instruction set for the VM.
//!
//! Each instruction is encoded as a variable-length byte sequence:
//! - Byte 0: opcode
//! - Bytes 1..n: operands (if any)
//!
//! Operand encoding:
//! - `u8` operands: 1 byte
//! - `u16` operands: 2 bytes, big-endian
//!
//! The maximum constant pool size is 65536 entries (u16 index).
//! The maximum number of local slots per frame is 256 (u8 index).

/// Single-byte opcode identifiers.
///
/// Opcodes are grouped by function. Within each group, values are
/// assigned sequentially for compact encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Opcode {
    // ─── Stack operations (0x00–0x0F) ──────────────────────────────
    /// Push constant from pool onto stack. Operand: u16 index.
    OpConstant = 0x00,
    /// Pop and discard top of stack.
    OpPop = 0x01,
    /// Duplicate top of stack.
    OpDup = 0x02,
    /// Swap top two stack values.
    OpSwap = 0x03,

    // ─── Variable access (0x10–0x1F) ───────────────────────────────
    /// Push local variable by slot index. Operand: u8.
    OpGetLocal = 0x10,
    /// Pop and store into local slot. Operand: u8.
    OpSetLocal = 0x11,
    /// Push global variable by name constant index. Operand: u16.
    OpGetGlobal = 0x12,
    /// Pop and store into global. Operand: u16.
    OpSetGlobal = 0x13,
    /// Push builtin function by index. Operand: u8.
    OpGetBuiltin = 0x14,

    // ─── Arithmetic & comparison (0x20–0x2F) ──────────────────────
    OpAdd = 0x20,
    OpSubtract = 0x21,
    OpMultiply = 0x22,
    OpDivide = 0x23,
    OpModulo = 0x24,
    OpEqual = 0x25,
    OpNotEqual = 0x26,
    OpLessThan = 0x27,
    OpGreaterThan = 0x28,
    OpLessEqual = 0x29,
    OpGreaterEqual = 0x2A,
    OpNot = 0x2B,
    OpNegate = 0x2C,
    /// Get length of array/string/hash. Stack: collection → length
    OpGetLen = 0x2D,

    // ─── Control flow (0x30–0x3F) ─────────────────────────────────
    /// Unconditional forward jump. Operand: u16 offset.
    OpJump = 0x30,
    /// Unconditional backward jump. Operand: u16 offset.
    OpJumpBackward = 0x31,
    /// Pop; jump forward if falsey. Operand: u16 offset.
    OpJumpIfFalse = 0x32,
    /// Pop; jump forward if truthy. Operand: u16 offset.
    OpJumpIfTruthy = 0x33,
    /// Pop; jump forward if falsey (short-circuit &&, ||). Operand: u16.
    OpPopJumpIfFalse = 0x34,

    // ─── Function calls (0x40–0x4F) ────────────────────────────────
    /// Call user-defined function. Operand: u8 arg count.
    OpCall = 0x40,
    /// Call builtin function. Operand: u8 arg count.
    OpCallBuiltin = 0x41,
    /// Mark top of stack as return value and unwind.
    OpReturnValue = 0x42,
    /// Create closure from sub-chunk. Operands: u8 param count, u16 chunk offset.
    OpClosure = 0x43,
    /// Call async function. Operand: u8 arg count.
    OpCallAsync = 0x44,
    /// Await a future on top of stack.
    OpAwait = 0x45,

    // ─── Collections (0x50–0x5F) ───────────────────────────────────
    /// Build array from N stack elements. Operand: u16 count.
    OpBuildArray = 0x50,
    /// Build hash from N key-value pairs. Operand: u16 pair count.
    OpBuildHash = 0x51,
    /// Index into collection: `collection, index → value`.
    OpIndex = 0x52,
    /// Set collection element: `collection, index, value →`.
    OpSetIndex = 0x53,

    // ─── Structs & methods (0x60–0x6F) ─────────────────────────────
    /// Build struct from N field values. Operand: u8 field count.
    OpBuildStruct = 0x60,
    /// Access struct field: `struct → value`.
    OpGetField = 0x61,
    /// Set struct field: `struct, value →`.
    OpSetField = 0x62,
    /// Call struct method. Operand: u8 arg count.
    OpCallMethod = 0x63,
    /// Build enum struct variant. Operand: u8 field count.
    OpBuildEnumStruct = 0x64,
    /// Build enum tuple variant. Operand: u8 arg count.
    OpBuildEnumTuple = 0x65,
    /// Check if stack top matches enum variant. Operands: u16 enum_name_idx, u16 variant_name_idx.
    OpMatchEnum = 0x66,
    /// Destructure enum tuple field. Operand: u8 field index.
    OpDestructureEnumTuple = 0x67,
    /// Destructure enum struct field. Operand: u16 field_name_idx.
    OpDestructureEnumStruct = 0x68,

    // ─── Exception handling (0x70–0x7F) ────────────────────────────
    /// Throw value on top of stack.
    OpThrow = 0x70,
    /// Register catch/finally handler. Operands: u16 catch addr, u16 finally addr.
    OpPushCatch = 0x71,
    /// Remove top exception handler.
    OpPopCatch = 0x72,
    /// Register finally block. Operand: u16 finally addr.
    OpPushFinally = 0x73,
    /// Resume after finally (re-throw or continue).
    OpEndFinally = 0x74,

    // ─── Loop control (0x80–0x8F) ─────────────────────────────────
    /// Break out of loop. Operand: u16 exit address.
    OpBreak = 0x80,
    /// Continue to next iteration. Operand: u16 condition address.
    OpContinue = 0x81,

    // ─── Modules (0x90–0x9F) ───────────────────────────────────────
    /// Import module by path constant index. Operand: u16.
    OpImportModule = 0x90,
    /// Get named export from module: `module → obj`.
    OpGetExport = 0x91,
}

impl Opcode {
    /// Decode an opcode from a raw byte.
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x00 => Some(Opcode::OpConstant),
            0x01 => Some(Opcode::OpPop),
            0x02 => Some(Opcode::OpDup),
            0x03 => Some(Opcode::OpSwap),
            0x10 => Some(Opcode::OpGetLocal),
            0x11 => Some(Opcode::OpSetLocal),
            0x12 => Some(Opcode::OpGetGlobal),
            0x13 => Some(Opcode::OpSetGlobal),
            0x14 => Some(Opcode::OpGetBuiltin),
            0x20 => Some(Opcode::OpAdd),
            0x21 => Some(Opcode::OpSubtract),
            0x22 => Some(Opcode::OpMultiply),
            0x23 => Some(Opcode::OpDivide),
            0x24 => Some(Opcode::OpModulo),
            0x25 => Some(Opcode::OpEqual),
            0x26 => Some(Opcode::OpNotEqual),
            0x27 => Some(Opcode::OpLessThan),
            0x28 => Some(Opcode::OpGreaterThan),
            0x29 => Some(Opcode::OpLessEqual),
            0x2A => Some(Opcode::OpGreaterEqual),
            0x2B => Some(Opcode::OpNot),
            0x2C => Some(Opcode::OpNegate),
            0x2D => Some(Opcode::OpGetLen),
            0x30 => Some(Opcode::OpJump),
            0x31 => Some(Opcode::OpJumpBackward),
            0x32 => Some(Opcode::OpJumpIfFalse),
            0x33 => Some(Opcode::OpJumpIfTruthy),
            0x34 => Some(Opcode::OpPopJumpIfFalse),
            0x40 => Some(Opcode::OpCall),
            0x41 => Some(Opcode::OpCallBuiltin),
            0x42 => Some(Opcode::OpReturnValue),
            0x43 => Some(Opcode::OpClosure),
            0x44 => Some(Opcode::OpCallAsync),
            0x45 => Some(Opcode::OpAwait),
            0x50 => Some(Opcode::OpBuildArray),
            0x51 => Some(Opcode::OpBuildHash),
            0x52 => Some(Opcode::OpIndex),
            0x53 => Some(Opcode::OpSetIndex),
            0x60 => Some(Opcode::OpBuildStruct),
            0x61 => Some(Opcode::OpGetField),
            0x62 => Some(Opcode::OpSetField),
            0x63 => Some(Opcode::OpCallMethod),
            0x64 => Some(Opcode::OpBuildEnumStruct),
            0x65 => Some(Opcode::OpBuildEnumTuple),
            0x66 => Some(Opcode::OpMatchEnum),
            0x67 => Some(Opcode::OpDestructureEnumTuple),
            0x68 => Some(Opcode::OpDestructureEnumStruct),
            0x70 => Some(Opcode::OpThrow),
            0x71 => Some(Opcode::OpPushCatch),
            0x72 => Some(Opcode::OpPopCatch),
            0x73 => Some(Opcode::OpPushFinally),
            0x74 => Some(Opcode::OpEndFinally),
            0x80 => Some(Opcode::OpBreak),
            0x81 => Some(Opcode::OpContinue),
            0x90 => Some(Opcode::OpImportModule),
            0x91 => Some(Opcode::OpGetExport),
            _ => None,
        }
    }

    /// Number of operand bytes for this opcode.
    pub fn operand_width(self) -> usize {
        match self {
            Opcode::OpConstant => 2,
            Opcode::OpPop | Opcode::OpDup | Opcode::OpSwap => 0,
            Opcode::OpGetLocal | Opcode::OpSetLocal | Opcode::OpGetBuiltin => 1,
            Opcode::OpGetGlobal | Opcode::OpSetGlobal => 2,
            Opcode::OpAdd
            | Opcode::OpSubtract
            | Opcode::OpMultiply
            | Opcode::OpDivide
            | Opcode::OpModulo
            | Opcode::OpEqual
            | Opcode::OpNotEqual
            | Opcode::OpLessThan
            | Opcode::OpGreaterThan
            | Opcode::OpLessEqual
            | Opcode::OpGreaterEqual
            | Opcode::OpNot
            | Opcode::OpNegate
            | Opcode::OpGetLen => 0,
            Opcode::OpJump
            | Opcode::OpJumpBackward
            | Opcode::OpJumpIfFalse
            | Opcode::OpJumpIfTruthy
            | Opcode::OpPopJumpIfFalse => 2,
            Opcode::OpCall | Opcode::OpCallBuiltin | Opcode::OpCallAsync => 1,
            Opcode::OpReturnValue => 0,
            Opcode::OpClosure => 3, // u8 params + u16 chunk_offset
            Opcode::OpAwait => 0,
            Opcode::OpBuildArray | Opcode::OpBuildHash => 2,
            Opcode::OpIndex | Opcode::OpSetIndex => 0,
            Opcode::OpBuildStruct => 1,
            Opcode::OpBuildEnumStruct => 1,
            Opcode::OpBuildEnumTuple => 1,
            Opcode::OpMatchEnum => 4,
            Opcode::OpDestructureEnumTuple => 1,
            Opcode::OpDestructureEnumStruct => 2,
            Opcode::OpGetField | Opcode::OpSetField => 0,
            Opcode::OpCallMethod => 1,
            Opcode::OpThrow => 0,
            Opcode::OpPushCatch => 4, // u16 catch + u16 finally
            Opcode::OpPopCatch => 0,
            Opcode::OpPushFinally => 2,
            Opcode::OpEndFinally => 0,
            Opcode::OpBreak | Opcode::OpContinue => 2,
            Opcode::OpImportModule => 2,
            Opcode::OpGetExport => 0,
        }
    }
}

/// A decoded instruction with its operands resolved.
///
/// Used by the VM's dispatch loop and the debug disassembler.
#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    Constant(u16),
    Pop,
    Dup,
    Swap,
    GetLocal(u8),
    SetLocal(u8),
    GetGlobal(u16),
    SetGlobal(u16),
    GetBuiltin(u8),
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Equal,
    NotEqual,
    LessThan,
    GreaterThan,
    LessEqual,
    GreaterEqual,
    Not,
    Negate,
    GetLen,
    Jump(u16),
    JumpBackward(u16),
    JumpIfFalse(u16),
    JumpIfTruthy(u16),
    PopJumpIfFalse(u16),
    Call(u8),
    CallBuiltin(u8),
    ReturnValue,
    Closure { param_count: u8, chunk_offset: u16 },
    CallAsync(u8),
    Await,
    BuildArray(u16),
    BuildHash(u16),
    Index,
    SetIndex,
    BuildStruct(u8),
    BuildEnumStruct(u8),
    BuildEnumTuple(u8),
    MatchEnum { enum_name_idx: u16, variant_name_idx: u16 },
    DestructureEnumTuple(u8),
    DestructureEnumStruct(u16),
    GetField,
    SetField,
    CallMethod(u8),
    Throw,
    PushCatch { catch_addr: u16, finally_addr: u16 },
    PopCatch,
    PushFinally(u16),
    EndFinally,
    Break(u16),
    Continue(u16),
    ImportModule(u16),
    GetExport,
}

/// Encode a single instruction into a byte vector.
///
/// Appends the opcode byte followed by operands in big-endian order.
pub(crate) fn encode_instruction(code: &mut Vec<u8>, instr: Instruction) {
    match instr {
        Instruction::Constant(idx) => {
            code.push(Opcode::OpConstant as u8);
            code.extend_from_slice(&idx.to_be_bytes());
        }
        Instruction::Pop => code.push(Opcode::OpPop as u8),
        Instruction::Dup => code.push(Opcode::OpDup as u8),
        Instruction::Swap => code.push(Opcode::OpSwap as u8),
        Instruction::GetLocal(slot) => {
            code.push(Opcode::OpGetLocal as u8);
            code.push(slot);
        }
        Instruction::SetLocal(slot) => {
            code.push(Opcode::OpSetLocal as u8);
            code.push(slot);
        }
        Instruction::GetGlobal(idx) => {
            code.push(Opcode::OpGetGlobal as u8);
            code.extend_from_slice(&idx.to_be_bytes());
        }
        Instruction::SetGlobal(idx) => {
            code.push(Opcode::OpSetGlobal as u8);
            code.extend_from_slice(&idx.to_be_bytes());
        }
        Instruction::GetBuiltin(idx) => {
            code.push(Opcode::OpGetBuiltin as u8);
            code.push(idx);
        }
        Instruction::Add => code.push(Opcode::OpAdd as u8),
        Instruction::Subtract => code.push(Opcode::OpSubtract as u8),
        Instruction::Multiply => code.push(Opcode::OpMultiply as u8),
        Instruction::Divide => code.push(Opcode::OpDivide as u8),
        Instruction::Modulo => code.push(Opcode::OpModulo as u8),
        Instruction::Equal => code.push(Opcode::OpEqual as u8),
        Instruction::NotEqual => code.push(Opcode::OpNotEqual as u8),
        Instruction::LessThan => code.push(Opcode::OpLessThan as u8),
        Instruction::GreaterThan => code.push(Opcode::OpGreaterThan as u8),
        Instruction::LessEqual => code.push(Opcode::OpLessEqual as u8),
        Instruction::GreaterEqual => code.push(Opcode::OpGreaterEqual as u8),
        Instruction::Not => code.push(Opcode::OpNot as u8),
        Instruction::Negate => code.push(Opcode::OpNegate as u8),
        Instruction::GetLen => code.push(Opcode::OpGetLen as u8),
        Instruction::Jump(offset) => {
            code.push(Opcode::OpJump as u8);
            code.extend_from_slice(&offset.to_be_bytes());
        }
        Instruction::JumpBackward(offset) => {
            code.push(Opcode::OpJumpBackward as u8);
            code.extend_from_slice(&offset.to_be_bytes());
        }
        Instruction::JumpIfFalse(offset) => {
            code.push(Opcode::OpJumpIfFalse as u8);
            code.extend_from_slice(&offset.to_be_bytes());
        }
        Instruction::JumpIfTruthy(offset) => {
            code.push(Opcode::OpJumpIfTruthy as u8);
            code.extend_from_slice(&offset.to_be_bytes());
        }
        Instruction::PopJumpIfFalse(offset) => {
            code.push(Opcode::OpPopJumpIfFalse as u8);
            code.extend_from_slice(&offset.to_be_bytes());
        }
        Instruction::Call(argc) => {
            code.push(Opcode::OpCall as u8);
            code.push(argc);
        }
        Instruction::CallBuiltin(argc) => {
            code.push(Opcode::OpCallBuiltin as u8);
            code.push(argc);
        }
        Instruction::ReturnValue => code.push(Opcode::OpReturnValue as u8),
        Instruction::Closure {
            param_count,
            chunk_offset,
        } => {
            code.push(Opcode::OpClosure as u8);
            code.push(param_count);
            code.extend_from_slice(&chunk_offset.to_be_bytes());
        }
        Instruction::CallAsync(argc) => {
            code.push(Opcode::OpCallAsync as u8);
            code.push(argc);
        }
        Instruction::Await => code.push(Opcode::OpAwait as u8),
        Instruction::BuildArray(count) => {
            code.push(Opcode::OpBuildArray as u8);
            code.extend_from_slice(&count.to_be_bytes());
        }
        Instruction::BuildHash(count) => {
            code.push(Opcode::OpBuildHash as u8);
            code.extend_from_slice(&count.to_be_bytes());
        }
        Instruction::Index => code.push(Opcode::OpIndex as u8),
        Instruction::SetIndex => code.push(Opcode::OpSetIndex as u8),
        Instruction::BuildStruct(count) => {
            code.push(Opcode::OpBuildStruct as u8);
            code.push(count);
        }
        Instruction::BuildEnumStruct(count) => {
            code.push(Opcode::OpBuildEnumStruct as u8);
            code.push(count);
        }
        Instruction::BuildEnumTuple(count) => {
            code.push(Opcode::OpBuildEnumTuple as u8);
            code.push(count);
        }
        Instruction::MatchEnum { enum_name_idx, variant_name_idx } => {
            code.push(Opcode::OpMatchEnum as u8);
            code.extend_from_slice(&enum_name_idx.to_be_bytes());
            code.extend_from_slice(&variant_name_idx.to_be_bytes());
        }
        Instruction::DestructureEnumTuple(idx) => {
            code.push(Opcode::OpDestructureEnumTuple as u8);
            code.push(idx);
        }
        Instruction::DestructureEnumStruct(idx) => {
            code.push(Opcode::OpDestructureEnumStruct as u8);
            code.extend_from_slice(&idx.to_be_bytes());
        }
        Instruction::GetField => code.push(Opcode::OpGetField as u8),
        Instruction::SetField => code.push(Opcode::OpSetField as u8),
        Instruction::CallMethod(argc) => {
            code.push(Opcode::OpCallMethod as u8);
            code.push(argc);
        }
        Instruction::Throw => code.push(Opcode::OpThrow as u8),
        Instruction::PushCatch {
            catch_addr,
            finally_addr,
        } => {
            code.push(Opcode::OpPushCatch as u8);
            code.extend_from_slice(&catch_addr.to_be_bytes());
            code.extend_from_slice(&finally_addr.to_be_bytes());
        }
        Instruction::PopCatch => code.push(Opcode::OpPopCatch as u8),
        Instruction::PushFinally(addr) => {
            code.push(Opcode::OpPushFinally as u8);
            code.extend_from_slice(&addr.to_be_bytes());
        }
        Instruction::EndFinally => code.push(Opcode::OpEndFinally as u8),
        Instruction::Break(addr) => {
            code.push(Opcode::OpBreak as u8);
            code.extend_from_slice(&addr.to_be_bytes());
        }
        Instruction::Continue(addr) => {
            code.push(Opcode::OpContinue as u8);
            code.extend_from_slice(&addr.to_be_bytes());
        }
        Instruction::ImportModule(idx) => {
            code.push(Opcode::OpImportModule as u8);
            code.extend_from_slice(&idx.to_be_bytes());
        }
        Instruction::GetExport => code.push(Opcode::OpGetExport as u8),
    }
}
