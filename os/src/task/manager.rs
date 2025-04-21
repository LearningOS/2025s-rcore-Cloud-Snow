//!Implementation of [`TaskManager`]
use super::TaskControlBlock;
use crate::sync::UPSafeCell;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use lazy_static::*;
///A array of `TaskControlBlock` that is thread-safe
pub struct TaskManager {
    ready_queue: VecDeque<Arc<TaskControlBlock>>,
}

/// A simple FIFO scheduler.
impl TaskManager {
    ///Creat an empty TaskManager
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
        }
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push_back(task);
    }
    /// Take a process out of the ready queue
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        self.ready_queue.pop_front()
    }
    /// 根据stride调度算法获取下一个进程
    pub fn fetch_with_stride(&mut self) -> Option<Arc<TaskControlBlock>> {
        //trace!("kernel: TaskManager::fetch_task_with_stride");
        let mut min_stride = usize::MAX;
        let mut min_stride_task: Option<Arc<TaskControlBlock>> = None;
        for task in self.ready_queue.iter() {
            let stride = task.inner_exclusive_access().stride;
            if stride < min_stride {
                min_stride = stride;
                min_stride_task = Some(task.clone());
            }
        }
        if let Some(task) = min_stride_task {
            let index = self
                .ready_queue
                .iter()
                .position(|t| Arc::ptr_eq(t, &task))
                .unwrap();
            self.ready_queue.remove(index);
            task.inner_exclusive_access().inc_stride();
            Some(task)
        } else {
            None
        }
    }
}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
}

/// Add process to ready queue
pub fn add_task(task: Arc<TaskControlBlock>) {
    //trace!("kernel: TaskManager::add_task");
    TASK_MANAGER.exclusive_access().add(task);
}

/// Take a process out of the ready queue
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    //trace!("kernel: TaskManager::fetch_task");
    TASK_MANAGER.exclusive_access().fetch()
}

/// 根据stride调度算法获取下一个进程
pub fn fetch_task_with_stride() -> Option<Arc<TaskControlBlock>> {
    //trace!("kernel: TaskManager::fetch_task_with_stride");
    TASK_MANAGER.exclusive_access().fetch_with_stride()
}
