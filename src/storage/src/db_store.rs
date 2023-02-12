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

use super::db_key::DbKey;
use crate::db3_document::DB3Document;
use bytes::BytesMut;
use db3_base::bson_util;
use db3_crypto::{
    db3_address::DB3Address, id::CollectionId, id::DbId, id::DocumentEntryId, id::DocumentId,
    id::IndexId, id::TxId,
};
use db3_error::{DB3Error, Result};
use db3_proto::db3_database_proto::{Collection, Database, Document};
use db3_proto::db3_mutation_proto::{DatabaseAction, DatabaseMutation};
use merkdb::proofs::{query::Query, Node, Op as ProofOp};
use merkdb::{BatchEntry, Merk, Op};
use prost::Message;
use std::collections::HashMap;
use std::collections::LinkedList;
use std::ops::Range;
use std::pin::Pin;
use tracing::{debug, info, span, warn, Level};

pub struct DbStore {}

impl DbStore {
    pub fn new() -> Self {
        Self {}
    }

    fn update_database(
        old_db: &Database,
        mutation: &DatabaseMutation,
        tx_id: &TxId,
        block_id: u64,
        mutation_id: u32,
    ) -> Result<Database> {
        let mut new_collections: HashMap<String, Collection> = HashMap::new();
        let mut idx = 0;
        for new_collection in mutation.collection_mutations.iter() {
            if old_db
                .collections
                .contains_key(&new_collection.collection_name)
            {
                return Err(DB3Error::ApplyDatabaseError(format!(
                    "duplicated collection names {}",
                    new_collection.collection_name
                )));
            } else {
                idx += 1;
                new_collections.insert(
                    new_collection.collection_name.to_string(),
                    Collection {
                        id: CollectionId::create(block_id, mutation_id, idx)
                            .unwrap()
                            .as_ref()
                            .to_vec(),
                        name: new_collection.collection_name.to_string(),
                        index_list: new_collection.index.to_vec(),
                    },
                );
            }
        }

        for (k, v) in old_db.collections.iter() {
            new_collections.insert(k.to_string(), v.clone());
        }
        let mut tx_list = old_db.tx.to_vec();
        tx_list.push(tx_id.as_ref().to_vec());
        Ok(Database {
            address: old_db.address.to_vec(),
            sender: old_db.sender.to_vec(),
            tx: tx_list,
            collections: new_collections,
        })
    }

    fn new_database(
        id: &DbId,
        sender: &DB3Address,
        txid: &TxId,
        mutation: &DatabaseMutation,
        block_id: u64,
        mutation_id: u32,
    ) -> Database {
        //TODO check the duplicated collection id
        let mut idx = 0;
        let collections: HashMap<String, Collection> = mutation
            .collection_mutations
            .iter()
            .map(move |x| {
                idx += 1;
                (
                    x.collection_name.to_string(),
                    Collection {
                        id: CollectionId::create(block_id, mutation_id, idx)
                            .unwrap()
                            .as_ref()
                            .to_vec(),
                        name: x.collection_name.to_string(),
                        index_list: x.index.to_vec(),
                    },
                )
            })
            .collect();
        Database {
            address: id.as_ref().to_vec(),
            sender: sender.as_ref().to_vec(),
            tx: vec![txid.as_ref().to_vec()],
            collections,
        }
    }

    fn convert(
        sender: &DB3Address,
        nonce: u64,
        tx: &TxId,
        mutation: &DatabaseMutation,
        block_id: u64,
        mutation_id: u32,
    ) -> Result<(BatchEntry, usize)> {
        let dbid = DbId::try_from((sender, nonce))?;
        let db = Self::new_database(&dbid, sender, tx, mutation, block_id, mutation_id);
        Self::encode_database(dbid, &db)
    }

    fn encode_database(dbid: DbId, database: &Database) -> Result<(BatchEntry, usize)> {
        let key = DbKey(dbid);
        let encoded_key = key.encode()?;
        let mut buf = BytesMut::with_capacity(1024 * 2);
        database
            .encode(&mut buf)
            .map_err(|e| DB3Error::ApplyDatabaseError(format!("{e}")))?;
        let buf = buf.freeze();
        let total_in_bytes = encoded_key.len() + buf.as_ref().len();
        Ok((
            (encoded_key, Op::Put(buf.as_ref().to_vec())),
            total_in_bytes,
        ))
    }

