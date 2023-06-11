//
// ns_store.rs
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
use crate::db_owner_key::DbOwnerKey;
use bytes::BytesMut;
use db3_crypto::db3_address::DB3Address;
use db3_crypto::id::DbId;
use db3_crypto::id_v2::OpEntryId;

use crate::collection_key;
use db3_error::{DB3Error, Result};
use db3_proto::db3_database_v2_proto::{
    database_message, Collection, DatabaseMessage, DocumentDatabase,
};
use db3_proto::db3_mutation_v2_proto::{CollectionMutation, DocumentDatabaseMutation};
use prost::Message;
use rocksdb::{DBWithThreadMode, MultiThreaded, Options, WriteBatch};
use std::path::Path;
use std::sync::Arc;
use tracing::info;

type StorageEngine = DBWithThreadMode<MultiThreaded>;

#[derive(Clone)]
pub struct DBStoreV2Config {
    pub db_path: String,
    pub db_store_cf_name: String,
    pub doc_store_cf_name: String,
    pub collection_store_cf_name: String,
    pub index_store_cf_name: String,
    pub doc_owner_store_cf_name: String,
    pub db_owner_store_cf_name: String,
    pub scan_max_limit: usize,
}

pub struct DBStoreV2 {
    config: DBStoreV2Config,
    se: Arc<StorageEngine>,
}

impl DBStoreV2 {
    pub fn new(config: DBStoreV2Config) -> Result<Self> {
        let mut cf_opts = Options::default();
        cf_opts.create_if_missing(true);
        cf_opts.create_missing_column_families(true);
        info!("open db store with path {}", config.db_path.as_str());
        let path = Path::new(config.db_path.as_str());
        let se = Arc::new(
            StorageEngine::open_cf(
                &cf_opts,
                &path,
                [
                    config.db_store_cf_name.as_str(),
                    config.doc_store_cf_name.as_str(),
                    config.collection_store_cf_name.as_str(),
                    config.index_store_cf_name.as_str(),
                    config.doc_owner_store_cf_name.as_str(),
                    config.db_owner_store_cf_name.as_str(),
                ],
            )
            .map_err(|e| DB3Error::OpenStoreError(config.db_path.to_string(), format!("{e}")))?,
        );
        Ok(Self { config, se })
    }

    pub fn get_collection(&self, db_addr: &DB3Address, name: &str) -> Result<Option<Collection>> {
        let ck = collection_key::build_collection_key(db_addr, name)
            .map_err(|e| DB3Error::ReadStoreError(format!("{e}")))?;
        let ck_ref: &[u8] = ck.as_ref();
        let collection_store_cf_handle = self
            .se
            .cf_handle(self.config.collection_store_cf_name.as_str())
            .ok_or(DB3Error::ReadStoreError("cf is not found".to_string()))?;
        let value = self
            .se
            .get_cf(&collection_store_cf_handle, ck_ref)
            .map_err(|e| DB3Error::ReadStoreError(format!("{e}")))?;
        if let Some(v) = value {
            match Collection::decode(v.as_ref()) {
                Ok(c) => Ok(Some(c)),
                Err(e) => Err(DB3Error::ReadStoreError(format!("{e}"))),
            }
        } else {
            Ok(None)
        }
    }

    pub fn create_collection(
        &self,
        sender: &DB3Address,
        db_addr: &DB3Address,
        collection: &CollectionMutation,
        block: u64,
        order: u32,
        idx: u16,
    ) -> Result<()> {
        let db = self.get_database(db_addr)?;
        if db.is_none() {
            return Err(DB3Error::ReadStoreError(
                "fail to find database".to_string(),
            ));
        }

        //TODO add mutex control
        let ck = collection_key::build_collection_key(db_addr, collection.collection_name.as_str())
            .map_err(|e| DB3Error::ReadStoreError(format!("{e}")))?;

        let collection_store_cf_handle = self
            .se
            .cf_handle(self.config.collection_store_cf_name.as_str())
            .ok_or(DB3Error::ReadStoreError("cf is not found".to_string()))?;

        let ck_ref: &[u8] = ck.as_ref();

        let value = self
            .se
            .get_cf(&collection_store_cf_handle, ck_ref)
            .map_err(|e| DB3Error::ReadStoreError(format!("{e}")))?;

        if let Some(_v) = value {
            return Err(DB3Error::ReadStoreError(format!(
                "collection with name {} exist",
                collection.collection_name.as_str()
            )));
        }
        let id = OpEntryId::create(block, order, idx)
            .map_err(|e| DB3Error::ReadStoreError(format!("{e}")))?;

        // validate the index
        let col = Collection {
            id: id.as_ref().to_vec(),
            name: collection.collection_name.to_string(),
            index_list: collection.index.to_vec(),
            sender: sender.as_ref().to_vec(),
        };
        let mut buf = BytesMut::with_capacity(1024);
        col.encode(&mut buf)
            .map_err(|e| DB3Error::WriteStoreError(format!("{e}")))?;
        let buf = buf.freeze();
        let mut batch = WriteBatch::default();
        batch.put_cf(&collection_store_cf_handle, ck_ref, buf.as_ref());
        self.se
            .write(batch)
            .map_err(|e| DB3Error::WriteStoreError(format!("{e}")))?;
        Ok(())
    }

