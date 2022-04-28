//! Transaction & TransactionContext utility module.

use std::collections::BTreeMap;

use eth_types::{Address, GethExecTrace, Word};

use crate::{
    state_db::{CodeDB, StateDB},
    Error,
};

use super::{call::ReversionGroup, Call, CallContext, CallKind, CodeSource, ExecStep};

#[derive(Debug)]
/// Context of a [`Transaction`] which can mutate in an [`ExecStep`].
pub struct TransactionContext {
    /// Unique identifier of transaction of the block. The value is `index + 1`.
    id: usize,
    /// Identifier if this transaction is last one of the block or not.
    is_last_tx: bool,
    /// Call stack.
    calls: Vec<CallContext>,
    /// Call `is_success` indexed by `call_index`.
    call_is_success: Vec<bool>,
    /// Reversion groups by failure calls. We keep the reversion groups in a
    /// stack because it's possible to encounter a revert within a revert,
    /// and in such case, we must only process the reverted operation once:
    /// in the inner most revert (which we track with the last element in
    /// the reversion groups stack), and skip it in the outer revert.
    reversion_groups: Vec<ReversionGroup>,
}

impl TransactionContext {
    /// Create a new Self.
    pub fn new(
        eth_tx: &eth_types::Transaction,
        geth_trace: &GethExecTrace,
        is_last_tx: bool,
    ) -> Result<Self, Error> {
        // Iterate over geth_trace to inspect and collect each call's is_success, which
        // is at the top of stack at the step after a call.
        let call_is_success = {
            let mut call_is_success_map = BTreeMap::new();
            let mut call_indices = Vec::new();
            for (index, geth_step) in geth_trace.struct_logs.iter().enumerate() {
                if let Some(geth_next_step) = geth_trace.struct_logs.get(index + 1) {
                    // Dive into call
                    if geth_step.depth + 1 == geth_next_step.depth {
                        call_indices.push(index);
                    // Emerge from call
                    } else if geth_step.depth - 1 == geth_next_step.depth {
                        let is_success = !geth_next_step.stack.last()?.is_zero();
                        call_is_success_map.insert(call_indices.pop().unwrap(), is_success);
                    // Callee with empty code
                    } else if CallKind::try_from(geth_step.op).is_ok() {
                        let is_success = !geth_next_step.stack.last()?.is_zero();
                        call_is_success_map.insert(index, is_success);
                    }
                }
            }

            std::iter::once(!geth_trace.failed)
                .chain(call_is_success_map.into_values())
                .collect()
        };

        let mut tx_ctx = Self {
            id: eth_tx
                .transaction_index
                .ok_or(Error::EthTypeError(eth_types::Error::IncompleteBlock))?
                .as_u64() as usize
                + 1,
            is_last_tx,
            call_is_success,
            calls: Vec::new(),
            reversion_groups: Vec::new(),
        };
        tx_ctx.push_call_ctx(0, eth_tx.input.to_vec());

        Ok(tx_ctx)
    }

    /// Return id of the this transaction.
    pub fn id(&self) -> usize {
        self.id
    }

    /// Return is_last_tx of the this transaction.
    pub fn is_last_tx(&self) -> bool {
        self.is_last_tx
    }

    /// Return the calls in this transaction.
    pub fn calls(&self) -> &[CallContext] {
        &self.calls
    }

    /// Return a collection of all of the [`ReversionGroup`]s inside of the Tx
    /// Context.
    pub fn reversion_groups(&self) -> &Vec<ReversionGroup> {
        &self.reversion_groups
    }

    /// Return the index of the current call (the last call in the call stack).
    pub(crate) fn call_index(&self) -> Result<usize, Error> {
        self.calls
            .last()
            .ok_or(Error::InvalidGethExecTrace(
                "Call stack is empty but call is used",
            ))
            .map(|call| call.index)
    }

    pub(crate) fn call_ctx(&self) -> Result<&CallContext, Error> {
        self.calls.last().ok_or(Error::InvalidGethExecTrace(
            "Call stack is empty but call is used",
        ))
    }

    pub(crate) fn call_ctx_mut(&mut self) -> Result<&mut CallContext, Error> {
        self.calls.last_mut().ok_or(Error::InvalidGethExecTrace(
            "Call stack is empty but call is used",
        ))
    }