    //
    // create a new database
    //
    fn create_database(
        db: Pin<&mut Merk>,
        sender: &DB3Address,
        nonce: u64,
        tx: &TxId,
        mutation: &DatabaseMutation,
        block_id: u64,
        mutation_id: u32,
    ) -> Result<()> {
        let mut entries: Vec<BatchEntry> = Vec::new();
        let (batch_entry, _) = Self::convert(sender, nonce, tx, mutation, block_id, mutation_id)?;
        entries.push(batch_entry);
        unsafe {
            Pin::get_unchecked_mut(db)
                .apply(&entries, &[])
                .map_err(|e| DB3Error::ApplyDatabaseError(format!("{e}")))?;
        }
        Ok(())
    }

    //
    // add a new collection to database
    //
    fn add_collection(
        db: Pin<&mut Merk>,
        sender: &DB3Address,
        tx: &TxId,
        mutation: &DatabaseMutation,
        block_id: u64,
        mutation_id: u32,
    ) -> Result<()> {
        let addr_ref: &[u8] = mutation.db_address.as_ref();
        let db_id = DbId::try_from(addr_ref)?;
        let database = Self::get_database(db.as_ref(), &db_id)?;
        match database {
            Some(d) => {
                let sender_ref: &[u8] = d.sender.as_ref();
                if sender_ref != sender.as_ref() {
                    warn!(
                        "no permission to add collection to database {}",
                        db_id.to_hex()
                    );
                } else {
                    let mut entries: Vec<BatchEntry> = Vec::new();
                    let new_db = Self::update_database(&d, mutation, tx, block_id, mutation_id)?;
                    let (entry, _) = Self::encode_database(db_id, &new_db)?;
                    entries.push(entry);
                    unsafe {
                        Pin::get_unchecked_mut(db)
                            .apply(&entries, &[])
                            .map_err(|e| DB3Error::ApplyDatabaseError(format!("{e}")))?;
                    }
                }
            }
            None => {
                warn!("database not found with addr {}", db_id.to_hex());
            }
        }
        Ok(())
    }

    //
    // delete document
    //
    fn delete_document(
        db: Pin<&mut Merk>,
        sender: &DB3Address,
        mutation: &DatabaseMutation,
    ) -> Result<()> {
        let span = span!(Level::INFO, "document").entered();
        let addr_ref: &[u8] = mutation.db_address.as_ref();
        let db_id = DbId::try_from(addr_ref)?;
        let database = Self::get_database(db.as_ref(), &db_id)?;
        let mut entries: Vec<BatchEntry> = Vec::new();
        match database {
            Some(d) => {
                for document_mutation in &mutation.document_mutations {
                    let collection = d
                        .collections
                        .get(document_mutation.collection_name.as_str())
                        .unwrap();
                    let collection_id = CollectionId::try_from_bytes(collection.id.as_slice())?;
                    for doc_id_base64 in document_mutation.ids.iter() {
                        let document_id = DocumentId::try_from_base64(&doc_id_base64)?;
                        if let Some(v) = db
                            .get(document_id.as_ref())
                            .map_err(|e| DB3Error::QueryDocumentError(format!("{:?}", e)))?
                        {
                            let db3_doc = DB3Document::try_from(v.clone())?;
                            let owner = &db3_doc.get_owner()?;
                            if sender == owner {
                                info!("delete doc id {}", document_id);
                                entries.push((document_id.as_ref().to_vec(), Op::Delete));

                                for index in collection.index_list.iter() {
                                    let key = db3_doc.get_keys(index)?;
                                    let index_id = IndexId::create(
                                        &collection_id,
                                        index.id,
                                        key.to_string().as_str(),
                                        &document_id,
                                    )?;
                                    entries.push((index_id.as_ref().to_vec(), Op::Delete));
                                }
                            } else {
                                return Err(DB3Error::DocumentModifiedPermissionError);
                            }
                        } else {
                            return Err(DB3Error::DocumentNotExist(doc_id_base64.clone()));
                        }
                    }
                }
            }
            None => {
                return Err(DB3Error::ApplyDatabaseError(format!(
                    "database not found with addr {}",
                    db_id.to_hex()
                )))
            }
        }
        unsafe {
            entries.sort_by(|(a_key, _), (b_key, _)| a_key.cmp(&b_key));
            Pin::get_unchecked_mut(db)
                .apply(&entries, &[])
                .map_err(|e| DB3Error::ApplyDatabaseError(format!("{:?}", e)))?;
        }
        span.exit();
        Ok(())
    }

