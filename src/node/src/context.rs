//
// context.rs
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

use super::auth_storage::AuthStorage;
use std::{
    boxed::Box,
    pin::Pin,
    sync::{Arc, Mutex},
};
use tendermint_rpc::HttpClient;

type ArcAuthStorage = Arc<Mutex<Pin<Box<AuthStorage>>>>;
#[derive(Clone)]
pub struct Context {
    pub store: ArcAuthStorage,
    pub client: HttpClient,
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {}
}
