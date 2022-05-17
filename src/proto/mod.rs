//
//
// mod.rs
// Copyright (C) 2022 rtstore.io Author imrtstore <rtstore_dev@outlook.com>
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

pub mod rtstore_base_proto {
    tonic::include_proto!("rtstore_base_proto");
}

pub mod rtstore_meta_proto {
    tonic::include_proto!("rtstore_meta_proto");
}

pub mod rtstore_memory_proto {
    tonic::include_proto!("rtstore_memory_proto");
}
