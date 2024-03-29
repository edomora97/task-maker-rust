//! The supported executors.
//!
//! An executor is something that implements the communication protocol for evaluating DAGs.
//! For example the `LocalExecutor` is an implementation of a thread-based executor that will listen
//! on the client channel and will spawn a list of local workers.
//!
//! # Example
//!
//! ```
//! use task_maker_store::FileStore;
//! use task_maker_exec::executors::LocalExecutor;
//! use std::sync::{Arc, Mutex, mpsc::channel};
//! # use std::thread;
//! # use tempfile::TempDir;
//! use task_maker_cache::Cache;
//! use task_maker_exec::SuccessSandboxRunner;
//! use ductile::new_local_channel;
//!
//! # let tmpdir = TempDir::new().unwrap();
//! # let path = tmpdir.path();
//! let store = FileStore::new(path, 1000, 1000).unwrap();
//! let cache = Cache::new(path).unwrap();
//! let num_cores = 4;
//! # let sandbox_runner = Arc::new(SuccessSandboxRunner::default());
//! let mut executor = LocalExecutor::new(Arc::new(store), cache, num_cores, path, sandbox_runner).expect("failed to start executor");
//! // the communication channels for the client
//! let (tx, rx_remote) = new_local_channel();
//! let (tx_remote, rx) = new_local_channel();
//!
//! # let server = thread::spawn(move || {
//! executor.evaluate(tx_remote, rx_remote).unwrap();  // this will block!!
//! # });
//! # drop(tx);
//! # drop(rx);
//! # server.join().unwrap();
//! ```

mod local_executor;
mod remote_executor;

pub use local_executor::*;
pub use remote_executor::*;
