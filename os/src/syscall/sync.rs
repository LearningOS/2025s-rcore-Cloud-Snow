use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;
use alloc::vec;
use log::*;
/// sleep syscall
pub fn sys_sleep(ms: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_sleep",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}
/// mutex create syscall
pub fn sys_mutex_create(blocking: bool) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mutex: Option<Arc<dyn Mutex>> = if !blocking {
        Some(Arc::new(MutexSpin::new()))
    } else {
        Some(Arc::new(MutexBlocking::new()))
    };
    let mut process_inner = process.inner_exclusive_access();
    if let Some(id) = process_inner
        .mutex_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.mutex_list[id] = mutex;
        process_inner.available_mutex[id] = 1;
        process_inner
            .allocation_mutex
            .iter_mut()
            .for_each(|v| v[id] = 0);
        process_inner.need_mutex.iter_mut().for_each(|v| v[id] = 0);
        id as isize
    } else {
        process_inner.mutex_list.push(mutex);
        process_inner.available_mutex.push(1);
        process_inner
            .allocation_mutex
            .iter_mut()
            .for_each(|v| v.push(0));
        process_inner.need_mutex.iter_mut().for_each(|v| v.push(0));
        process_inner.mutex_list.len() as isize - 1
    }
}
/// mutex lock syscall
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    let tid = current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid;
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_lock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());

    debug!("tid:{tid} mutext_id:{mutex_id}");
    process_inner.need_mutex[tid][mutex_id] += 1;
    if process_inner.enable_deadlock_detect {
        let mut work = process_inner.available_mutex.clone();
        let thread_num = process_inner.tasks.len();
        let mut finish = vec![false; thread_num];
        let mut found = true;
        while found {
            found = false;
            for i in 0..thread_num {
                if finish[i] == false
                    && process_inner.need_mutex[i]
                        .iter()
                        .enumerate()
                        .all(|(j, &x)| x <= work[j])
                {
                    found = true;
                    for j in 0..work.len() {
                        work[j] += process_inner.allocation_mutex[i][j];
                    }
                    finish[i] = true;
                }
            }
            // 没有找到满足条件的线程
            if !found {
                // finish不全为true，存在死锁
                if finish.iter().any(|&x| !x) {
                    return -0xDEAD;
                }
            }
        }
    }
    drop(process_inner);
    drop(process);
    mutex.lock();
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    process_inner.available_mutex[mutex_id] -= 1;
    process_inner.need_mutex[tid][mutex_id] -= 1;
    process_inner.allocation_mutex[tid][mutex_id] += 1;
    0
}
/// mutex unlock syscall
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    let tid = current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid;
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_unlock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    drop(process);
    mutex.unlock();
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    process_inner.available_mutex[mutex_id] += 1;
    process_inner.allocation_mutex[tid][mutex_id] -= 1;
    0
}
/// semaphore create syscall
pub fn sys_semaphore_create(res_count: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .semaphore_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.semaphore_list[id] = Some(Arc::new(Semaphore::new(res_count)));
        process_inner.available_semaphore[id] = res_count;
        process_inner
            .allocation_semaphore
            .iter_mut()
            .for_each(|v| v[id] = 0);
        process_inner
            .need_semaphore
            .iter_mut()
            .for_each(|v| v[id] = 0);
        id
    } else {
        process_inner
            .semaphore_list
            .push(Some(Arc::new(Semaphore::new(res_count))));
        process_inner.available_semaphore.push(res_count);
        process_inner
            .allocation_semaphore
            .iter_mut()
            .for_each(|v| v.push(0));
        process_inner
            .need_semaphore
            .iter_mut()
            .for_each(|v| v.push(0));
        debug!(
            "kernel:pid[{}] tid[{}] sys_semaphore_create: add new semaphore [{}]",
            current_task().unwrap().process.upgrade().unwrap().getpid(),
            current_task()
                .unwrap()
                .inner_exclusive_access()
                .res
                .as_ref()
                .unwrap()
                .tid,
            process_inner.semaphore_list.len() - 1
        );
        debug!("need_semaphore: {:?}", process_inner.need_semaphore);
        process_inner.semaphore_list.len() - 1
    };
    id as isize
}
/// semaphore up syscall
pub fn sys_semaphore_up(sem_id: usize) -> isize {
    let tid: usize = current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid;
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_up",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    drop(process_inner);
    sem.up();
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    process_inner.available_semaphore[sem_id] += 1;
    debug!("tid:{tid} sem_id:{sem_id}");
    process_inner.allocation_semaphore[tid][sem_id] -= 1;
    0
}
/// semaphore down syscall
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    let tid = current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid;
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_down",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    debug!("tid:{tid} sem_id:{sem_id}");
    process_inner.need_semaphore[tid][sem_id] += 1;
    if process_inner.enable_deadlock_detect {
        let mut work = process_inner.available_semaphore.clone();
        let thread_num = process_inner.tasks.len();
        let mut finish = vec![false; thread_num];
        let mut found = true;
        while found {
            found = false;
            for i in 0..thread_num {
                if finish[i] == false
                    && process_inner.need_semaphore[i]
                        .iter()
                        .enumerate()
                        .all(|(j, &x)| x <= work[j])
                {
                    found = true;
                    for j in 0..work.len() {
                        work[j] += process_inner.allocation_semaphore[i][j];
                    }
                    finish[i] = true;
                }
            }
            // 没有找到满足条件的线程
            if !found {
                // finish不全为true，存在死锁
                if finish.iter().any(|&x| !x) {
                    return -0xDEAD;
                }
            }
        }
    }
    drop(process_inner);
    sem.down();

    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    process_inner.available_semaphore[sem_id] -= 1;
    process_inner.need_semaphore[tid][sem_id] -= 1;
    process_inner.allocation_semaphore[tid][sem_id] += 1;
    0
}
/// condvar create syscall
pub fn sys_condvar_create() -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .condvar_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.condvar_list[id] = Some(Arc::new(Condvar::new()));
        id
    } else {
        process_inner
            .condvar_list
            .push(Some(Arc::new(Condvar::new())));
        process_inner.condvar_list.len() - 1
    };
    id as isize
}
/// condvar signal syscall
pub fn sys_condvar_signal(condvar_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_signal",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    drop(process_inner);
    condvar.signal();
    0
}
/// condvar wait syscall
pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_wait",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    condvar.wait(mutex);
    0
}
/// enable deadlock detection syscall
///
/// YOUR JOB: Implement deadlock detection, but might not all in this syscall
pub fn sys_enable_deadlock_detect(enabled: usize) -> isize {
    trace!("kernel: sys_enable_deadlock_detect");
    match enabled {
        1 => {
            current_process()
                .inner_exclusive_access()
                .enable_deadlock_detect = true;
        }
        0 => {
            current_process()
                .inner_exclusive_access()
                .enable_deadlock_detect = false;
        }
        _ => return -1,
    }
    0
}
