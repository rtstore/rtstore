//
// kv_store.rs
// Copyright (C) 2022 db3.network Author imotai <codego.me@gmail.com>
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//    http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//

use super::key::Key;
use db3_error::{DB3Error, Result};
use db3_proto::db3_base_proto::Units;
use db3_proto::db3_mutation_proto::{KvPair, Mutation, MutationAction};
use db3_types::cost;
use ethereum_types::Address as AccountAddress;
use merk::{BatchEntry, Merk, Op};
use std::pin::Pin;
pub struct KvStore {}

impl KvStore {
    pub fn new() -> Self {
        Self {}
    }

    fn convert(
        kp: &KvPair,
        account_addr: &AccountAddress,
        ns: &[u8],
    ) -> Result<(BatchEntry, usize)> {
        let key = Key(*account_addr, ns, kp.key.as_ref());
        let encoded_key = key.encode()?;
        let action = MutationAction::from_i32(kp.action);
        match action {
            Some(MutationAction::InsertKv) => {
                //TODO avoid copying operation
                let total_in_bytes = encoded_key.len() + kp.value.len();
                Ok(((encoded_key, Op::Put(kp.value.to_vec())), total_in_bytes))
            }
            Some(MutationAction::DeleteKv) => Ok(((encoded_key, Op::Delete), 0)),
            None => Err(DB3Error::ApplyMutationError(
                "invalid action type".to_string(),
            )),
        }
    }

    pub fn apply(
        db: Pin<&mut Merk>,
        account_addr: &AccountAddress,
        mutation: &Mutation,
    ) -> Result<(Units, usize)> {
        let ns = mutation.ns.as_ref();
        //TODO avoid copying operation
        let mut ordered_kv_pairs = mutation.kv_pairs.to_vec();
        ordered_kv_pairs.sort_by(|a, b| a.key.cmp(&b.key));
        let mut entries: Vec<BatchEntry> = Vec::new();
        let mut total_in_bytes: usize = 0;
        for kv in ordered_kv_pairs {
            let (batch_entry, bytes) = Self::convert(&kv, account_addr, ns)?;
            total_in_bytes += bytes;
            entries.push(batch_entry);
        }
        let gas = cost::estimate_gas(mutation);
        unsafe {
            Pin::get_unchecked_mut(db)
                .apply(&entries, &[])
                .map_err(|e| DB3Error::ApplyMutationError(format!("{}", e)))?;
        }
        Ok((gas, total_in_bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use db3_base::get_a_static_address;
    use db3_proto::db3_base_proto::{ChainId, ChainRole};
    use std::boxed::Box;
    use tempdir::TempDir;
    #[test]
    fn it_apply_mutation() {
        let tmp_dir_path = TempDir::new("assign_partition").expect("create temp dir");
        let addr = get_a_static_address();
        let mut merk = Merk::open(tmp_dir_path).unwrap();
        let mut db = Box::pin(merk);
        let kv1 = KvPair {
            key: "k1".as_bytes().to_vec(),
            value: "value1".as_bytes().to_vec(),
            action: MutationAction::InsertKv.into(),
        };
        let kv2 = KvPair {
            key: "k2".as_bytes().to_vec(),
            value: "value1".as_bytes().to_vec(),
            action: MutationAction::InsertKv.into(),
        };
        let mutation = Mutation {
            ns: "my_twitter".as_bytes().to_vec(),
            kv_pairs: vec![kv1, kv2],
            nonce: 1,
            chain_id: ChainId::MainNet.into(),
            chain_role: ChainRole::StorageShardChain.into(),
            gas_price: None,
            gas: 10,
        };
        let db_m: Pin<&mut Merk> = Pin::as_mut(&mut db);
        let result = KvStore::apply(db_m, &addr, &mutation);
        assert!(result.is_ok());
    }
}