    pub fn get_database(&self, db_addr: &DB3Address) -> Result<Option<DatabaseMessage>> {
        let db_store_cf_handle = self
            .se
            .cf_handle(self.config.db_store_cf_name.as_str())
            .ok_or(DB3Error::ReadStoreError("cf is not found".to_string()))?;
        let value = self
            .se
            .get_cf(&db_store_cf_handle, db_addr.as_ref())
            .map_err(|e| DB3Error::ReadStoreError(format!("{e}")))?;
        if let Some(v) = value {
            match DatabaseMessage::decode(v.as_ref()) {
                Ok(m) => Ok(Some(m)),
                Err(e) => Err(DB3Error::ReadStoreError(format!("{e}"))),
            }
        } else {
            Ok(None)
        }
    }

    pub fn create_doc_database(
        &self,
        sender: &DB3Address,
        mutation: &DocumentDatabaseMutation,
        nonce: u64,
        network_id: u64,
        block: u64,
        order: u32,
    ) -> Result<DbId> {
        let db_addr = DbId::from((sender, nonce, network_id));
        let db_store_cf_handle = self
            .se
            .cf_handle(self.config.db_store_cf_name.as_str())
            .ok_or(DB3Error::ReadStoreError("cf is not found".to_string()))?;
        let db_owner_store_cf_handle = self
            .se
            .cf_handle(self.config.db_owner_store_cf_name.as_str())
            .ok_or(DB3Error::ReadStoreError("cf is not found".to_string()))?;
        //TODO use u32
        let db_owner = DbOwnerKey(sender, block, order as u16);
        let db_owner_encoded_key = db_owner.encode()?;
        let database = DocumentDatabase {
            address: db_addr.as_ref().to_vec(),
            sender: sender.as_ref().to_vec(),
            desc: mutation.db_desc.to_string(),
        };
        let database_msg = DatabaseMessage {
            database: Some(database_message::Database::DocDb(database)),
        };
        let mut buf = BytesMut::with_capacity(1024);
        database_msg
            .encode(&mut buf)
            .map_err(|e| DB3Error::WriteStoreError(format!("{e}")))?;
        let buf = buf.freeze();
        let mut batch = WriteBatch::default();
        batch.put_cf(&db_store_cf_handle, db_addr.as_ref(), buf.as_ref());
        batch.put_cf(
            &db_owner_store_cf_handle,
            &db_owner_encoded_key,
            db_addr.as_ref(),
        );
        self.se
            .write(batch)
            .map_err(|e| DB3Error::WriteStoreError(format!("{e}")))?;
        Ok(db_addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempdir::TempDir;

    #[test]
    fn test_new_db_store() {
        let tmp_dir_path = TempDir::new("new_db_store_path").expect("create temp dir");
        let real_path = tmp_dir_path.path().to_str().unwrap().to_string();
        let config = DBStoreV2Config {
            db_path: real_path,
            db_store_cf_name: "db".to_string(),
            doc_store_cf_name: "doc".to_string(),
            collection_store_cf_name: "cf2".to_string(),
            index_store_cf_name: "index".to_string(),
            doc_owner_store_cf_name: "doc_owner".to_string(),
            db_owner_store_cf_name: "db_owner".to_string(),
            scan_max_limit: 50,
        };
        let result = DBStoreV2::new(config);
        assert_eq!(result.is_ok(), true);
    }

    #[test]
    fn test_create_doc_db() {
        let tmp_dir_path = TempDir::new("new_database").expect("create temp dir");
        let real_path = tmp_dir_path.path().to_str().unwrap().to_string();
        let config = DBStoreV2Config {
            db_path: real_path,
            db_store_cf_name: "db".to_string(),
            doc_store_cf_name: "doc".to_string(),
            collection_store_cf_name: "cf2".to_string(),
            index_store_cf_name: "index".to_string(),
            doc_owner_store_cf_name: "doc_owner".to_string(),
            db_owner_store_cf_name: "db_owner".to_string(),
            scan_max_limit: 50,
        };
        let result = DBStoreV2::new(config);
        assert_eq!(result.is_ok(), true);
        let db_m = DocumentDatabaseMutation {
            db_desc: "test_desc".to_string(),
        };
        let db3_store = result.unwrap();
        let result = db3_store.create_doc_database(&DB3Address::ZERO, &db_m, 1, 1, 1, 1);
        assert!(result.is_ok());
        let db_id = result.unwrap();
        if let Ok(Some(db)) = db3_store.get_database(db_id.address()) {
            if let Some(database_message::Database::DocDb(doc_db)) = db.database {
                assert_eq!("test_desc", doc_db.desc.as_str());
            }
        } else {
            assert!(false);
        }
    }
}
