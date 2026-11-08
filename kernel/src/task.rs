//! A tiny cooperative task scheduler.
//!
//! A *task* is a function with its own stack. "Cooperative" means a task
//! runs until it chooses to pause (by calling `yield_now`, `sleep_ms` or
//! `wait_input`). The scheduler then picks the next task that is ready.
//! If nobody is ready, the CPU halts (`hlt`) until the next interrupt,
//! which saves battery and keeps the laptop cool.
//!
//! Why cooperative rather than *preemptive* (forcibly switching tasks on a
//! timer)? It's much simpler, needs no locks between tasks, and for a
//! single-user machine where we write all the tasks ourselves, it's enough.
//!
//! Switching tasks = saving the CPU registers the compiler expects to be
//! preserved (rbx, rbp, r12-r15) on the old task's stack, storing its stack
//! pointer, loading the new task's stack pointer and popping its registers.
//! See `switch_context` below: just 15 instructions.

extern crate alloc;
use crate::arch::cpu;
use crate::time;
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::arch::global_asm;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Set by `kick()`: wakes tasks blocked in `wait_input` even without input.
static KICK: AtomicBool = AtomicBool::new(false);
/// CPU time spent halted (idle), in TSC ticks.
static IDLE_TSC: AtomicU64 = AtomicU64::new(0);

const STACK_SIZE: usize = 64 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Ready,
    Sleeping { until_ms: u64 },
    /// Wake on input *or* at the deadline (for animations and timeouts).
    WaitInput { until_ms: u64 },
    Done,
}

struct Task {
    name: &'static str,
    rsp: u64,
    state: State,
    _stack: Option<Box<[u8]>>, // None for the boot task (it uses Limine's stack)
    entry: Option<fn()>,
}

struct Scheduler { tasks: Vec<Task>, current: usize, pub switches: u64 }

struct Global(UnsafeCell<Option<Scheduler>>);
// Safety: only touched from task context (never from interrupt handlers),
// on a single CPU, and never across a switch while borrowed.
unsafe impl Sync for Global {}
static SCHED: Global = Global(UnsafeCell::new(None));

fn sched() -> &'static mut Scheduler { unsafe { (*SCHED.0.get()).as_mut().expect("scheduler not started") } }

global_asm!(r#"
.section .text
.global switch_context
// switch_context(old_rsp_slot: *mut u64 (rdi), new_rsp: u64 (rsi))
switch_context:
    push rbp
    push rbx
    push r12
    push r13
    push r14
    push r15
    mov [rdi], rsp
    mov rsp, rsi
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp
    ret

// First code a new task runs: interrupts on, then call task_main().
task_trampoline:
    sti
    call task_main
    ud2
"#);

unsafe extern "C" {
    fn switch_context(old: *mut u64, new: u64);
    fn task_trampoline();
}

#[unsafe(no_mangle)]
extern "C" fn task_main() -> ! {
    let s = sched();
    let f = s.tasks[s.current].entry.expect("task without entry");
    f();
    // The task function returned: mark it done and never come back.
    sched().tasks[sched().current].state = State::Done;
    loop { schedule(); }
}

/// Turn the current (boot) code into task 0.
pub fn init() {
    unsafe {
        *SCHED.0.get() = Some(Scheduler {
            tasks: alloc::vec![Task { name: "main", rsp: 0, state: State::Ready, _stack: None, entry: None }],
            current: 0,
            switches: 0,
        });
    }
}

/// Start a new task running `f`.
pub fn spawn(name: &'static str, f: fn()) {
    let mut stack = alloc::vec![0u8; STACK_SIZE].into_boxed_slice();
    let top = (stack.as_mut_ptr() as u64 + STACK_SIZE as u64) & !15;
    // Build the stack exactly as switch_context expects to find it:
    // 6 saved registers, then the address to "return" to.
    let frame = top - 8 * 8;
    unsafe {
        let p = frame as *mut u64;
        for i in 0..6 { *p.add(i) = 0; }          // r15, r14, r13, r12, rbx, rbp
        *p.add(6) = task_trampoline as *const () as u64;     // return address
        *p.add(7) = 0;                           // keeps the stack 16-byte aligned
    }
    sched().tasks.push(Task { name, rsp: frame, state: State::Ready, _stack: Some(stack), entry: Some(f) });
}

/// Is this task allowed to run now?
fn runnable(st: State, now: u64, input: bool) -> bool {
    match st {
        State::Ready => true,
        State::Sleeping { until_ms } => now >= until_ms,
        State::WaitInput { until_ms } => input || now >= until_ms,
        State::Done => false,
    }
}

/// Pick the next runnable task (round robin) and switch to it. If nothing
/// can run, sleep the CPU until an interrupt changes that.
fn schedule() {
    loop {
        cpu::cli(); // no interrupts between "nothing to do" and "hlt"
        let s = sched();
        let now = time::ticks_ms();
        let input = crate::input::pending() || KICK.load(Ordering::Relaxed);
        let n = s.tasks.len();
        let mut pick = None;
        for i in 1..=n {
            let idx = (s.current + i) % n;
            if runnable(s.tasks[idx].state, now, input) { pick = Some(idx); break; }
        }
        match pick {
            Some(next) => {
                s.tasks[next].state = State::Ready;
                if next != s.current {
                    let old = s.current;
                    s.current = next;
                    s.switches += 1;
                    let old_slot = &mut s.tasks[old].rsp as *mut u64;
                    let new_rsp = s.tasks[next].rsp;
                    unsafe { switch_context(old_slot, new_rsp) };
                }
                cpu::sti();
                return;
            }
            None => {
                // Nothing to do: sleep until the next interrupt, then look again.
                let t = cpu::rdtsc();
                cpu::sti_hlt();
                IDLE_TSC.fetch_add(cpu::rdtsc() - t, Ordering::Relaxed);
            }
        }
    }
}

/// Let other tasks run, then continue.
pub fn yield_now() { schedule(); }

/// Pause this task for at least `ms` milliseconds.
pub fn sleep_ms(ms: u64) {
    let s = sched();
    s.tasks[s.current].state = State::Sleeping { until_ms: time::ticks_ms() + ms };
    schedule();
}

/// Pause until there's keyboard/mouse input, or `ms` milliseconds pass.
pub fn wait_input(ms: u64) {
    if !crate::input::pending() && !KICK.load(Ordering::Relaxed) {
        let s = sched();
        s.tasks[s.current].state = State::WaitInput { until_ms: time::ticks_ms() + ms };
        schedule();
    }
    KICK.store(false, Ordering::Relaxed);
}

/// Wake whoever is in `wait_input` (e.g. the clock changed, redraw please).
pub fn kick() { KICK.store(true, Ordering::Relaxed); }

/// Total time the CPU has spent idle, in microseconds.
pub fn idle_us() -> u64 { IDLE_TSC.load(Ordering::Relaxed) * 1_000_000 / time::tsc_hz() }

/// (task count, context switches so far)
pub fn stats() -> (usize, u64) { let s = sched(); (s.tasks.iter().filter(|t| t.state != State::Done).count(), s.switches) }

pub fn current_name() -> &'static str { let s = sched(); s.tasks[s.current].name }