    //
    // add document
    //
    fn add_document(
        db: Pin<&mut Merk>,
        sender: &DB3Address,
        tx: &TxId,
        mutation: &DatabaseMutation,
        block_id: u64,
        mutation_id: u32,
    ) -> Result<Vec<DocumentId>> {
        let addr_ref: &[u8] = mutation.db_address.as_ref();
        let db_id = DbId::try_from(addr_ref)?;
        let database = Self::get_database(db.as_ref(), &db_id)?;
        let span = span!(Level::INFO, "database").entered();
        match database {
            Some(d) => {
                let mut entries: Vec<BatchEntry> = Vec::new();
                let mut document_ids: Vec<DocumentId> = Vec::new();
                let cid_index_map: &HashMap<String, _> = &d.collections;
                for document_mutation in &mutation.document_mutations {
                    if let Some(collection) = cid_index_map.get(&document_mutation.collection_name)
                    {
                        let collection_id = CollectionId::try_from_bytes(collection.id.as_slice())
                            .map_err(|e| DB3Error::InvalidCollectionIdBytes(format!("{:?}", e)))
                            .unwrap();
                        for document in document_mutation.documents.iter() {
                            // generate document entry id
                            let document_entry_id = DocumentEntryId::create(
                                block_id,
                                mutation_id,
                                entries.len() as u32,
                            )
                            .map_err(|e| DB3Error::ApplyDatabaseError(format!("{:?}", e)))
                            .unwrap();

                            // generate document id
                            let document_id =
                                DocumentId::create(&collection_id, &document_entry_id)
                                    .map_err(|e| DB3Error::ApplyDatabaseError(format!("{:?}", e)))
                                    .unwrap();

                            // construct db3 document with tx_id and owner addr
                            let db3_document = DB3Document::create_from_document_bytes(
                                document.clone(),
                                &document_id,
                                &tx,
                                &sender,
                            )
                            .map_err(|e| DB3Error::ApplyDatabaseError(format!("{:?}", e)))
                            .unwrap();
                            let document_vec = db3_document.into_bytes().to_vec();
                            info!("put document id {}", document_id.to_string());
                            entries.push((document_id.as_ref().to_vec(), Op::Put(document_vec)));

                            // insert index key -> document_id
                            for index in collection.index_list.iter() {
                                // retrieve key(single/multiple) from db3 document
                                match db3_document.get_keys(index) {
                                    Ok(key) => {
                                        // generate index id
                                        let index_id = IndexId::create(
                                            &collection_id,
                                            index.id,
                                            // TODO: convert key into bson bytes
                                            key.to_string().as_str(),
                                            &document_id,
                                        )
                                        .map_err(|e| {
                                            DB3Error::ApplyDatabaseError(format!("{:?}", e))
                                        })
                                        .unwrap();

                                        // put indexId->documentId
                                        info!("put index id {}", index_id.to_string());
                                        entries.push((index_id.as_ref().to_vec(), Op::Put(vec![])));
                                    }
                                    Err(e) => {
                                        return Err(DB3Error::ApplyDatabaseError(format!(
                                            "fail to decode index keys fron document: {:?}",
                                            e
                                        )));
                                    }
                                }
                            }

                            document_ids.push(document_id);
                        }
                    }
                }
                unsafe {
                    entries.sort_by(|(a_key, _), (b_key, _)| a_key.cmp(&b_key));
                    Pin::get_unchecked_mut(db)
                        .apply(&entries, &[])
                        .map_err(|e| DB3Error::ApplyDatabaseError(format!("{:?}", e)))?;
                }
                span.exit();
                return Ok(document_ids);
            }
            None => {
                span.exit();
                return Err(DB3Error::ApplyDatabaseError(format!(
                    "database not found with addr {}",
                    db_id.to_hex()
                )));
            }
        }
    }
    //
    // add document
    //
    pub fn get_document(db: Pin<&Merk>, document_id: &DocumentId) -> Result<Option<Document>> {
        //TODO use reference
        debug!("get document id: {}", document_id);
        if let Some(v) = db
            .get(document_id.as_ref())
            .map_err(|e| DB3Error::QueryDocumentError(format!("{e}")))?
        {
            let db3_doc = DB3Document::try_from(v.clone())?;
            let doc = bson_util::bson_document_into_bytes(db3_doc.get_document()?);
            let owner = db3_doc.get_owner()?.to_vec();
            let tx_id = db3_doc.get_tx_id()?.as_ref().to_vec();
            Ok(Some(Document {
                id: document_id.as_ref().to_vec(),
                doc,
                owner,
                tx_id,
            }))
        } else {
            Ok(None)
        }
    }
    //
    // get documents
    //
    pub fn get_documents(db: Pin<&Merk>, collection_id: &CollectionId) -> Result<Vec<Document>> {
        //TODO use reference
        let start_key = DocumentId::create(collection_id, &DocumentEntryId::zero())
            .unwrap()
            .as_ref()
            .to_vec();
        let end_key = DocumentId::create(collection_id, &DocumentEntryId::one())
            .unwrap()
            .as_ref()
            .to_vec();
        let mut query = Query::new();
        query.insert_range(std::ops::Range {
            start: start_key,
            end: end_key,
        });

        let ops = db
            .execute_query(query)
            .map_err(|e| DB3Error::QueryKvError(format!("{}", e)))?;
        let mut values: Vec<_> = Vec::new();
        for op in ops.iter() {
            match op {
                ProofOp::Push(Node::KV(k, v)) => {
                    let db3_doc = DB3Document::try_from(v.clone())?;
                    let doc = bson_util::bson_document_into_bytes(db3_doc.get_document()?);
                    let owner = db3_doc.get_owner()?.to_vec();
                    let tx_id = db3_doc.get_tx_id()?.as_ref().to_vec();
                    values.push(Document {
                        id: k.to_vec(),
                        doc,
                        owner,
                        tx_id,
                    })
                }
                _ => {}
            }
        }
        Ok(values)
    }
    pub fn apply_mutation(
        db: Pin<&mut Merk>,
        sender: &DB3Address,
        nonce: u64,
        tx: &TxId,
        mutation: &DatabaseMutation,
        block_id: u64,
        mutation_id: u32,
    ) -> Result<()> {
        let action = DatabaseAction::from_i32(mutation.action);
        match action {
            Some(DatabaseAction::CreateDb) => {
                Self::create_database(db, sender, nonce, tx, mutation, block_id, mutation_id)
            }
            Some(DatabaseAction::AddCollection) => {
                Self::add_collection(db, sender, tx, mutation, block_id, mutation_id)
            }
            Some(DatabaseAction::AddDocument) => {
                // TODO: send event with added ids
                match Self::add_document(db, sender, tx, mutation, block_id, mutation_id) {
                    Ok(ids) => Ok(()),
                    Err(e) => Err(e),
                }
            }
            Some(DatabaseAction::DeleteDocument) => Self::delete_document(db, sender, mutation),
            None => Ok(()),
        }
    }
    pub fn get_database(db: Pin<&Merk>, id: &DbId) -> Result<Option<Database>> {
        //TODO use reference
        let key = DbKey(id.clone());
        let encoded_key = key.encode()?;
        let value = db
            .get(encoded_key.as_ref())
            .map_err(|e| DB3Error::QueryDatabaseError(format!("{:?}", e)))?;
        if let Some(v) = value {
            match Database::decode(v.as_ref()) {
                Ok(database) => Ok(Some(database)),
                Err(e) => Err(DB3Error::QueryDatabaseError(format!("{:?}", e))),
            }
        } else {
            Ok(None)
        }
    }

