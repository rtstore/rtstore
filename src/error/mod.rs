//
//
// mod.rs
// Copyright (C) 2022 rtstore.io Author imotai <codego.me@gmail.com>
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

use arrow::error::ArrowError;
use s3::error::S3Error;
use std::io::{Error as IoError, ErrorKind};
use thiserror::Error;

/// The error system for rtstore
#[derive(Debug, Error)]
pub enum RTStoreError {
    #[error("table with name {tname} was not found")]
    TableNotFoundError { tname: String },
    #[error("invalid table names for {error}")]
    TableInvalidNamesError { error: String },
    #[error("table with name {name} exists")]
    TableNamesExistError { name: String },
    #[error("table type mismatch left {left} and right {right}")]
    TableTypeMismatchError { left: String, right: String },
    #[error("table to arrow for error : {0}")]
    TableArrowError(ArrowError),
    #[error("file with {path} is invalid")]
    FSInvalidFileError { path: String },
    #[error("filesystem io error:{0}")]
    FSIoError(IoError),
    #[error("fail to convert {0} to rtstore column type")]
    TableSchemaConvertError(i32),
    #[error("the schema for table {name} is invalid, please check the input")]
    TableSchemaInvalidError { name: String },
    #[error("create table error for {err}")]
    MetaRpcCreateTableError { err: String },
    #[error("the {name} of cell store config is invalid for {err}")]
    CellStoreInvalidConfigError { name: String, err: String },
    #[error("aws-s3: {0}")]
    CellStoreS3Error(S3Error),
    #[error("row codec error : {0}")]
    RowCodecError(bincode::Error),
    #[error("system busy for error : {0}")]
    BaseBusyError(String),
}

/// convert io error to rtstore error
impl From<IoError> for RTStoreError {
    fn from(error: IoError) -> Self {
        RTStoreError::FSIoError(error)
    }
}

impl From<S3Error> for RTStoreError {
    fn from(error: S3Error) -> Self {
        RTStoreError::CellStoreS3Error(error)
    }
}

impl From<ArrowError> for RTStoreError {
    fn from(error: ArrowError) -> Self {
        RTStoreError::TableArrowError(error)
    }
}

impl From<RTStoreError> for IoError {
    fn from(error: RTStoreError) -> Self {
        match error {
            RTStoreError::FSIoError(e) => e,
            _ => IoError::from(ErrorKind::Other),
        }
    }
}

impl From<RTStoreError> for String {
    fn from(error: RTStoreError) -> Self {
        format!("{}", error)
    }
}

/// The Result for rtstore
pub type Result<T> = std::result::Result<T, RTStoreError>;
