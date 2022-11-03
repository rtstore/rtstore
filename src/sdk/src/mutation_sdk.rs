//
// mutation_sdk.rs
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

use bytes::BytesMut;
use db3_crypto::signer::MutationSigner;
use db3_error::{DB3Error, Result};
use db3_proto::db3_mutation_proto::Mutation;
use prost::Message;
use tendermint_rpc::{Client, HttpClient};

pub struct MutationSDK {
    client: HttpClient,
    signer: MutationSigner,
}

impl MutationSDK {
    pub fn new(client: HttpClient, signer: MutationSigner) -> Self {
        Self { client, signer }
    }

    pub async fn submit_mutation(&self, mutation: &Mutation) -> Result<()> {
        //TODO update gas and nonce
        let request = self.signer.sign(&mutation)?;
        let mut buf = BytesMut::with_capacity(1024 * 4);
        request.encode(&mut buf);
        let buf = buf.freeze();
        self.client
            .broadcast_tx_async(buf.as_ref().to_vec().into())
            .await
            .map_err(|e| DB3Error::SubmitMutationError(format!("{}", e)))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::HttpClient;
    use super::Mutation;
    use super::MutationSDK;
    use super::MutationSigner;
    use db3_proto::db3_base_proto::{ChainId, ChainRole, UnitType, Units};
    use db3_proto::db3_mutation_proto::{KvPair, MutationAction};
    use fastcrypto::secp256k1::Secp256k1KeyPair;
    use fastcrypto::traits::KeyPair;
    use rand::rngs::StdRng;
    use rand::SeedableRng;
    use std::{thread, time};
    #[tokio::test]
    async fn it_submit_mutation() {
        let client = HttpClient::new("http://127.0.0.1:26657").unwrap();
        let mut rng = StdRng::from_seed([0; 32]);
        let kp = Secp256k1KeyPair::generate(&mut rng);
        let signer = MutationSigner::new(kp);
        let sdk = MutationSDK::new(client, signer);
        let mut count = 11;
        loop {
            let kv = KvPair {
                key: format!("k{}", count).as_bytes().to_vec(),
                value: "value1".as_bytes().to_vec(),
                action: MutationAction::InsertKv.into(),
            };
            let mutation = Mutation {
                ns: "my_twitter".as_bytes().to_vec(),
                kv_pairs: vec![kv],
                nonce: 1,
                chain_id: ChainId::MainNet.into(),
                chain_role: ChainRole::StorageShardChain.into(),
                gas_price: None,
                gas: 10,
            };
            let result = sdk.submit_mutation(&mutation).await;
            assert!(result.is_ok());
            let ten_millis = time::Duration::from_millis(1000);
            let now = time::Instant::now();
            thread::sleep(ten_millis);
            count = count + 1;
            if count > 20 {
                break;
            }
        }
    }
}