    pub fn get_databases(db: Pin<&Merk>) -> Result<LinkedList<ProofOp>> {
        let start_key = DbKey::min();
        let end_key = DbKey::max();
        let range = Range {
            start: start_key.encode()?,
            end: end_key.encode()?,
        };
        let mut query = Query::new();
        query.insert_range(range);
        let ops = db
            .execute_query(query)
            .map_err(|e| DB3Error::QueryDatabaseError(format!("{e}")))?;
        Ok(ops)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use db3_base::bson_util;
    use db3_crypto::key_derive;
    use db3_crypto::signature_scheme::SignatureScheme;
    use db3_proto::db3_database_proto::{
        index::index_field::{Order, ValueMode},
        index::IndexField,
        Index,
    };
    use db3_proto::db3_mutation_proto::CollectionMutation;
    use db3_proto::db3_mutation_proto::DocumentMutation;
    use merkdb::rocksdb::merge_operator::delete_callback;
    use std::boxed::Box;
    use tempdir::TempDir;

    fn gen_address() -> DB3Address {
        let seed: [u8; 32] = [0; 32];
        let (address, _) =
            key_derive::derive_key_pair_from_path(&seed, None, &SignatureScheme::ED25519).unwrap();
        address
    }

    fn build_delete_document_mutation(
        addr: &DB3Address,
        collection_name: &str,
        ids: Vec<String>,
    ) -> DatabaseMutation {
        let document_mutations = vec![DocumentMutation {
            collection_name: collection_name.to_string(),
            documents: vec![],
            ids,
        }];
        let dm = DatabaseMutation {
            meta: None,
            collection_mutations: vec![],
            document_mutations,
            db_address: addr.to_vec(),
            action: DatabaseAction::CreateDb.into(),
        };
        let json_data = serde_json::to_string(&dm).unwrap();
        println!("{json_data}");
        dm
    }
    fn build_add_document_mutation(
        addr: &DB3Address,
        collection_name: &str,
        docs: Vec<String>,
    ) -> DatabaseMutation {
        let documents = docs
            .iter()
            .map(|data| bson_util::json_str_to_bson_bytes(data).unwrap())
            .collect();
        let document_mutations = vec![DocumentMutation {
            collection_name: collection_name.to_string(),
            documents,
            ids: vec![],
        }];
        let dm = DatabaseMutation {
            meta: None,
            collection_mutations: vec![],
            document_mutations,
            db_address: addr.to_vec(),
            action: DatabaseAction::CreateDb.into(),
        };
        let json_data = serde_json::to_string(&dm).unwrap();
        println!("{json_data}");
        dm
    }

    fn build_database_mutation(addr: &DB3Address, collection_name: &str) -> DatabaseMutation {
        let index_field = IndexField {
            field_path: "name".to_string(),
            value_mode: Some(ValueMode::Order(Order::Ascending as i32)),
        };

        let index = Index {
            id: 0,
            name: "idx1".to_string(),
            fields: vec![index_field],
        };

        let index_mutation = CollectionMutation {
            index: vec![index],
            collection_name: collection_name.to_string(),
        };

        let dm = DatabaseMutation {
            meta: None,
            collection_mutations: vec![index_mutation],
            document_mutations: vec![],
            db_address: addr.to_vec(),
            action: DatabaseAction::CreateDb.into(),
        };
        let json_data = serde_json::to_string(&dm).unwrap();
        println!("{json_data}");
        dm
    }

    #[test]
    fn db_store_new_database_test() {
        let addr = gen_address();
        let db_id = DbId::try_from((&addr, 1)).unwrap();
        let db_mutation = build_database_mutation(&addr, "collection1");
        let database = DbStore::new_database(&db_id, &addr, &TxId::zero(), &db_mutation, 1000, 100);
        assert!(database.collections.contains_key("collection1"))
    }

    #[test]
    fn db_store_update_database_test() {
        let addr = gen_address();
        let db_id = DbId::try_from((&addr, 1)).unwrap();
        let db_mutation = build_database_mutation(&addr, "collection1");
        let old_database =
            DbStore::new_database(&db_id, &addr, &TxId::zero(), &db_mutation, 1000, 100);
        assert!(old_database.collections.contains_key("collection1"));
        let db_mutation_2 = build_database_mutation(&addr, "collection2");
        let new_database =
            DbStore::update_database(&old_database, &db_mutation_2, &TxId::zero(), 1000, 101)
                .unwrap();

        assert!(new_database.collections.contains_key("collection1"));
        assert!(new_database.collections.contains_key("collection2"));
    }

    #[test]
    fn db_store_update_database_wrong_path() {
        let addr = gen_address();
        let db_id = DbId::try_from((&addr, 1)).unwrap();
        let db_mutation = build_database_mutation(&addr, "collection1");
        let old_database =
            DbStore::new_database(&db_id, &addr, &TxId::zero(), &db_mutation, 1000, 100);
        assert!(old_database.collections.contains_key("collection1"));
        let db_mutation_2 = build_database_mutation(&addr, "collection1");
        let res = DbStore::update_database(&old_database, &db_mutation_2, &TxId::zero(), 1000, 101);
        assert!(res.is_err());
        assert_eq!(
            "Err(ApplyDatabaseError(\"duplicated collection names collection1\"))",
            format!("{:?}", res)
        );
    }

    #[test]
    fn db_store_smoke_test() {
        let tmp_dir_path = TempDir::new("db_store_test").expect("create temp dir");
        let addr = gen_address();
        let merk = Merk::open(tmp_dir_path).unwrap();
        let mut db = Box::pin(merk);
        let collection_name = "db_store_smoke_test".to_string();
        let db_mutation = build_database_mutation(&addr, collection_name.as_str());
        let db_m: Pin<&mut Merk> = Pin::as_mut(&mut db);

        // create DB Test
        let result = DbStore::apply_mutation(db_m, &addr, 1, &TxId::zero(), &db_mutation, 1000, 1);
        assert!(result.is_ok());
        if let Ok(ops) = DbStore::get_databases(db.as_ref()) {
            assert_eq!(1, ops.len());
        } else {
            assert!(false);
        }

        // get database test
        let db_id = DbId::try_from((&addr, 1)).unwrap();
        if let Ok(Some(res)) = DbStore::get_database(db.as_ref(), &db_id) {
            assert_eq!(1, res.collections.len());
            assert!(res.collections.contains_key(&collection_name));
            let collection = &res.collections.get(&collection_name).unwrap();
            let collection_id = CollectionId::try_from_bytes(collection.id.as_slice()).unwrap();
            let db_mutation = build_add_document_mutation(
                db_id.address(),
                collection.name.as_str(),
                vec![r#"
        {
            "name": "John Doe",
            "age": 43,
            "phones": [
                "+44 1234567",
                "+44 2345678"
            ]
        }"#
                .to_string()],
            );

            // add document test
            let db_m: Pin<&mut Merk> = Pin::as_mut(&mut db);
            let ids =
                DbStore::add_document(db_m, &addr, &TxId::zero(), &db_mutation, 1000, 2).unwrap();
            assert_eq!(1, ids.len());
            let document_id_1 = ids[0];

            // get document test
            let res = DbStore::get_document(db.as_ref(), &document_id_1);
            if let Ok(Some(document)) = res {
                assert_eq!(
                    r#"Document({"name": String("John Doe"), "age": Int64(43), "phones": Array([String("+44 1234567"), String("+44 2345678")])})"#,
                    format!(
                        "{:?}",
                        bson_util::bytes_to_bson_document(document.doc).unwrap()
                    )
                );
                assert_eq!(document_id_1.as_ref(), document.id);
                assert_eq!(addr.to_vec(), document.owner)
            } else {
                assert!(false);
            }

            // insert two documents
            let db_m: Pin<&mut Merk> = Pin::as_mut(&mut db);
            let db_mutation = build_add_document_mutation(
                db_id.address(),
                collection.name.as_str(),
                vec![
                    r#"
        {
            "name": "Mike",
            "age": 44,
            "phones": [
                "+44 1234567",
                "+44 2345678"
            ]
        }"#
                    .to_string(),
                    r#"
        {
            "name": "Bob",
            "age": 45,
            "phones": [
                "+44 1234567",
                "+44 2345678"
            ]
        }"#
                    .to_string(),
                ],
            );
            let ids =
                DbStore::add_document(db_m, &addr, &TxId::zero(), &db_mutation, 1000, 3).unwrap();
            assert_eq!(2, ids.len());
            let document_id_2 = ids[0];
            let document_id_3 = ids[1];

            // show documents
            if let Ok(documents) = DbStore::get_documents(db.as_ref(), &collection_id) {
                assert_eq!(3, documents.len());
            } else {
                assert!(false);
            }

            // delete document
            let db_mutation = build_delete_document_mutation(
                db_id.address(),
                &collection_name,
                vec![document_id_2.to_base64(), document_id_3.to_base64()],
            );
            let db_m: Pin<&mut Merk> = Pin::as_mut(&mut db);
            let res = DbStore::delete_document(db_m, &addr, &db_mutation);
            assert!(res.is_ok(), "{:?}", res);

            // show documents
            if let Ok(documents) = DbStore::get_documents(db.as_ref(), &collection_id) {
                assert_eq!(1, documents.len());
            } else {
                assert!(false);
            }

            assert!(DbStore::get_document(db.as_ref(), &document_id_2)
                .unwrap()
                .is_none());
            assert!(DbStore::get_document(db.as_ref(), &document_id_3)
                .unwrap()
                .is_none());
            assert!(DbStore::get_document(db.as_ref(), &document_id_1)
                .unwrap()
                .is_some());
        } else {
            assert!(false);
        }
    }
}
