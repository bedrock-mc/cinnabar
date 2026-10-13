//! Engine policy and the wall-clock backstop shared by local and server components.

use anyhow::{Context, Result};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use wasmtime::{Config, Engine, Store};

const EPOCH_PERIOD: Duration = Duration::from_millis(10);
const CALLBACK_DEADLINE: Duration = Duration::from_millis(250);
const LOAD_DEADLINE: Duration = Duration::from_secs(2);
/// Bytes one canonical guest-to-host call may copy, before host import validation.
pub(crate) const HOSTCALL_BYTES: usize = 1024 * 1024;

/// Stops and joins only the ticker owned by this engine.
pub(crate) struct EpochTicker {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

/// Builds a fuel-metered synchronous engine with epoch checks and a bounded guest stack.
pub(crate) fn engine() -> Result<(Engine, EpochTicker)> {
    let mut config = Config::new();
    config
        .wasm_component_model(true)
        .consume_fuel(true)
        .epoch_interruption(true)
        .wasm_backtrace(false)
        .wasm_memory64(false)
        .wasm_multi_memory(false)
        .wasm_relaxed_simd(false)
        .max_wasm_stack(256 * 1024);
    let engine = Engine::new(&config)?;
    let ticking = engine.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = Arc::clone(&stop);
    let worker = thread::Builder::new()
        .name("mod-epoch-ticker".into())
        .spawn(move || {
            let start = Instant::now();
            let mut ticks = 0;
            while !stopped.load(Ordering::Acquire) {
                thread::park_timeout(EPOCH_PERIOD);
                let due = start.elapsed().as_nanos() / EPOCH_PERIOD.as_nanos();
                for _ in ticks..due {
                    ticking.increment_epoch();
                }
                ticks = due;
            }
        })
        .context("start mod epoch ticker")?;
    Ok((
        engine,
        EpochTicker {
            stop,
            worker: Some(worker),
        },
    ))
}

/// Restores fuel and a fresh epoch deadline before guest execution, including post-return.
pub(crate) fn arm<T>(store: &mut Store<T>, fuel: u64, loading: bool) -> Result<()> {
    store.set_hostcall_fuel(HOSTCALL_BYTES);
    store.set_fuel(fuel)?;
    let deadline = if loading {
        LOAD_DEADLINE
    } else {
        CALLBACK_DEADLINE
    };
    store.set_epoch_deadline(deadline.as_nanos().div_ceil(EPOCH_PERIOD.as_nanos()) as u64);
    Ok(())
}

impl Drop for EpochTicker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasmtime::{Instance, Module, Trap};

    /// Advances a deadline without waiting for the ticker or reading wall time.
    fn expire(engine: &Engine) {
        let ticks = LOAD_DEADLINE.as_nanos().div_ceil(EPOCH_PERIOD.as_nanos());
        for _ in 0..=ticks {
            engine.increment_epoch();
        }
    }

    #[test]
    fn epochs_interrupt_bulk_memory_work_and_deadlines_can_be_rearmed() {
        let (engine, _ticker) = engine().unwrap();
        let module = Module::new(
            &engine,
            r#"(module
            (memory 1)
            (func (export "work") (loop $work
                i32.const 0 i32.const 0 i32.const 65536 memory.fill br $work))
            (func (export "idle")))"#,
        )
        .unwrap();
        let mut store = Store::new(&engine, ());
        arm(&mut store, u64::MAX, true).unwrap();
        let instance = Instance::new(&mut store, &module, &[]).unwrap();
        let work = instance
            .get_typed_func::<(), ()>(&mut store, "work")
            .unwrap();
        let idle = instance
            .get_typed_func::<(), ()>(&mut store, "idle")
            .unwrap();
        for loading in [true, false] {
            arm(&mut store, u64::MAX, loading).unwrap();
            expire(&engine);
            let error = work.call(&mut store, ()).unwrap_err();
            assert_eq!(error.downcast_ref::<Trap>(), Some(&Trap::Interrupt));
            arm(&mut store, u64::MAX, loading).unwrap();
            idle.call(&mut store, ()).unwrap();
        }
    }

    #[test]
    fn unneeded_memory_proposals_are_refused() {
        let (engine, _ticker) = engine().unwrap();
        for source in [
            "(module (memory i64 1))",
            "(module (memory 1) (memory 1))",
            "(module (memory 1 1 shared))",
        ] {
            assert!(Module::new(&engine, source).is_err());
        }
    }

    #[test]
    fn epoch_deadline_covers_core_start_functions() {
        let (engine, _ticker) = engine().unwrap();
        let module = Module::new(
            &engine,
            "(module (func $start (loop $work br $work)) (start $start))",
        )
        .unwrap();
        let mut store = Store::new(&engine, ());
        arm(&mut store, u64::MAX, true).unwrap();
        expire(&engine);
        let error = Instance::new(&mut store, &module, &[]).err().unwrap();
        assert_eq!(error.downcast_ref::<Trap>(), Some(&Trap::Interrupt));
    }
}
