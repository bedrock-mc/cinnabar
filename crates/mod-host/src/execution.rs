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
use wasmtime::{Config, Engine, Store, Trap, UpdateDeadline};

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
        .wasm_backtrace(std::env::var_os("CINNABAR_MOD_BACKTRACE").is_some())
        .wasm_memory64(false)
        .wasm_multi_memory(false)
        .wasm_relaxed_simd(false)
        .max_wasm_stack(256 * 1024);
    ticking_engine(&config)
}

/// Builds the trusted packet-mod engine: wall-clock deadlines instead of fuel, and the deeper
/// stack a language runtime's guest needs.
pub(crate) fn trusted_engine() -> Result<(Engine, EpochTicker)> {
    let mut config = Config::new();
    config
        .wasm_component_model(true)
        .consume_fuel(false)
        .async_support(true)
        .epoch_interruption(true)
        .wasm_memory64(false)
        .wasm_relaxed_simd(false)
        .max_wasm_stack(1024 * 1024);
    ticking_engine(&config)
}

/// Creates an engine and the ticker thread that advances its epoch for deadline checks.
fn ticking_engine(config: &Config) -> Result<(Engine, EpochTicker)> {
    let engine = Engine::new(config)?;
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
    let start = Instant::now();
    arm_with_elapsed(store, fuel, loading, move || start.elapsed())
}

/// Arms a store using elapsed callback time, so delayed idle ticks cannot expire fresh work.
fn arm_with_elapsed<T>(
    store: &mut Store<T>,
    fuel: u64,
    loading: bool,
    elapsed: impl FnMut() -> Duration + Send + Sync + 'static,
) -> Result<()> {
    store.set_hostcall_fuel(HOSTCALL_BYTES);
    store.set_fuel(fuel)?;
    let deadline = if loading {
        LOAD_DEADLINE
    } else {
        CALLBACK_DEADLINE
    };
    arm_deadline_with_elapsed(store, deadline, elapsed);
    Ok(())
}

/// Gives the next guest call `deadline` of wall-clock time, without touching fuel.
pub(crate) fn arm_deadline<T>(store: &mut Store<T>, deadline: Duration) {
    let start = Instant::now();
    arm_deadline_with_elapsed(store, deadline, move || start.elapsed());
}

/// Interrupts guest code once `elapsed` reaches `deadline`, checked at each epoch tick.
fn arm_deadline_with_elapsed<T>(
    store: &mut Store<T>,
    deadline: Duration,
    mut elapsed: impl FnMut() -> Duration + Send + Sync + 'static,
) {
    store.epoch_deadline_callback(move |_| {
        let remaining = deadline.saturating_sub(elapsed());
        if remaining.is_zero() {
            return Err(Trap::Interrupt.into());
        }
        Ok(UpdateDeadline::Continue(epoch_ticks(remaining)))
    });
    store.set_epoch_deadline(epoch_ticks(deadline));
}

/// Rounds a positive remaining duration up to whole ticker periods.
fn epoch_ticks(duration: Duration) -> u64 {
    duration.as_nanos().div_ceil(EPOCH_PERIOD.as_nanos()) as u64
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
    use std::sync::atomic::AtomicU64;
    use wasmtime::{Instance, Module, Trap};

    /// Simulates a delayed ticker catching up past either deadline without advancing time.
    fn catch_up(engine: &Engine) {
        let ticks = epoch_ticks(LOAD_DEADLINE);
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
            arm_with_elapsed(&mut store, u64::MAX, loading, || LOAD_DEADLINE).unwrap();
            catch_up(&engine);
            let error = work.call(&mut store, ()).unwrap_err();
            assert_eq!(error.downcast_ref::<Trap>(), Some(&Trap::Interrupt));
            arm_with_elapsed(&mut store, u64::MAX, loading, || Duration::ZERO).unwrap();
            idle.call(&mut store, ()).unwrap();
        }
    }

    #[test]
    fn idle_catch_up_preserves_work_until_its_elapsed_deadline() {
        let (engine, _ticker) = engine().unwrap();
        let module = Module::new(&engine, "(module (func (export \"idle\")))").unwrap();
        let mut store = Store::new(&engine, ());
        arm_with_elapsed(&mut store, u64::MAX, true, || Duration::ZERO).unwrap();
        let instance = Instance::new(&mut store, &module, &[]).unwrap();
        let idle = instance
            .get_typed_func::<(), ()>(&mut store, "idle")
            .unwrap();
        for loading in [true, false] {
            let elapsed = Arc::new(AtomicU64::new(0));
            let clock = Arc::clone(&elapsed);
            arm_with_elapsed(&mut store, u64::MAX, loading, move || {
                Duration::from_nanos(clock.load(Ordering::Acquire))
            })
            .unwrap();
            catch_up(&engine);
            idle.call(&mut store, ()).unwrap();

            let deadline = if loading {
                LOAD_DEADLINE
            } else {
                CALLBACK_DEADLINE
            };
            elapsed.store(deadline.as_nanos() as u64 - 1, Ordering::Release);
            catch_up(&engine);
            idle.call(&mut store, ()).unwrap();

            elapsed.store(deadline.as_nanos() as u64, Ordering::Release);
            catch_up(&engine);
            let error = idle.call(&mut store, ()).unwrap_err();
            assert_eq!(error.downcast_ref::<Trap>(), Some(&Trap::Interrupt));
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
        arm_with_elapsed(&mut store, u64::MAX, true, || LOAD_DEADLINE).unwrap();
        catch_up(&engine);
        let error = Instance::new(&mut store, &module, &[]).err().unwrap();
        assert_eq!(error.downcast_ref::<Trap>(), Some(&Trap::Interrupt));
    }
}
