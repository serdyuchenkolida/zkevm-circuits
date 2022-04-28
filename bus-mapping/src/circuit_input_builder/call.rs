use eth_types::{evm_types::OpcodeId, Address, Hash, Word, U256};

use crate::{exec_trace::OperationRef, Error};

use super::CodeSource;

/// Type of a *CALL*/CREATE* Function.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CallKind {
    /// CALL
    Call,
    /// CALLCODE
    CallCode,
    /// DELEGATECALL
    DelegateCall,
    /// STATICCALL
    StaticCall,
    /// CREATE
    Create,
    /// CREATE2
    Create2,
}

impl CallKind {
    fn is_create(&self) -> bool {
        matches!(self, Self::Create | Self::Create2)
    }
}

impl Default for CallKind {
    fn default() -> Self {
        Self::Call
    }
}

impl TryFrom<OpcodeId> for CallKind {
    type Error = Error;

    fn try_from(op: OpcodeId) -> Result<Self, Self::Error> {
        Ok(match op {
            OpcodeId::CALL => CallKind::Call,
            OpcodeId::CALLCODE => CallKind::CallCode,
            OpcodeId::DELEGATECALL => CallKind::DelegateCall,
            OpcodeId::STATICCALL => CallKind::StaticCall,
            OpcodeId::CREATE => CallKind::Create,
            OpcodeId::CREATE2 => CallKind::Create2,
            _ => return Err(Error::OpcodeIdNotCallType),
        })
    }
}

/// Circuit Input related to an Ethereum Call
#[derive(Clone, Debug, Default)]
pub struct Call {
    /// Unique call identifier within the Block.
    pub call_id: usize,
    /// Caller's id.
    pub caller_id: usize,
    /// Type of call
    pub kind: CallKind,
    /// This call is being executed without write access (STATIC)
    pub is_static: bool,
    /// This call generated implicity by a Transaction.
    pub is_root: bool,
    /// This call is persistent or call stack reverts at some point
    pub is_persistent: bool,
    /// This call ends successfully or not
    pub is_success: bool,
    /// This rw_counter at the end of reversion
    pub rw_counter_end_of_reversion: usize,
    /// Address of caller
    pub caller_address: Address,
    /// Address where this call is being executed
    pub address: Address,
    /// Code Source
    pub code_source: CodeSource,
    /// Code Hash
    pub code_hash: Hash,
    /// Depth
    pub depth: usize,
    /// Value
    pub value: Word,
    /// Call data offset
    pub call_data_offset: u64,
    /// Call data length
    pub call_data_length: u64,
    /// Return data offset
    pub return_data_offset: u64,
    /// Return data length
    pub return_data_length: u64,
}

impl Call {
    /// This call is root call with tx.to == null, or op == CREATE or op ==
    /// CREATE2
    pub fn is_create(&self) -> bool {
        self.kind.is_create()
    }
}

/// Context of a [`Call`].
#[derive(Debug, Default)]
pub struct CallContext {
    /// Index of call
    pub index: usize,
    /// Reversible Write Counter tracks the number of write operations in the
    /// call. It is incremented when a subcall in this call succeeds by the
    /// number of successful writes in the subcall.
    pub reversible_write_counter: usize,
    /// Call data (copy of tx input or caller's
    /// memory[call_data_offset..call_data_offset + call_data_length])
    pub call_data: Vec<u8>,
}

/// A reversion group is the collection of calls and the operations which are
/// [`Operation::reversible`] that happened in them, that will be reverted at
/// once when the call that initiated this reversion group eventually ends with
/// failure (and thus reverts).
#[derive(Debug, Default)]
pub struct ReversionGroup {
    /// List of `index` and `reversible_write_counter_offset` of calls belong to
    /// this group. `reversible_write_counter_offset` is the number of
    /// reversible operations that have happened before the call within the
    /// same reversion group.
    calls: Vec<(usize, usize)>,
    /// List of `step_index` and `OperationRef` that have been done in this
    /// group.
    op_refs: Vec<(usize, OperationRef)>,
}
/// Auxiliary data for CopyToMemory internal state.
#[derive(Clone, Copy, Debug)]
pub struct CopyToMemoryAuxData {
    /// Source start address
    pub src_addr: u64,
    /// Destination address
    pub dst_addr: u64,
    /// Bytes left
    pub bytes_left: u64,
    /// Source end address
    pub src_addr_end: u64,
    /// Indicate if copy from transaction call data
    pub from_tx: bool,
}

/// Auxiliary data for CopyCodeToMemory internal state.
#[derive(Clone, Copy, Debug)]
pub struct CopyCodeToMemoryAuxData {
    /// Source start address
    pub src_addr: u64,
    /// Destination address
    pub dst_addr: u64,
    /// Bytes left
    pub bytes_left: u64,
    /// Source end address
    pub src_addr_end: u64,
    /// Hash of the bytecode to be copied
    pub code_source: U256,
}

/// Auxiliary data of Execution step
#[derive(Clone, Copy, Debug)]
pub enum StepAuxiliaryData {
    /// Auxiliary data of Copy To Memory
    CopyToMemory(CopyToMemoryAuxData),
    /// Auxiliary data of Copy Code To Memory
    CopyCodeToMemory(CopyCodeToMemoryAuxData),
}