    /// Push a new call context and its index into the call stack.
    pub(crate) fn push_call_ctx(&mut self, call_idx: usize, call_data: Vec<u8>) {
        if !self.call_is_success[call_idx] {
            self.reversion_groups.push(ReversionGroup {
                calls: vec![(call_idx, 0)],
                op_refs: Vec::new(),
            })
        } else if let Some(reversion_group) = self.reversion_groups.last_mut() {
            let caller_ctx = self.calls.last().expect("calls should not be empty");
            let caller_reversible_write_counter = self
                .calls
                .last()
                .expect("calls should not be empty")
                .reversible_write_counter;
            let caller_reversible_write_counter_offset = reversion_group
                .calls
                .iter()
                .find(|(call_idx, _)| *call_idx == caller_ctx.index)
                .expect("calls should not be empty")
                .1;
            reversion_group.calls.push((
                call_idx,
                caller_reversible_write_counter + caller_reversible_write_counter_offset,
            ));
        }

        self.calls.push(CallContext {
            index: call_idx,
            reversible_write_counter: 0,
            call_data,
        });
    }

    /// Pop the last entry in the call stack.
    pub(crate) fn pop_call_ctx(&mut self) {
        let call = self.calls.pop().expect("calls should not be empty");
        // Accumulate reversible_write_counter if call is success
        if self.call_is_success[call.index] {
            if let Some(caller) = self.calls.last_mut() {
                caller.reversible_write_counter += call.reversible_write_counter;
            }
        }
    }
}

#[derive(Debug, Clone)]
/// Result of the parsing of an Ethereum Transaction.
pub struct Transaction {
    /// Nonce
    pub nonce: u64,
    /// Gas
    pub gas: u64,
    /// Gas price
    pub gas_price: Word,
    /// From / Caller Address
    pub from: Address,
    /// To / Callee Address
    pub to: Address,
    /// Value
    pub value: Word,
    /// Input / Call Data
    pub input: Vec<u8>,
    /// Calls made in the transaction
    calls: Vec<Call>,
    /// Execution steps
    steps: Vec<ExecStep>,
}

impl Transaction {
    /// Create a new Self.
    pub fn new(
        call_id: usize,
        sdb: &StateDB,
        code_db: &mut CodeDB,
        eth_tx: &eth_types::Transaction,
        is_success: bool,
    ) -> Result<Self, Error> {
        let (found, _) = sdb.get_account(&eth_tx.from);
        if !found {
            return Err(Error::AccountNotFound(eth_tx.from));
        }

        let call = if let Some(address) = eth_tx.to {
            // Contract Call / Transfer
            let (found, account) = sdb.get_account(&address);
            if !found {
                return Err(Error::AccountNotFound(address));
            }
            let code_hash = account.code_hash;
            Call {
                call_id,
                kind: CallKind::Call,
                is_root: true,
                is_persistent: is_success,
                is_success,
                caller_address: eth_tx.from,
                address,
                code_source: CodeSource::Address(address),
                code_hash,
                depth: 1,
                value: eth_tx.value,
                call_data_length: eth_tx.input.as_ref().len() as u64,
                ..Default::default()
            }
        } else {
            // Contract creation
            let code_hash = code_db.insert(eth_tx.input.to_vec());
            Call {
                call_id,
                kind: CallKind::Create,
                is_root: true,
                is_persistent: is_success,
                is_success,
                caller_address: eth_tx.from,
                address: get_contract_address(eth_tx.from, eth_tx.nonce),
                code_source: CodeSource::Tx,
                code_hash,
                depth: 1,
                value: eth_tx.value,
                ..Default::default()
            }
        };

        Ok(Self {
            nonce: eth_tx.nonce.as_u64(),
            gas: eth_tx.gas.as_u64(),
            gas_price: eth_tx.gas_price.unwrap_or_default(),
            from: eth_tx.from,
            to: eth_tx.to.unwrap_or_default(),
            value: eth_tx.value,
            input: eth_tx.input.to_vec(),
            calls: vec![call],
            steps: Vec::new(),
        })
    }

    /// Wether this [`Transaction`] is a create one
    pub fn is_create(&self) -> bool {
        self.calls[0].is_create()
    }

    /// Return the list of execution steps of this transaction.
    pub fn steps(&self) -> &[ExecStep] {
        &self.steps
    }

    #[cfg(test)]
    pub fn steps_mut(&mut self) -> &mut Vec<ExecStep> {
        &mut self.steps
    }

    /// Return the list of calls of this transaction.
    pub fn calls(&self) -> &[Call] {
        &self.calls
    }

    fn push_call(&mut self, call: Call) {
        self.calls.push(call);
    }
}
