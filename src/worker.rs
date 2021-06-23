use std::sync::Arc;

use task_maker_exec::executors::{RemoteEntityMessage, RemoteEntityMessageResponse};
use task_maker_exec::Worker;
use task_maker_store::FileStore;

use crate::error::NiceError;
use crate::opt::{Opt, WorkerOptions};
use crate::remote::connect_to_remote_server;
use crate::sandbox::SelfExecSandboxRunner;

/// Version of task-maker
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Entry point for the worker.
pub fn main_worker(opt: Opt, worker_opt: WorkerOptions) {
    let store_path = opt.store_dir();
    let file_store = Arc::new(
        FileStore::new(
            store_path.join("store"),
            opt.max_cache * 1024 * 1024,
            opt.min_cache * 1024 * 1024,
        )
        .nice_expect("Cannot create the file store"),
    );
    let sandbox_path = store_path.join("sandboxes");

    let name = opt
        .name
        .unwrap_or_else(|| format!("{}@{}", whoami::username(), whoami::hostname()));
    let (executor_tx, executor_rx) = connect_to_remote_server(&worker_opt.server_addr, 27183)
        .nice_expect("Failed to connect to the server");
    executor_tx
        .send(RemoteEntityMessage::Welcome {
            name: name.clone(),
            version: VERSION.into(),
        })
        .nice_expect("Cannot send welcome to the server");
    if let RemoteEntityMessageResponse::Rejected(err) = executor_rx
        .recv()
        .nice_expect("Remote executor didn't reply to the welcome message")
    {
        error!("The server rejected the worker connection: {}", err);
        return;
    }

    let name = if let Some(wid) = worker_opt.worker_id {
        format!("{} {}", name, wid)
    } else {
        name
    };

    let worker = Worker::new_with_channel(
        &name,
        file_store.clone(),
        sandbox_path.clone(),
        executor_tx.change_type(),
        executor_rx.change_type(),
        SelfExecSandboxRunner::default(),
    );
    worker.work().nice_expect("Worker failed");
}
