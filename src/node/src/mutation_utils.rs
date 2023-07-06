//
// mutation_utils.rs
// Copyright (C) 2023 db3.network Author imotai <codego.me@gmail.com>
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
use db3_crypto::db3_address::DB3Address;
use db3_error::DB3Error;
use db3_proto::db3_mutation_v2_proto::Mutation as MutationV2;
use ethers::core::types::Bytes as EthersBytes;
use ethers::types::{transaction::eip712::TypedData, Address, Signature};
use prost::Message;
use std::str::FromStr;

pub struct MutationUtil {}

impl MutationUtil {
    pub fn get_str_field<'a>(data: &'a TypedData, name: &'a str, default_val: &'a str) -> &'a str {
        if let Some(v) = data.message.get(name) {
            if let Some(t) = v.as_str() {
                t
            } else {
                default_val
            }
        } else {
            default_val
        }
    }

    pub fn get_u64_field(data: &TypedData, name: &str, default_val: u64) -> u64 {
        if let Some(v) = data.message.get(name) {
            if let Some(t) = v.as_str() {
                if let Ok(vt) = t.parse::<u64>() {
                    return vt;
                } else {
                    default_val
                }
            } else {
                default_val
            }
        } else {
            default_val
        }
    }

    pub fn verify_setup(payload: &[u8], sig: &str) -> Result<(Address, TypedData), DB3Error> {
        match serde_json::from_slice::<TypedData>(payload) {
            Ok(data) => {
                let signature = Signature::from_str(sig).map_err(|e| {
                    DB3Error::ApplyMutationError(format!("invalid signature for err {e}"))
                })?;
                let address = signature.recover_typed_data(&data).map_err(|e| {
                    DB3Error::ApplyMutationError(format!("invalid typed data for err {e}"))
                })?;
                Ok((address, data))
            }
            Err(e) => Err(DB3Error::ApplyMutationError(format!(
                "bad typed data for err {e}"
            ))),
        }
    }

    /// unwrap and verify write request
    pub fn unwrap_and_light_verify(
        payload: &[u8],
        sig: &str,
    ) -> Result<(MutationV2, DB3Address, u64), DB3Error> {
        match serde_json::from_slice::<TypedData>(payload) {
            Ok(data) => {
                // serde signature
                let signature = Signature::from_str(sig).map_err(|e| {
                    DB3Error::ApplyMutationError(format!("invalid signature for err {e}"))
                })?;
                if let (Some(payload), Some(nonce)) =
                    (data.message.get("payload"), data.message.get("nonce"))
                {
                    let address = signature.recover_typed_data(&data).map_err(|e| {
                        DB3Error::ApplyMutationError(format!("invalid typed data for err {e}"))
                    })?;
                    let db3_address = DB3Address::from(address.as_fixed_bytes());
                    let data: EthersBytes =
                        serde_json::from_value(payload.clone()).map_err(|e| {
                            DB3Error::ApplyMutationError(format!("invalid payload for err {e}"))
                        })?;
                    let dm = MutationV2::decode(data.as_ref()).map_err(|e| {
                        DB3Error::ApplyMutationError(format!("invalid mutation for err {e}"))
                    })?;
                    let real_nonce = u64::from_str(
                        nonce
                            .as_str()
                            .ok_or(DB3Error::ApplyMutationError("invalid nonce".to_string()))?,
                    )
                    .map_err(|e| {
                        DB3Error::ApplyMutationError(format!(
                            "fail to convert payload type to i32 {e}"
                        ))
                    })?;
                    Ok((dm, db3_address, real_nonce))
                } else {
                    Err(DB3Error::ApplyMutationError("bad typed data".to_string()))
                }
            }
            Err(e) => Err(DB3Error::ApplyMutationError(format!(
                "bad typed data for err {e}"
            ))),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use chrono::Utc;
    use ethers::types::transaction::eip712::Eip712;

    #[test]
    pub fn test_java_sdk_verfiy_ut() {
        //let expected_addr = "f39fd6e51aad88f6f4ce6ab8827279cfffb92266";
        let typed_data = r#"
       {"types":{"EIP712Domain":[{"name":"name","type":"string"}],"Message":[{"name":"payload","type":"bytes"},{"name":"nonce","type":"string"}]},"primaryType":"Message","message":{"payload":"0x1a0822060a0464657363","nonce":"1"},"domain":{"name":"db3.network"}}
        "#;
        let typed_data_obj = serde_json::from_slice::<TypedData>(typed_data.as_bytes()).unwrap();
        let hashed_message = typed_data_obj.encode_eip712().unwrap();
        let hex_str = hex::encode(hashed_message);
        assert_eq!(
            "2b6ab2777e1ffb472f2f3206566f0cb691228ba5fb02692fd8fe933576b5003e",
            hex_str.as_str()
        );
    }
}
